//! The render worker: a thread that owns the simulation and renders the newest job.
//!
//! The UI never renders. It sends a [`Job`] for every change, numbered by a
//! generation counter, and the [`Worker`] thread drains its queue to the newest job
//! before it starts, so a burst of changes costs one render. Each [`Reply`] carries
//! the job's generation, and [`Generations`] decides on the UI side which replies are
//! still worth showing.
//!
//! A job is either a preview or a full render. A preview crops the scene to the full
//! render's aspect ratio, reduces it so the long edge is [`PREVIEW_LONG_EDGE`] pixels,
//! and renders it at match-input resolution, which costs about a tenth of a native
//! render; the reduced scene is cached per source scene. A full render uses the
//! preset as it is.
//!
//! The [`Renderer`] keeps two running simulators, one for previews and one for full
//! renders, each for one camera and scene size. A new preset reaches them through
//! [`Simulator::set_preset`] and a new scene size through
//! [`Simulator::set_scene_size`], so the camera's analog memory (oscillator phases,
//! capacitor charge, servos, running clock faults) carries from one render to the
//! next as in a camera that keeps running, and each render is the next frame of its
//! simulator's sequence. Each [`Output`] carries the [`Reconfigured`] report of that
//! update: a new readout geometry rebuilds the simulator at power-on and restarts the
//! sequence, and rewiring the bus bend resets the bend alone. A job whose
//! [`power_cycle`](Job::power_cycle) count is higher than any seen before resets both
//! simulators to their power-on state first; the full render after a power cycle is
//! frame 0 of a fresh simulator, byte for byte what `vcb render` writes for the same
//! input and preset.
//!
//! The gallery's [`Thumbnail`]s go to the same worker as [`Request::Thumbnail`], in
//! the worker's [`Lane::Background`]: they wait in order and run one at a time, only
//! while no preview or full render waits, so a thumbnail delays a render by at most
//! the one thumbnail in progress. A thumbnail crops and reduces the scene as a preview
//! does, to [`GALLERY_LONG_EDGE`] pixels, and renders frame 0 of a fresh simulator at
//! match-input resolution, which is dropped afterwards: the frame `vcb render`
//! computes for the reduced scene, and the running simulators never see it.

use std::any::Any;
use std::collections::VecDeque;
use std::fmt::{self, Debug, Formatter};
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::ColorImage;
use vcb_core::plane::Plane;
use vcb_core::preset::Resolution;
use vcb_core::scene::Channel;
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing};

use crate::presets::Entry;
use crate::viewport;

/// Long edge of a preview render in pixels.
///
/// Small enough for a preview in a few tens of milliseconds on a laptop, large enough
/// that the bus bend's line structure stays readable in a fitted view.
pub const PREVIEW_LONG_EDGE: u32 = 720;

/// Long edge of a gallery thumbnail in pixels.
///
/// The legacy gallery's size, a little larger than a tile's image on a standard
/// density screen; a thumbnail costs about a sixth of a preview, so two dozen presets
/// fill in well within a second.
pub const GALLERY_LONG_EDGE: u32 = 300;

/// One render request.
#[derive(Debug, Clone)]
pub struct Job {
    /// Position of the job in the UI's sequence of requests.
    pub generation: u64,
    /// The scene in front of the lens, at its own size.
    pub scene: Arc<Scene>,
    /// The camera the preset names.
    pub camera: Arc<Camera>,
    /// The preset to render.
    pub preset: Preset,
    /// Whether to render a reduced preview instead of the full preset.
    pub preview: bool,
    /// How often the UI has asked to power-cycle the camera; a new count resets both simulators.
    pub power_cycle: u64,
}

