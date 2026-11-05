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
//! preset as it is. Both start from a power-on simulator and render frame 0, so a
//! full render is byte for byte what `vcb render` writes for the same input and
//! preset.
//!
//! TODO: build the simulator once and update it with `Simulator::set_preset` when the
//! core gains it (plan stage 7); until then every job builds a new one.

use std::any::Any;
use std::fmt::{self, Debug, Formatter};
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::ColorImage;
use vcb_core::plane::Plane;
use vcb_core::preset::Resolution;
use vcb_core::scene::Channel;
use vcb_core::{Camera, Frame, Preset, Scene, Simulator, Timing};

use crate::viewport;

/// Long edge of a preview render in pixels.
///
/// Small enough for a preview in a few tens of milliseconds on a laptop, large enough
/// that the bus bend's line structure stays readable in a fitted view.
pub const PREVIEW_LONG_EDGE: u32 = 720;

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
    /// Wall-clock time of the render, building the simulator included.
    pub elapsed: Duration,
}

impl Debug for Output {
    /// Prints the shape only; the image holds millions of pixels.
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Output")
            .field("frame", &self.frame)
            .field("sensor", &self.sensor)
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

/// A thread that runs the newest job it has been sent and replies with the result.
///
/// Jobs that arrive while a job runs wait in a queue; before the next job starts, the
/// thread drains the queue and keeps only the newest, so only the most recent request
/// is ever answered after a burst. After each reply the thread calls `notify`, which
/// the app uses to wake the UI. Dropping the worker closes the queue; the thread
/// finishes its current job and ends.
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
        mut work: impl FnMut(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        let (jobs, queue) = mpsc::channel::<J>();
        let (answers, replies) = mpsc::channel::<R>();
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                while let Ok(mut job) = queue.recv() {
                    while let Ok(newer) = queue.try_recv() {
                        job = newer;
                    }
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

/// Renders jobs: the worker's state, which keeps the reduced preview scene.
#[derive(Debug, Default)]
pub struct Renderer {
    cache: Option<Reduced>,
}

/// A reduced copy of a source scene for previews.
#[derive(Debug)]
struct Reduced {
    source: Arc<Scene>,
    aspect: [u32; 2],
    scene: Arc<Scene>,
}

impl Renderer {
    /// Renders `job` and logs the result; a panic in the simulation becomes an error.
    pub fn render(&mut self, job: &Job) -> Reply {
        let outcome =
            panic::catch_unwind(AssertUnwindSafe(|| self.run(job))).unwrap_or_else(|payload| {
                Err(format!("the simulation failed: {}", panic_text(&*payload)))
            });
        match &outcome {
            Ok(output) => tracing::info!(
                name: "app.render.done",
                generation = job.generation,
                preview = job.preview,
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

    fn run(&mut self, job: &Job) -> Result<Output, String> {
        let started = Instant::now();
        let camera = (*job.camera).clone();
        let (scene, preset) = if job.preview {
            let full = Timing::derive(&camera, &job.preset, job.scene.width(), job.scene.height())
                .map_err(|error| error.to_string())?;
            let aspect = [full.output_width, full.output_height];
            (
                self.reduced(&job.scene, aspect),
                preview_preset(&job.preset),
            )
        } else {
            (Arc::clone(&job.scene), job.preset.clone())
        };
        let mut simulator = Simulator::builder((camera, preset))
            .input_size(scene.width(), scene.height())
            .build()
            .map_err(|error| error.to_string())?;
        let timing = simulator.timing();
        let sensor = [timing.sensor_width, timing.sensor_height];
        let frame = simulator.frame(&scene, 0).clone();
        let image = viewport::texture_image(frame.width(), frame.height(), frame.rgb());
        Ok(Output {
            frame: Arc::new(frame),
            image,
            sensor,
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
        let ratio = f64::from(aspect[0].max(1)) / f64::from(aspect[1].max(1));
        let scene = Arc::new(reduce(source, ratio, PREVIEW_LONG_EDGE));
        self.cache = Some(Reduced {
            source: Arc::clone(source),
            aspect,
            scene: Arc::clone(&scene),
        });
        scene
    }
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