/// A finished render.
#[derive(Clone)]
pub struct Output {
    /// The camera's output frame.
    pub frame: Arc<Frame>,
    /// The frame as an egui image, ready to upload as a texture.
    pub image: ColorImage,
    /// Size of the virtual sensor that rendered it, in photosites.
    pub sensor: [u32; 2],
    /// Index of the frame in its simulator's sequence since power-on, from 0.
    pub index: u64,
    /// What configuring the simulator for this job kept; [`Reconfigured::Rebuilt`] for a new one.
    pub reconfigured: Reconfigured,
    /// Wall-clock time of the render, configuring the simulator included.
    pub elapsed: Duration,
}

impl Debug for Output {
    /// Prints the shape only; the image holds millions of pixels.
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Output")
            .field("frame", &self.frame)
            .field("sensor", &self.sensor)
            .field("index", &self.index)
            .field("reconfigured", &self.reconfigured)
            .field("elapsed", &self.elapsed)
            .finish_non_exhaustive()
    }
}

/// The worker's answer to a [`Job`].
#[derive(Debug, Clone)]
pub struct Reply {
    /// The generation of the job.
    pub generation: u64,
    /// Whether the job was a preview.
    pub preview: bool,
    /// The render, or why it failed.
    pub outcome: Result<Output, String>,
}

/// A gallery thumbnail to render: a preset on a reduced copy of the scene.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    /// The gallery's number of the request, which the reply carries back.
    pub ticket: u64,
    /// The preset file the thumbnail shows.
    pub entry: Entry,
    /// The preset as read from the file.
    pub preset: Preset,
    /// The camera the preset names.
    pub camera: Arc<Camera>,
    /// The scene in front of the lens, at its own size; the worker reduces it.
    pub scene: Arc<Scene>,
    /// The UI's count of scene changes when it asked, to tell stale thumbnails.
    pub scene_generation: u64,
}

/// The worker's answer to a [`Thumbnail`].
#[derive(Debug, Clone)]
pub struct ThumbnailReply {
    /// The ticket of the request.
    pub ticket: u64,
    /// The preset file the thumbnail shows.
    pub path: PathBuf,
    /// The camera it was rendered through.
    pub camera: String,
    /// The scene generation of the request.
    pub scene_generation: u64,
    /// The thumbnail, frame 0 of a fresh simulator, or why it failed.
    pub outcome: Result<Output, String>,
}

/// What the UI asks of the render worker.
#[derive(Debug, Clone)]
pub enum Request {
    /// A preview or a full render; only the newest waits, ahead of every thumbnail.
    Render(Job),
    /// A gallery thumbnail; thumbnails wait in order behind any render.
    Thumbnail(Thumbnail),
}

impl Request {
    /// The worker lane the request waits in.
    #[must_use]
    pub fn lane(&self) -> Lane {
        match self {
            Self::Render(_) => Lane::Newest,
            Self::Thumbnail(_) => Lane::Background,
        }
    }
}

/// The render worker's answer to a [`Request`].
#[derive(Debug, Clone)]
pub enum Answer {
    /// The answer to a preview or full render.
    Render(Reply),
    /// The answer to a thumbnail.
    Thumbnail(ThumbnailReply),
}

/// Which of the worker's two queues a job waits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lane {
    /// Only the newest such job waits, and it runs before any background job.
    Newest,
    /// Every such job waits, in order, and runs only while no newest job does.
    Background,
}

/// The jobs a worker thread has taken from its channel and not yet run.
#[derive(Debug)]
struct Pending<J> {
    newest: Option<J>,
    background: VecDeque<J>,
}

impl<J> Pending<J> {
    fn is_empty(&self) -> bool {
        self.newest.is_none() && self.background.is_empty()
    }

    /// Queues `job` in `lane`; a newest job replaces the one waiting.
    fn push(&mut self, job: J, lane: Lane) {
        match lane {
            Lane::Newest => self.newest = Some(job),
            Lane::Background => self.background.push_back(job),
        }
    }

    /// The job to run next: the newest job, else the oldest background job.
    fn pop(&mut self) -> Option<J> {
        self.newest.take().or_else(|| self.background.pop_front())
    }
}

/// A thread that runs the newest job it has been sent and replies with the result.
///
/// Jobs that arrive while a job runs wait in a queue; before the next job starts, the
/// thread drains the queue and keeps only the newest, so only the most recent request
/// is ever answered after a burst. A worker started with
/// [`spawn_lanes`](Self::spawn_lanes) also keeps background jobs, every one in order,
/// and runs one of them per pass while no newest job waits. After each reply the
/// thread calls `notify`, which the app uses to wake the UI. Dropping the worker
/// closes the queue; the thread finishes its current job and ends.
#[derive(Debug)]
pub struct Worker<J, R> {
    jobs: Sender<J>,
    replies: Receiver<R>,
}

impl<J: Send + 'static, R: Send + 'static> Worker<J, R> {
    /// Starts a worker thread called `name` that answers each job with `work`.
    ///
    /// # Errors
    ///
    /// Returns the operating system's error if the thread cannot be started.
    pub fn spawn(
        name: &str,
        work: impl FnMut(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        Self::spawn_lanes(name, |_: &J| Lane::Newest, work, notify)
    }

    /// Starts a worker thread that sorts each job into the lane `lane` names.
    ///
    /// Before each job the thread takes every job sent so far; the newest job of
    /// [`Lane::Newest`] runs first, and the oldest of [`Lane::Background`] only when
    /// none waits, so a background job delays a newest one by at most the one in
    /// progress.
    ///
    /// # Errors
    ///
    /// Returns the operating system's error if the thread cannot be started.
    pub fn spawn_lanes(
        name: &str,
        lane: impl Fn(&J) -> Lane + Send + 'static,
        mut work: impl FnMut(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        let (jobs, queue) = mpsc::channel::<J>();
        let (answers, replies) = mpsc::channel::<R>();
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let mut pending = Pending {
                    newest: None,
                    background: VecDeque::new(),
                };
                loop {
                    if pending.is_empty() {
                        let Ok(job) = queue.recv() else { break };
                        let lane = lane(&job);
                        pending.push(job, lane);
                    }
                    for job in queue.try_iter() {
                        let lane = lane(&job);
                        pending.push(job, lane);
                    }
                    let Some(job) = pending.pop() else { continue };
                    if answers.send(work(job)).is_err() {
                        break;
                    }
                    notify();
                }
            })?;
        Ok(Self { jobs, replies })
    }

    /// Queues a job; returns `false` if the worker thread has ended.
    pub fn submit(&self, job: J) -> bool {
        self.jobs.send(job).is_ok()
    }

    /// Returns the next reply, if one has arrived.
    #[must_use]
    pub fn poll(&self) -> Option<R> {
        self.replies.try_recv().ok()
    }
}

/// The UI's bookkeeping of job generations: which replies to show.
///
/// Generations increase with every job. A reply is shown if it is newer than the
/// frame on screen and was requested after the last [`invalidate`](Self::invalidate),
/// which the app calls when the scene changes. Replies older than the newest request
/// are still shown, since during a drag they are the closest view of the controls;
/// [`is_latest`](Self::is_latest) tells whether a frame answers the newest request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Generations {
    latest: u64,
    floor: u64,
    shown: Option<u64>,
    answered: u64,
}

impl Generations {
    /// Starts a new generation for a job and returns it.
    pub fn issue(&mut self) -> u64 {
        self.latest += 1;
        self.latest
    }

    /// The generation of the newest job.
    #[must_use]
    pub fn latest(&self) -> u64 {
        self.latest
    }

    /// Whether `generation` is the newest job.
    #[must_use]
    pub fn is_latest(&self, generation: u64) -> bool {
        generation == self.latest
    }

    /// Whether the newest job has not been answered yet.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.answered < self.latest
    }

    /// Marks every job requested so far as stale and forgets the frame on screen.
    pub fn invalidate(&mut self) {
        self.floor = self.latest + 1;
        self.shown = None;
    }

    /// Records a reply and returns whether it should replace the frame on screen.
    pub fn accept(&mut self, generation: u64) -> bool {
        self.answered = self.answered.max(generation);
        let stale = generation < self.floor || self.shown.is_some_and(|shown| generation <= shown);
        if !stale {
            self.shown = Some(generation);
        }
        !stale
    }
}

/// Returns whether a preview of `preset` would differ from its full render.
///
/// A match-input preset on a scene no larger than the preview renders the same frame
/// either way, so it needs no preview.
#[must_use]
pub fn needs_preview(preset: &Preset, scene: &Scene) -> bool {
    preset.readout.resolution == Resolution::Native
        || scene.width().max(scene.height()) > PREVIEW_LONG_EDGE
}

/// Returns the preset of a preview: the same preset at match-input resolution.
#[must_use]
pub fn preview_preset(preset: &Preset) -> Preset {
    let mut preview = preset.clone();
    preview.readout.resolution = Resolution::MatchInput;
    preview
}

/// Renders jobs: the worker's state, which keeps the running simulators.
#[derive(Debug, Default)]
pub struct Renderer {
    cache: Option<Reduced>,
    thumbnail_cache: Option<Shrunk>,
    preview: Option<Live>,
    full: Option<Live>,
    power_cycle: u64,
}

/// A reduced copy of a source scene for previews.
#[derive(Debug)]
struct Reduced {
    source: Arc<Scene>,
    aspect: [u32; 2],
    scene: Arc<Scene>,
}

/// A reduced copy of a source scene for thumbnails.
///
/// The source is held weakly, so a scene the UI has replaced is freed even while no
/// thumbnail comes to replace this copy; the weak reference keeps the allocation, so
/// a new scene never takes the old one's address.
#[derive(Debug)]
struct Shrunk {
    source: Weak<Scene>,
    aspect: [u32; 2],
    scene: Arc<Scene>,
}

/// A running simulator and the camera it was built for.
#[derive(Debug)]
struct Live {
    camera: Arc<Camera>,
    simulator: Simulator,
    /// Frames rendered since power-on.
    frames: u64,
}

impl Live {
    /// Returns the simulator in `slot` configured for `camera`, `preset` and `size`.
    ///
    /// A simulator for the same camera is updated in place, keeping its analog state
    /// where [`Simulator::set_preset`] allows; another camera, or a configuration the
    /// running simulator refuses, builds a new one, whose error is then reported.
    /// Returns the simulator and what the update kept ([`Reconfigured::Rebuilt`] for a
    /// new simulator).
    fn configure<'a>(
        slot: &'a mut Option<Self>,
        camera: &Arc<Camera>,
        preset: &Preset,
        size: (u32, u32),
    ) -> Result<(&'a mut Self, Reconfigured), String> {
        let updated = match slot.as_mut() {
            Some(live) if Arc::ptr_eq(&live.camera, camera) || *live.camera == **camera => {
                live.update(preset, size)
            }
            _ => None,
        };
        let reconfigured = if let Some(reconfigured) = updated {
            reconfigured
        } else {
            let simulator = Simulator::builder(((**camera).clone(), preset.clone()))
                .input_size(size.0, size.1)
                .build()
                .map_err(|error| error.to_string())?;
            *slot = Some(Self {
                camera: Arc::clone(camera),
                simulator,
                frames: 0,
            });
            Reconfigured::Rebuilt
        };
        let live = slot
            .as_mut()
            .expect("the slot holds a configured simulator");
        Ok((live, reconfigured))
    }

    /// Applies `size` and `preset` to the running simulator; `None` if it refuses either.
    ///
    /// A rebuilt simulator is at power-on, so the frame count restarts.
    fn update(&mut self, preset: &Preset, size: (u32, u32)) -> Option<Reconfigured> {
        let sim = &mut self.simulator;
        let mut reconfigured = UNCHANGED;
        if sim.scene_size() != size {
            reconfigured = merge(reconfigured, sim.set_scene_size(size.0, size.1).ok()?);
        }
        if sim.preset() != preset {
            reconfigured = merge(reconfigured, sim.set_preset(preset.clone()).ok()?);
        }
        if reconfigured == Reconfigured::Rebuilt {
            self.frames = 0;
        }
        Some(reconfigured)
    }
}

/// A reconfiguration that kept everything.
const UNCHANGED: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

/// What two reconfigurations in a row kept together.
fn merge(first: Reconfigured, second: Reconfigured) -> Reconfigured {
    match (first, second) {
        (Reconfigured::Rebuilt, _) | (_, Reconfigured::Rebuilt) => Reconfigured::Rebuilt,
        (
            Reconfigured::Kept {
                bus_reset: bus_a,
                servo_reset: servo_a,
            },
            Reconfigured::Kept {
                bus_reset: bus_b,
                servo_reset: servo_b,
            },
        ) => Reconfigured::Kept {
            bus_reset: bus_a || bus_b,
            servo_reset: servo_a || servo_b,
        },
    }
}

impl Renderer {
    /// Answers `request`: a render on the running simulators, or a thumbnail.
    pub fn answer(&mut self, request: &Request) -> Answer {
        match request {
            Request::Render(job) => Answer::Render(self.render(job)),
            Request::Thumbnail(job) => Answer::Thumbnail(self.thumbnail(job)),
        }
    }

    /// Renders the thumbnail `job` on a fresh simulator and logs the result.
    ///
    /// The simulator is built for the job and dropped after it, so the running
    /// simulators keep their state and only one thumbnail's simulator exists at a
    /// time; a panic in the simulation becomes an error.
    pub fn thumbnail(&mut self, job: &Thumbnail) -> ThumbnailReply {
        let outcome =
            panic::catch_unwind(AssertUnwindSafe(|| self.sketch(job))).unwrap_or_else(|payload| {
                Err(format!("the simulation failed: {}", panic_text(&*payload)))
            });
        match &outcome {
            Ok(output) => tracing::info!(
                name: "app.thumbnail.done",
                preset = %job.entry.name,
                width = output.frame.width(),
                height = output.frame.height(),
                elapsed_ms = output.elapsed.as_secs_f64() * 1e3,
                "thumbnail done"
            ),
            Err(message) => tracing::warn!(
                name: "app.thumbnail.failed",
                preset = %job.entry.name,
                error = %message,
                "thumbnail failed"
            ),
        }
        ThumbnailReply {
            ticket: job.ticket,
            path: job.entry.path.clone(),
            camera: job.preset.camera.clone(),
            scene_generation: job.scene_generation,
            outcome,
        }
    }

    fn sketch(&mut self, job: &Thumbnail) -> Result<Output, String> {
        let started = Instant::now();
        let aspect = output_aspect(&job.camera, &job.preset, &job.scene)?;
        let scene = self.thumbnail_scene(&job.scene, aspect);
        let mut simulator =
            Simulator::builder(((*job.camera).clone(), preview_preset(&job.preset)))
                .input_size(scene.width(), scene.height())
                .build()
                .map_err(|error| error.to_string())?;
        let timing = simulator.timing();
        let sensor = [timing.sensor_width, timing.sensor_height];
        let frame = simulator.frame(&scene, 0).clone();
        drop(simulator);
        let image = viewport::texture_image(frame.width(), frame.height(), frame.rgb());
        Ok(Output {
            frame: Arc::new(frame),
            image,
            sensor,
            index: 0,
            reconfigured: Reconfigured::Rebuilt,
            elapsed: started.elapsed(),
        })
    }

    /// Returns the thumbnail scene of `source` for an output of shape `aspect`, cached.
    fn thumbnail_scene(&mut self, source: &Arc<Scene>, aspect: [u32; 2]) -> Arc<Scene> {
        if let Some(cache) = &self.thumbnail_cache
            && cache.source.as_ptr() == Arc::as_ptr(source)
            && cache.aspect == aspect
        {
            return Arc::clone(&cache.scene);
        }
        let scene = Arc::new(reduce(source, ratio(aspect), GALLERY_LONG_EDGE));
        self.thumbnail_cache = Some(Shrunk {
            source: Arc::downgrade(source),
            aspect,
            scene: Arc::clone(&scene),
        });
        scene
    }

    /// Renders `job` and logs the result; a panic in the simulation becomes an error.
    pub fn render(&mut self, job: &Job) -> Reply {
        let outcome =
            panic::catch_unwind(AssertUnwindSafe(|| self.run(job))).unwrap_or_else(|payload| {
                // A simulator that panicked mid-frame holds half-written state.
                self.preview = None;
                self.full = None;
                Err(format!("the simulation failed: {}", panic_text(&*payload)))
            });
        match &outcome {
            Ok(output) => tracing::info!(
                name: "app.render.done",
                generation = job.generation,
                preview = job.preview,
                index = output.index,
                width = output.frame.width(),
                height = output.frame.height(),
                elapsed_ms = output.elapsed.as_secs_f64() * 1e3,
                "render done"
            ),
            Err(message) => tracing::warn!(
                name: "app.render.failed",
                generation = job.generation,
                preview = job.preview,
                error = %message,
                "render failed"
            ),
        }
        Reply {
            generation: job.generation,
            preview: job.preview,
            outcome,
        }
    }

    /// Returns both simulators to their power-on state.
    pub fn power_cycle(&mut self) {
        for live in [&mut self.preview, &mut self.full].into_iter().flatten() {
            live.simulator.reset();
            live.frames = 0;
        }
    }

    fn run(&mut self, job: &Job) -> Result<Output, String> {
        let started = Instant::now();
        if job.power_cycle > self.power_cycle {
            self.power_cycle = job.power_cycle;
            self.power_cycle();
        }
        let (scene, preset) = if job.preview {
            let aspect = output_aspect(&job.camera, &job.preset, &job.scene)?;
            (
                self.reduced(&job.scene, aspect),
                preview_preset(&job.preset),
            )
        } else {
            (Arc::clone(&job.scene), job.preset.clone())
        };
        let slot = if job.preview {
            &mut self.preview
        } else {
            &mut self.full
        };
        let size = (scene.width(), scene.height());
        let (live, reconfigured) = Live::configure(slot, &job.camera, &preset, size)?;
        let timing = live.simulator.timing();
        let sensor = [timing.sensor_width, timing.sensor_height];
        let index = live.frames;
        let frame = live.simulator.frame(&scene, index).clone();
        live.frames += 1;
        let image = viewport::texture_image(frame.width(), frame.height(), frame.rgb());
        Ok(Output {
            frame: Arc::new(frame),
            image,
            sensor,
            index,
            reconfigured,
            elapsed: started.elapsed(),
        })
    }

    /// Returns the preview scene of `source` for an output of shape `aspect`, cached.
    fn reduced(&mut self, source: &Arc<Scene>, aspect: [u32; 2]) -> Arc<Scene> {
        if let Some(cache) = &self.cache
            && Arc::ptr_eq(&cache.source, source)
            && cache.aspect == aspect
        {
            return Arc::clone(&cache.scene);
        }
        let scene = Arc::new(reduce(source, ratio(aspect), PREVIEW_LONG_EDGE));
        self.cache = Some(Reduced {
            source: Arc::clone(source),
            aspect,
            scene: Arc::clone(&scene),
        });
        scene
    }
}

/// The size of the full render of `preset` on `scene`, whose shape previews crop to.
fn output_aspect(camera: &Camera, preset: &Preset, scene: &Scene) -> Result<[u32; 2], String> {
    let full = Timing::derive(camera, preset, scene.width(), scene.height())
        .map_err(|error| error.to_string())?;
    Ok([full.output_width, full.output_height])
}

/// Width over height of an output of size `aspect`.
fn ratio(aspect: [u32; 2]) -> f64 {
    f64::from(aspect[0].max(1)) / f64::from(aspect[1].max(1))
}

/// Crops `scene` centrally to `aspect` (width over height) and shrinks it to `long_edge`.
///
/// Shrinking averages the light over each output pixel's footprint, as a photosite
/// integrates it; a scene whose cropped long edge is already within `long_edge` is
/// only cropped. Both edges of the result are at least 2 pixels.
///
/// # Panics
///
/// Panics if `aspect` is not positive and finite.
#[must_use]
pub fn reduce(scene: &Scene, aspect: f64, long_edge: u32) -> Scene {
    assert!(
        aspect.is_finite() && aspect > 0.0,
        "aspect ratio must be positive, got {aspect}"
    );
    let (width, height) = (scene.width(), scene.height());
    let (crop_w, crop_h) = if f64::from(width) / f64::from(height) > aspect {
        (
            ((f64::from(height) * aspect).round() as u32).clamp(1, width),
            height,
        )
    } else {
        (
            width,
            ((f64::from(width) / aspect).round() as u32).clamp(1, height),
        )
    };
    let x0 = (width - crop_w) / 2;
    let y0 = (height - crop_h) / 2;
    let scale = (f64::from(long_edge) / f64::from(crop_w.max(crop_h))).min(1.0);
    let out_w = ((f64::from(crop_w) * scale).round() as u32).clamp(2.min(crop_w), crop_w);
    let out_h = ((f64::from(crop_h) * scale).round() as u32).clamp(2.min(crop_h), crop_h);

    let columns = taps(x0, crop_w, out_w);
    let rows = taps(y0, crop_h, out_h);
    let mut out = Scene::new(out_w, out_h);
    let mut across = vec![0.0f32; out_w as usize * height as usize];
    for channel in Channel::ALL {
        shrink_rows(scene.plane(channel), &columns, out_w, &mut across);
        let plane = out.plane_mut(channel);
        for (y, tap) in rows.iter().enumerate() {
            let row = plane.row_mut(y as u32);
            row.fill(0.0);
            for (k, weight) in tap.weights.iter().enumerate() {
                let start = (tap.start + k) * out_w as usize;
                let source = &across[start..start + out_w as usize];
                for (value, sample) in row.iter_mut().zip(source) {
                    *value += weight * sample;
                }
            }
        }
    }
    out
}

/// Shrinks every row of `plane` horizontally into `across`, `out_w` values per row.
fn shrink_rows(plane: &Plane<f32>, columns: &[Tap], out_w: u32, across: &mut [f32]) {
    for (row, out) in plane.rows().zip(across.chunks_exact_mut(out_w as usize)) {
        for (value, tap) in out.iter_mut().zip(columns) {
            *value = tap
                .weights
                .iter()
                .zip(&row[tap.start..])
                .map(|(weight, sample)| weight * sample)
                .sum();
        }
    }
}

/// The source samples that make up one output sample and their weights.
#[derive(Debug, Clone, PartialEq)]
struct Tap {
    start: usize,
    weights: Vec<f32>,
}

/// Area-averaging taps from `len` samples starting at `offset` onto `out` samples.
fn taps(offset: u32, len: u32, out: u32) -> Vec<Tap> {
    let step = f64::from(len) / f64::from(out);
    (0..out)
        .map(|i| {
            let from = f64::from(i) * step;
            let to = from + step;
            let first = from.floor() as u32;
            let last = (to.ceil() as u32).min(len);
            let weights = (first..last)
                .map(|j| {
                    let overlap = (f64::from(j + 1).min(to) - f64::from(j).max(from)).max(0.0);
                    (overlap / step) as f32
                })
                .collect();
            Tap {
                start: (offset + first) as usize,
                weights,
            }
        })
        .collect()
}

/// The message of a caught panic.
fn panic_text(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic")
}
