//! The application state and the window layout.
//!
//! The window has a top bar (files, presets, camera, view), the viewport in the
//! centre, the inspector on the right with the signal-path strip above the panels, a
//! transport bar for videos and a status line at the bottom. [`App`] owns the source,
//! the camera, the preset under edit and the frame on screen, and turns every change
//! of the preset into render jobs: a preview at once, and the full render when the
//! drag ends or the preset has been still for [`SETTLE`]. For a video it also owns the
//! decoder and the frame cache, plays the clip through the preview simulator one frame
//! after the other, and runs video exports in the background.
//!
//! Replies from the worker threads are taken in by `App::logic`, which eframe runs
//! before every ui pass and also while the window is hidden and no ui pass runs, so a
//! render that finishes behind other windows is shown once the window comes back.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, CentralPanel, ComboBox, CornerRadius, Layout, Margin, Panel, ProgressBar,
    RichText, ScrollArea, Slider, Ui, Vec2, ViewportCommand,
};
use vcb_core::{Camera, Frame, Preset, Timing};

use crate::export::{self, Choice, Dialog, Export, Outcome};
use crate::io::{self, Source, Task};
use crate::panels::{self, Edit, PanelId, Panels};
use crate::presets::{self, Current, Entry};
use crate::render::{self, Generations, Job, Renderer, Reply, Worker};
use crate::shortcuts::{self, Action};
use crate::theme;
use crate::video::{self, Clip, Decoded, FrameCache, Next, PlayStream, VideoFrame};
use crate::viewport::{self, Picture, Texture, View};

/// How long the preset must stay unchanged, outside a drag, before the full render.
pub const SETTLE: Duration = Duration::from_millis(150);

/// Width of the inspector when the window opens, in points.
const INSPECTOR_WIDTH: f32 = 408.0;

/// Environment variable naming a PNG file: after the first full render the app saves
/// the window to it and quits, for checking the layout without a person at the screen.
pub const SCREENSHOT_ENV: &str = "VCB_APP_SCREENSHOT";

/// Frames over which the play-through rate is averaged.
const RATE_WINDOW: usize = 16;

/// How often the status line refreshes while a video export runs.
const EXPORT_REFRESH: Duration = Duration::from_millis(250);

/// How long the status line says "circuit reset" after rewiring reset the bus bend.
///
/// Long enough to be read after letting go of a switch, short enough not to linger.
const CIRCUIT_RESET_NOTICE: Duration = Duration::from_secs(2);

/// The frame on screen and how it was made.
#[derive(Debug, Clone)]
struct Shown {
    generation: u64,
    preview: bool,
    frame: Arc<Frame>,
    sensor: [u32; 2],
    elapsed: Duration,
}

/// The source file and its texture.
#[derive(Debug)]
struct Loaded {
    source: Source,
    original: Texture,
}

/// A line for the status bar.
#[derive(Debug, Clone, Default)]
struct Message {
    text: String,
    error: bool,
}

impl Message {
    fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: true,
        }
    }
}

/// A video in front of the lens: the clip, the frame chosen, and what reads frames.
#[derive(Debug)]
struct Video {
    clip: Clip,
    /// The frame the user has chosen, or the last one played.
    index: u64,
    /// The frame whose full-size scene the source holds.
    loaded: u64,
    decoder: Option<Worker<u64, Decoded>>,
    cache: FrameCache<VideoFrame>,
    play: Option<Play>,
}

/// Play-through: frames streamed at preview size and rendered one after the other.
#[derive(Debug)]
struct Play {
    stream: PlayStream,
    /// The generation and frame index of the render in flight.
    in_flight: Option<(u64, u64)>,
    /// When the most recent frames came back from the renderer.
    times: VecDeque<Instant>,
}

impl Play {
    /// Frames shown per second over the last few frames.
    fn rate(&self) -> Option<f64> {
        let (first, last) = (self.times.front()?, self.times.back()?);
        let span = last.duration_since(*first).as_secs_f64();
        (self.times.len() > 1 && span > 0.0).then(|| (self.times.len() - 1) as f64 / span)
    }
}

/// What the transport bar asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    Step(i64),
    TogglePlay,
    Seek(u64),
}

/// The desktop app: state, render scheduling and layout.
#[derive(Debug)]
pub struct App {
    camera_dir: PathBuf,
    cameras: Vec<Entry>,
    presets: Vec<Entry>,
    camera: Option<Arc<Camera>>,
    preset: Preset,
    current: Current,
    panels: Panels,
    timing: Option<Timing>,
    loaded: Option<Loaded>,
    video: Option<Video>,
    shown: Option<Shown>,
    texture: Option<Texture>,
    worker: Option<Worker<Job, Reply>>,
    generations: Generations,
    latest_full: bool,
    settle: Option<Instant>,
    /// A render waiting for the chosen video frame: whether it is a preview.
    waiting: Option<bool>,
    power_cycle: u64,
    /// Frames the full render's simulator has run since power-on.
    running: Option<u64>,
    export: Option<PathBuf>,
    opening: Option<Task<Source>>,
    writing: Option<Task<PathBuf>>,
    dialog: Option<Dialog>,
    exporting: Option<Export>,
    scroll_to: Option<PanelId>,
    view: View,
    view_rect: egui::Rect,
    message: Message,
    render_error: Option<String>,
    screenshot: Option<PathBuf>,
    title: String,
    /// Whether a control was being dragged in the last ui pass.
    dragging: bool,
    /// When a render last reported that rewiring returned the bus bend to power-on.
    circuit_reset: Option<Instant>,
}

/// What the app starts with: the first file, the camera folder and the screenshot switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    /// The image or video to open first.
    pub path: Option<PathBuf>,
    /// The folder of camera files.
    pub camera_dir: PathBuf,
    /// Where to save the window after the first full render, before quitting.
    pub screenshot: Option<PathBuf>,
}

impl Setup {
    /// The setup the environment asks for: [`presets::camera_dir`] and [`SCREENSHOT_ENV`].
    #[must_use]
    pub fn from_env(path: Option<PathBuf>) -> Self {
        Self {
            path,
            camera_dir: presets::camera_dir(),
            screenshot: std::env::var_os(SCREENSHOT_ENV).map(PathBuf::from),
        }
    }
}

impl App {
    /// Sets up the theme, the render worker and the camera, then opens `path` if given.
    ///
    /// The camera folder and the screenshot switch come from the environment.
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        Self::with_setup(cc, Setup::from_env(path))
    }

    /// Sets up the theme, the render worker and the camera as `setup` says, then opens its file.
    #[must_use]
    pub fn with_setup(cc: &eframe::CreationContext<'_>, setup: Setup) -> Self {
        let Setup {
            path,
            camera_dir,
            screenshot,
        } = setup;
        let ctx = cc.egui_ctx.clone();
        theme::apply(&ctx);
        let waker = ctx.clone();
        let mut renderer = Renderer::default();
        let worker = Worker::spawn(
            "vcb-render",
            move |job: Job| renderer.render(&job),
            move || waker.request_repaint(),
        );
        let mut app = Self {
            cameras: Vec::new(),
            presets: presets::discover(&presets::presets_dir()).unwrap_or_default(),
            camera: None,
            preset: Preset::new(""),
            current: Current::unsaved("untitled", &Preset::new("")),
            panels: Panels::default(),
            timing: None,
            loaded: None,
            video: None,
            shown: None,
            texture: None,
            worker: None,
            generations: Generations::default(),
            latest_full: false,
            settle: None,
            waiting: None,
            power_cycle: 0,
            running: None,
            export: None,
            opening: None,
            writing: None,
            dialog: None,
            exporting: None,
            scroll_to: None,
            view: View::default(),
            view_rect: egui::Rect::NOTHING,
            message: Message::default(),
            render_error: None,
            screenshot,
            title: String::new(),
            dragging: false,
            circuit_reset: None,
            camera_dir,
        };
        match worker {
            Ok(worker) => app.worker = Some(worker),
            Err(error) => {
                app.message = Message::error(format!("cannot start the renderer: {error}"));
            }
        }
        app.start_camera();
        if let Some(path) = path {
            app.open(&ctx, path);
        }
        app
    }

    /// Discovers the cameras and starts with the A520, or the first one, and a clean preset.
    fn start_camera(&mut self) {
        match presets::discover(&self.camera_dir) {
            Ok(cameras) if !cameras.is_empty() => self.cameras = cameras,
            Ok(_) | Err(_) => {
                self.message = Message::error(format!(
                    "no camera files in {}; run from the repository or set {}",
                    self.camera_dir.display(),
                    presets::CAMERAS_ENV
                ));
                return;
            }
        }
        let name = self
            .cameras
            .iter()
            .find(|entry| entry.name == "a520")
            .unwrap_or(&self.cameras[0])
            .name
            .clone();
        match presets::load_camera(&self.camera_dir, &name) {
            Ok(camera) => {
                let mut preset = Preset::new(name);
                preset.seed = 1;
                self.camera = Some(Arc::new(camera));
                self.current = Current::unsaved("untitled", &preset);
                self.panels = Panels::from_preset(&preset);
                self.preset = preset;
            }
            Err(error) => self.message = Message::error(format!("{error:#}")),
        }
    }

    /// Starts opening `path` in the background.
    fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        let name = path.display().to_string();
        match Task::spawn(ctx, move || io::open(&path)) {
            Ok(task) => {
                self.opening = Some(task);
                self.message = Message::info(format!("Opening {name}…"));
            }
            Err(error) => self.message = Message::error(format!("cannot open {name}: {error}")),
        }
    }

    /// Takes in finished file operations, render replies, decoded frames and export news.
    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(result) = self.opening.as_ref().and_then(Task::poll) {
            self.opening = None;
            match result {
                Ok(source) => self.install(ctx, source),
                Err(error) => self.message = Message::error(format!("{error:#}")),
            }
        }
        if let Some(result) = self.writing.as_ref().and_then(Task::poll) {
            self.writing = None;
            self.message = match result {
                Ok(path) => Message::info(format!("Exported {}", path.display())),
                Err(error) => Message::error(format!("{error:#}")),
            };
        }
        while let Some(reply) = self.worker.as_ref().and_then(Worker::poll) {
            self.receive(ctx, reply);
        }
        self.poll_frames();
        if let Some(result) = self.exporting.as_ref().and_then(Export::poll) {
            self.exporting = None;
            self.message = match result {
                Ok(Outcome::Finished { path, frames, size }) => Message::info(format!(
                    "Exported {frames} frames of {} × {} px to {}",
                    size[0],
                    size[1],
                    path.display()
                )),
                Ok(Outcome::Cancelled) => Message::info("Video export cancelled"),
                Err(error) => Message::error(format!("video export failed: {error:#}")),
            };
        }
    }

    /// Takes in frames from the decoder; the chosen frame is put in front of the lens.
    fn poll_frames(&mut self) {
        let Some(video) = &mut self.video else {
            return;
        };
        let mut arrived = None;
        let mut failed = None;
        while let Some(decoded) = video.decoder.as_ref().and_then(Worker::poll) {
            match decoded.frame {
                Ok(frame) => {
                    if decoded.index == video.index {
                        arrived = Some(frame.clone());
                    }
                    video.cache.insert(decoded.index, frame);
                }
                Err(message) if decoded.index == video.index => failed = Some(message),
                Err(_) => {}
            }
        }
        let playing = video.play.is_some();
        let index = video.index;
        if let Some(message) = failed {
            self.waiting = None;
            self.message = Message::error(message);
        }
        if let Some(frame) = arrived
            && !playing
        {
            self.use_frame(index, frame);
            if let Some(preview) = self.waiting.take() {
                self.request(preview);
            }
        }
    }

    /// Makes a freshly opened source the working scene.
    fn install(&mut self, ctx: &egui::Context, mut source: Source) {
        let image = std::mem::take(&mut source.original);
        self.waiting = None;
        self.video = None;
        if let Some(clip) = source.clip.take() {
            let waker = ctx.clone();
            let decoder = match video::decoder(&clip, move || waker.request_repaint()) {
                Ok(decoder) => Some(decoder),
                Err(error) => {
                    self.message =
                        Message::error(format!("cannot start the video decoder: {error}"));
                    None
                }
            };
            let mut cache =
                FrameCache::new(video::cache_capacity(clip.info.width, clip.info.height));
            cache.insert(
                0,
                VideoFrame {
                    scene: Arc::clone(&source.scene),
                    original: image.clone(),
                },
            );
            self.video = Some(Video {
                clip,
                index: 0,
                loaded: 0,
                decoder,
                cache,
                play: None,
            });
        }
        let original = Texture::upload(ctx, "original", image);
        if self.camera.is_some() {
            self.message = Message::default();
        }
        self.loaded = Some(Loaded { source, original });
        self.generations.invalidate();
        self.shown = None;
        self.view.fit();
        self.refresh_timing();
        self.request(true);
    }

    /// Handles one reply of the render worker.
    fn receive(&mut self, ctx: &egui::Context, reply: Reply) {
        let played = self
            .video
            .as_mut()
            .and_then(|video| {
                let play = video.play.as_mut()?;
                let (generation, index) = play.in_flight?;
                (generation == reply.generation).then(|| {
                    play.in_flight = None;
                    play.times.push_back(Instant::now());
                    if play.times.len() > RATE_WINDOW {
                        play.times.pop_front();
                    }
                    video.index = index;
                    index
                })
            })
            .is_some();
        // Even a stale reply reports a reset its simulator went through.
        if let Ok(output) = &reply.outcome
            && output.reconfigured.buffers_kept()
            && output.reconfigured.bus_reset()
        {
            self.circuit_reset = Some(Instant::now());
            ctx.request_repaint_after(CIRCUIT_RESET_NOTICE);
        }
        if !self.generations.accept(reply.generation) {
            return;
        }
        let latest = self.generations.is_latest(reply.generation);
        match reply.outcome {
            Ok(output) => {
                self.render_error = None;
                match &mut self.texture {
                    Some(texture) => texture.set(output.image),
                    None => self.texture = Some(Texture::upload(ctx, "render", output.image)),
                }
                if !reply.preview {
                    self.running = Some(output.index + 1);
                }
                self.shown = Some(Shown {
                    generation: reply.generation,
                    preview: reply.preview,
                    frame: output.frame,
                    sensor: output.sensor,
                    elapsed: output.elapsed,
                });
                if latest && !reply.preview {
                    self.write_pending(ctx);
                    if let Some(path) = self.screenshot.take() {
                        save_screenshot(ctx, path);
                    }
                }
            }
            Err(message) => {
                if latest && self.export.take().is_some() {
                    self.message = Message::error("export cancelled: the full render failed");
                }
                if played {
                    self.stop_play();
                }
                self.render_error = Some(message);
            }
        }
    }

    /// Re-derives the readout timing for the current preset and scene.
    fn refresh_timing(&mut self) {
        self.timing = match (&self.camera, &self.loaded) {
            (Some(camera), Some(loaded)) => Timing::derive(
                camera,
                &self.preset,
                loaded.source.scene.width(),
                loaded.source.scene.height(),
            )
            .ok(),
            _ => None,
        };
    }

    /// Queues a render of the current preset: a preview if one would differ from the full.
    ///
    /// While a video plays, play drives the renders. While the chosen video frame is
    /// still being decoded, the request waits for it.
    fn request(&mut self, preview: bool) {
        if let Some(video) = &self.video {
            if video.play.is_some() {
                return;
            }
            if video.loaded != video.index {
                self.waiting = Some(preview);
                return;
            }
        }
        let (Some(camera), Some(loaded), Some(worker)) = (&self.camera, &self.loaded, &self.worker)
        else {
            return;
        };
        let scene = Arc::clone(&loaded.source.scene);
        let preview = preview && render::needs_preview(&self.preset, &scene);
        let generation = self.generations.issue();
        let job = Job {
            generation,
            scene,
            camera: Arc::clone(camera),
            preset: self.preset.clone(),
            preview,
            power_cycle: self.power_cycle,
        };
        if !worker.submit(job) {
            self.message = Message::error("the renderer stopped; restart the app");
        }
        self.latest_full = !preview;
        self.settle = preview.then(Instant::now);
    }

    /// Requests the full render once the preset has settled.
    fn settle(&mut self, ctx: &egui::Context, dragging: bool) {
        let Some(changed) = self.settle else {
            return;
        };
        let waited = changed.elapsed();
        if dragging {
            return;
        }
        if waited >= SETTLE {
            self.request(false);
        } else {
            ctx.request_repaint_after(SETTLE.saturating_sub(waited));
        }
    }

    /// Whether the frame on screen is the full render of the current preset.
    #[must_use]
    pub fn shows_full_render(&self) -> bool {
        self.full_on_screen().is_some()
    }

    /// The frame on screen if it is the full render of the current preset.
    fn full_on_screen(&self) -> Option<&Shown> {
        self.shown.as_ref().filter(|shown| {
            !shown.preview && self.generations.is_latest(shown.generation) && self.settle.is_none()
        })
    }

    /// Exports the full render to `path`, rendering it first if only a preview is shown.
    fn export_to(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.stop_play();
        self.export = Some(path);
        if self.full_on_screen().is_some() {
            self.write_pending(ctx);
        } else {
            self.message = Message::info("Rendering the full frame for export…");
            if !(self.latest_full && self.generations.is_pending()) {
                self.request(false);
            }
        }
    }

    /// Asks where to export the full render as an image, then exports it.
    fn ask_export_image(&mut self, ctx: &egui::Context) {
        if self.loaded.is_none() || self.camera.is_none() {
            return;
        }
        let source = self
            .loaded
            .as_ref()
            .map_or_else(|| "render".to_owned(), |loaded| stem(&loaded.source.path));
        let frame = self
            .video
            .as_ref()
            .map(|video| format!("-{:05}", video.index + 1))
            .unwrap_or_default();
        let name = format!("{source}{frame}-{}.png", self.current.name);
        if let Some(path) = io::pick_export(&name) {
            self.export_to(ctx, path);
        }
    }

    /// Writes the pending export from the full render on screen.
    fn write_pending(&mut self, ctx: &egui::Context) {
        let Some(frame) = self.full_on_screen().map(|shown| Arc::clone(&shown.frame)) else {
            return;
        };
        let Some(path) = self.export.take() else {
            return;
        };
        let task = Task::spawn(ctx, move || io::export(&path, &frame).map(|()| path));
        match task {
            Ok(task) => {
                self.writing = Some(task);
                self.message = Message::info("Exporting…");
            }
            Err(error) => self.message = Message::error(format!("cannot export: {error}")),
        }
    }

    /// Switches to the camera called `name`.
    fn select_camera(&mut self, name: &str) {
        match presets::load_camera(&self.camera_dir, name) {
            Ok(camera) => {
                self.camera = Some(Arc::new(camera));
                name.clone_into(&mut self.preset.camera);
            }
            Err(error) => self.message = Message::error(format!("{error:#}")),
        }
    }

    /// Makes `preset` the one under edit, loading its camera; errors keep the old state.
    fn adopt(&mut self, preset: Preset, current: Current) {
        let same_camera = self
            .camera
            .as_ref()
            .is_some_and(|camera| camera.info.name == preset.camera);
        if !same_camera {
            match presets::load_camera(&self.camera_dir, &preset.camera) {
                Ok(camera) => self.camera = Some(Arc::new(camera)),
                Err(error) => {
                    self.message = Message::error(format!("{error:#}"));
                    return;
                }
            }
        }
        self.panels.load(&preset);
        self.preset = preset;
        self.current = current;
        self.message = Message::default();
    }

    fn load_preset_file(&mut self, path: PathBuf) {
        match presets::load_preset(&path) {
            Ok(preset) => {
                let name = stem(&path);
                let current = Current {
                    path: Some(path),
                    name,
                    saved: preset.clone(),
                };
                self.adopt(preset, current);
            }
            Err(error) => self.message = Message::error(format!("{error:#}")),
        }
    }

    fn pick_shipped_preset(&mut self, entry: &Entry) {
        match presets::load_preset(&entry.path) {
            Ok(preset) => {
                let current = Current::unsaved(entry.name.clone(), &preset);
                self.adopt(preset, current);
            }
            Err(error) => self.message = Message::error(format!("{error:#}")),
        }
    }

    fn save_preset(&mut self, ask: bool) {
        let path = match (&self.current.path, ask) {
            (Some(path), false) => Some(path.clone()),
            _ => presets::pick_save(&self.current),
        };
        let Some(path) = path else {
            return;
        };
        match presets::save_preset(&path, &self.preset) {
            Ok(()) => {
                self.message = Message::info(format!("Saved {}", path.display()));
                self.current = Current {
                    name: stem(&path),
                    path: Some(path),
                    saved: self.preset.clone(),
                };
                if let Ok(list) = presets::discover(&presets::presets_dir()) {
                    self.presets = list;
                }
            }
            Err(error) => self.message = Message::error(format!("{error:#}")),
        }
    }

    /// Chooses video frame `index` and renders it, decoding it first if needed.
    fn choose_frame(&mut self, index: u64) {
        let Some(video) = &mut self.video else {
            return;
        };
        let index = index.min(video.clip.last());
        if index == video.index && video.loaded == index {
            return;
        }
        video.index = index;
        self.render_chosen(true);
    }

    /// Renders the chosen video frame: at once if its scene is at hand, else once decoded.
    fn render_chosen(&mut self, preview: bool) {
        let Some(video) = &mut self.video else {
            return;
        };
        let index = video.index;
        if video.loaded == index {
            self.request(preview);
        } else if let Some(frame) = video.cache.get(index).cloned() {
            self.use_frame(index, frame);
            self.request(preview);
        } else {
            let submitted = video
                .decoder
                .as_ref()
                .is_some_and(|decoder| decoder.submit(index));
            if submitted {
                self.waiting = Some(preview);
            } else {
                self.message = Message::error("the video decoder stopped; open the video again");
            }
        }
    }

    /// Puts decoded frame `index` in front of the lens.
    fn use_frame(&mut self, index: u64, frame: VideoFrame) {
        if let (Some(video), Some(loaded)) = (&mut self.video, &mut self.loaded) {
            loaded.source.scene = frame.scene;
            loaded.original.set(frame.original);
            video.loaded = index;
        }
    }

    /// Steps the chosen frame by `delta` frames, stopping play first.
    fn step(&mut self, delta: i64) {
        self.stop_play();
        if let Some(video) = &self.video {
            let index = video.index.saturating_add_signed(delta);
            self.choose_frame(index);
        }
    }

    /// Starts or stops playing the video through the preview simulator.
    fn toggle_play(&mut self, ctx: &egui::Context) {
        let Some(video) = &mut self.video else {
            return;
        };
        if video.play.is_some() {
            self.stop_play();
            return;
        }
        let from = if video.index >= video.clip.last() {
            0
        } else {
            video.index + 1
        };
        let source = [video.clip.info.width, video.clip.info.height];
        let aspect = self
            .timing
            .as_ref()
            .map_or(source, |timing| [timing.output_width, timing.output_height]);
        let dimension = video::preview_dimension(source, aspect);
        let waker = ctx.clone();
        match PlayStream::start(&video.clip, from, Some(dimension), move || {
            waker.request_repaint();
        }) {
            Ok(stream) => {
                video.play = Some(Play {
                    stream,
                    in_flight: None,
                    times: VecDeque::with_capacity(RATE_WINDOW + 1),
                });
                self.settle = None;
                self.waiting = None;
            }
            Err(error) => self.message = Message::error(format!("cannot play: {error}")),
        }
    }

    /// Stops play and renders the frame it stopped at in full.
    fn stop_play(&mut self) {
        let stopped = self
            .video
            .as_mut()
            .is_some_and(|video| video.play.take().is_some());
        if stopped {
            self.render_chosen(false);
        }
    }

    /// Sends the next played frame to the renderer once the previous one is back.
    fn drive_play(&mut self) {
        let (Some(video), Some(camera), Some(worker), Some(loaded)) = (
            &mut self.video,
            &self.camera,
            &self.worker,
            &mut self.loaded,
        ) else {
            return;
        };
        let Some(play) = &mut video.play else {
            return;
        };
        if play.in_flight.is_some() {
            return;
        }
        let failure = match play.stream.next() {
            Next::Frame(index, frame) => {
                loaded.original.set(frame.original);
                let generation = self.generations.issue();
                let job = Job {
                    generation,
                    scene: frame.scene,
                    camera: Arc::clone(camera),
                    preset: self.preset.clone(),
                    preview: true,
                    power_cycle: self.power_cycle,
                };
                if worker.submit(job) {
                    play.in_flight = Some((generation, index));
                    self.latest_full = false;
                    None
                } else {
                    Some(Message::error("the renderer stopped; restart the app"))
                }
            }
            Next::Waiting => return,
            Next::Ended => Some(Message::default()),
            Next::Failed(message) => Some(Message::error(format!("cannot play: {message}"))),
        };
        if let Some(message) = failure {
            if !message.text.is_empty() {
                self.message = message;
            }
            self.stop_play();
        }
    }

    /// Opens the video export form for the clip on screen.
    fn open_export_dialog(&mut self) {
        let Some(video) = &self.video else {
            return;
        };
        let directory = video.clip.path.parent().unwrap_or_else(|| Path::new("."));
        let output = directory.join(format!(
            "{}-{}.mp4",
            stem(&video.clip.path),
            self.current.name
        ));
        self.dialog = Some(Dialog::new(&video.clip, &output, 0.0));
    }

    /// Shows the export form, if open, and starts the export it asks for.
    fn show_dialog(&mut self, ctx: &egui::Context) {
        let (Some(dialog), Some(video)) = (&mut self.dialog, &self.video) else {
            self.dialog = None;
            return;
        };
        let current_s = video.clip.time_s(video.index);
        let timing = self.timing.as_ref();
        let response = egui::Modal::new(egui::Id::unique("export-video")).show(ctx, |ui| {
            ui.set_width(theme::GRID * 60.0);
            dialog.show(ui, &video.clip, timing, current_s)
        });
        let choice = if response.should_close() {
            Choice::Close
        } else {
            response.inner
        };
        match choice {
            Choice::Open => {}
            Choice::Close => self.dialog = None,
            Choice::Export => self.start_export(ctx),
        }
    }

    /// Starts the video export the form describes.
    fn start_export(&mut self, ctx: &egui::Context) {
        let (Some(dialog), Some(video), Some(camera)) = (&self.dialog, &self.video, &self.camera)
        else {
            return;
        };
        let settings = match dialog.settings(&video.clip, camera, &self.preset) {
            Ok(settings) => settings,
            Err(error) => {
                self.message = Message::error(format!("{error:#}"));
                return;
            }
        };
        let output = settings.output.display().to_string();
        let waker = ctx.clone();
        match Export::start(settings, move || waker.request_repaint()) {
            Ok(job) => {
                self.exporting = Some(job);
                self.dialog = None;
                self.message = Message::info(format!("Exporting {output}…"));
            }
            Err(error) => {
                self.message = Message::error(format!("cannot start the export: {error}"));
            }
        }
    }

    /// Carries out a keyboard shortcut.
    fn perform(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Open => {
                if let Some(path) = io::pick_source() {
                    self.open(ctx, path);
                }
            }
            Action::SavePreset => self.save_preset(false),
            Action::SavePresetAs => self.save_preset(true),
            Action::ExportImage => self.ask_export_image(ctx),
            Action::Fit => self.view.fit(),
            Action::OneToOne => {
                let content = self.content_size().unwrap_or(Vec2::ZERO);
                self.view.actual_pixels(content, ctx.pixels_per_point());
            }
            Action::NewSeed => self.preset.seed = panels::readout::new_seed(self.preset.seed),
            Action::PreviousFrame => self.step(-1),
            Action::NextFrame => self.step(1),
            Action::Play => self.toggle_play(ctx),
        }
    }

    /// The window title: the file, the preset and an asterisk for unsaved changes.
    fn window_title(&self) -> String {
        let edited = if self.current.is_edited(&self.preset) {
            "*"
        } else {
            ""
        };
        match &self.loaded {
            Some(loaded) => format!(
                "{} — {}{edited} — {}",
                loaded.source.name(),
                self.current.name,
                crate::TITLE
            ),
            None => format!("{}{edited} — {}", self.current.name, crate::TITLE),
        }
    }

    /// The size of the full render, which the viewport shows whatever is on screen.
    fn content_size(&self) -> Option<Vec2> {
        if let Some(timing) = &self.timing {
            return Some(egui::vec2(
                timing.output_width as f32,
                timing.output_height as f32,
            ));
        }
        if let Some(shown) = &self.shown {
            return Some(egui::vec2(
                shown.frame.width() as f32,
                shown.frame.height() as f32,
            ));
        }
        self.loaded.as_ref().map(|loaded| {
            egui::vec2(
                loaded.source.scene.width() as f32,
                loaded.source.scene.height() as f32,
            )
        })
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            if ui
                .button("Open…")
                .on_hover_text(format!(
                    "Open an image or a video ({})",
                    Action::Open.label()
                ))
                .clicked()
                && let Some(path) = io::pick_source()
            {
                self.open(&ctx, path);
            }
            if ui.button("Load preset…").clicked()
                && let Some(path) = presets::pick_open()
            {
                self.load_preset_file(path);
            }
            let save = ui
                .button("Save preset")
                .on_hover_text(match &self.current.path {
                    Some(path) => format!(
                        "Save to {} ({})",
                        path.display(),
                        Action::SavePreset.label()
                    ),
                    None => format!("Save to a new file ({})", Action::SavePreset.label()),
                });
            if save.clicked() {
                self.save_preset(false);
            }
            if ui
                .button("Save as…")
                .on_hover_text(format!(
                    "Save to a new file ({})",
                    Action::SavePresetAs.label()
                ))
                .clicked()
            {
                self.save_preset(true);
            }
            let can_export = self.loaded.is_some() && self.camera.is_some();
            let export = ui
                .add_enabled(can_export, egui::Button::new("Export image…"))
                .on_hover_text(format!(
                    "Write the full render as PNG or JPEG ({})",
                    Action::ExportImage.label()
                ));
            if export.clicked() {
                self.ask_export_image(&ctx);
            }
            let can_export_video = self.video.is_some() && can_export && self.exporting.is_none();
            let export_video = ui
                .add_enabled(can_export_video, egui::Button::new("Export video…"))
                .on_hover_text("Render the clip through the camera into a video file")
                .on_disabled_hover_text(if self.exporting.is_some() {
                    "An export is running"
                } else {
                    "Open a video to export one"
                });
            if export_video.clicked() {
                self.open_export_dialog();
            }
            ui.separator();
            self.pickers(ui);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let content = self.content_size().unwrap_or(Vec2::ZERO);
                if ui
                    .button("1:1")
                    .on_hover_text(format!(
                        "One image pixel per screen pixel ({})",
                        Action::OneToOne.label()
                    ))
                    .clicked()
                {
                    self.view
                        .actual_pixels(content, ui.ctx().pixels_per_point());
                }
                if ui
                    .selectable_label(self.view.is_fit(), "Fit")
                    .on_hover_text(format!("Fit the image ({})", Action::Fit.label()))
                    .clicked()
                {
                    self.view.fit();
                }
            });
        });
    }

    fn pickers(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Camera").color(theme::TEXT_WEAK));
        let mut camera_choice = None;
        ComboBox::from_id_salt("camera")
            .selected_text(self.preset.camera.clone())
            .width(theme::GRID * 10.0)
            .show_ui(ui, |ui| {
                for entry in &self.cameras {
                    if ui
                        .selectable_label(entry.name == self.preset.camera, entry.name.as_str())
                        .clicked()
                    {
                        camera_choice = Some(entry.name.clone());
                    }
                }
            });
        if let Some(name) = camera_choice
            && name != self.preset.camera
        {
            self.select_camera(&name);
        }

        ui.label(RichText::new("Preset").color(theme::TEXT_WEAK));
        let edited = if self.current.is_edited(&self.preset) {
            " (edited)"
        } else {
            ""
        };
        let mut preset_choice = None;
        ComboBox::from_id_salt("preset")
            .selected_text(format!("{}{edited}", self.current.name))
            .width(theme::GRID * 18.0)
            .show_ui(ui, |ui| {
                for entry in &self.presets {
                    if ui
                        .selectable_label(entry.name == self.current.name, entry.name.as_str())
                        .clicked()
                    {
                        preset_choice = Some(entry.clone());
                    }
                }
            });
        if let Some(entry) = preset_choice {
            self.pick_shipped_preset(&entry);
        }
    }

    /// The bar above the status line for a video: step, play, the scrubber and the time.
    fn transport(&mut self, ui: &mut Ui) -> Edit {
        let mut edit = Edit::default();
        let Some(video) = &self.video else {
            return edit;
        };
        let playing = video.play.is_some();
        let rate = video.play.as_ref().and_then(Play::rate);
        let clip = &video.clip;
        let mut index = video.index;
        let mut action = None;
        ui.horizontal(|ui| {
            if ui
                .button("◁")
                .on_hover_text(format!(
                    "Previous frame ({})",
                    Action::PreviousFrame.label()
                ))
                .clicked()
            {
                action = Some(Transport::Step(-1));
            }
            let (symbol, hover) = if playing {
                ("■", "Stop playing")
            } else {
                ("▶", "Play through the preview camera, every frame in turn")
            };
            if ui
                .button(symbol)
                .on_hover_text(format!("{hover} ({})", Action::Play.label()))
                .clicked()
            {
                action = Some(Transport::TogglePlay);
            }
            if ui
                .button("▷")
                .on_hover_text(format!("Next frame ({})", Action::NextFrame.label()))
                .clicked()
            {
                action = Some(Transport::Step(1));
            }
            let readout = format!(
                "{} / {}",
                video::timecode(clip.time_s(index)),
                video::timecode(clip.duration_s())
            );
            let counter = format!("frame {} / {}", index + 1, clip.frames);
            let reserve = theme::GRID * 34.0;
            ui.spacing_mut().slider_width =
                (ui.available_width() - reserve).max(theme::GRID * 12.0);
            let response = ui
                .add(Slider::new(&mut index, 0..=clip.last()).show_value(false))
                .on_hover_text("Scrub through the clip; the full render follows when you let go");
            if response.changed() {
                action = Some(Transport::Seek(index));
            }
            edit.dragging |= response.dragged();
            edit.released |= response.drag_stopped();
            ui.label(RichText::new(readout).monospace().color(theme::TEXT));
            ui.label(RichText::new(counter).color(theme::TEXT_WEAK));
            if let Some(rate) = rate {
                ui.label(RichText::new(format!("{rate:.1} fps")).color(theme::ACCENT));
            }
        });
        let ctx = ui.ctx().clone();
        match action {
            Some(Transport::Step(delta)) => self.step(delta),
            Some(Transport::TogglePlay) => self.toggle_play(&ctx),
            Some(Transport::Seek(index)) => {
                if let Some(video) = &mut self.video {
                    video.play = None;
                }
                self.choose_frame(index);
            }
            None => {}
        }
        edit
    }

    fn status_line(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let weak = |text: String| RichText::new(text).color(theme::TEXT_WEAK);
            match &self.loaded {
                Some(loaded) => {
                    ui.label(loaded.source.name());
                    let (width, height) = match &self.video {
                        Some(video) => (video.clip.info.width, video.clip.info.height),
                        None => (loaded.source.scene.width(), loaded.source.scene.height()),
                    };
                    ui.label(weak(format!(
                        "{width} × {height} px, {}",
                        file_size(loaded.source.bytes)
                    )));
                }
                None => {
                    ui.label(weak("No image".to_owned()));
                }
            }
            if let Some(video) = &self.video {
                ui.separator();
                ui.label(weak(format!(
                    "frame {} / {}, {}",
                    video.index + 1,
                    video.clip.frames,
                    video::timecode(video.clip.time_s(video.index))
                )));
            }
            if let Some(shown) = &self.shown {
                ui.separator();
                ui.label(weak(format!(
                    "sensor {} × {}",
                    shown.sensor[0], shown.sensor[1]
                )));
                ui.separator();
                let playing = self
                    .video
                    .as_ref()
                    .is_some_and(|video| video.play.is_some());
                let kind = match (playing, shown.preview) {
                    (true, _) => "playing",
                    (false, true) => "preview",
                    (false, false) => "full",
                };
                ui.label(RichText::new(kind).color(if shown.preview {
                    theme::TEXT_WEAK
                } else {
                    theme::ACCENT
                }));
                ui.label(weak(format!(
                    "{} × {}, {:.0} ms",
                    shown.frame.width(),
                    shown.frame.height(),
                    shown.elapsed.as_secs_f64() * 1e3
                )));
            }
            self.notices(ui);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let zoom = match self.content_size() {
                    Some(content) if !self.view.is_fit() => format!(
                        "{:.0} %",
                        self.view.zoom(self.view_rect, content)
                            * ui.ctx().pixels_per_point()
                            * 100.0
                    ),
                    _ => "fit".to_owned(),
                };
                ui.label(weak(zoom));
                if let Some(job) = &self.exporting {
                    ui.separator();
                    export_progress(ui, job);
                }
            });
        });
    }

    /// The status line's notices: a recent circuit reset, work in progress, the message.
    fn notices(&self, ui: &mut Ui) {
        if self
            .circuit_reset
            .is_some_and(|at| at.elapsed() < CIRCUIT_RESET_NOTICE)
        {
            ui.separator();
            ui.label(RichText::new("circuit reset").color(theme::ACCENT))
                .on_hover_text(
                    "The bus wiring changed, so the bend's capacitor and gates started from power-on",
                );
        }
        let busy = self.generations.is_pending()
            || self.opening.is_some()
            || self.writing.is_some()
            || self.waiting.is_some();
        if busy {
            ui.spinner();
        }
        if let Some(error) = &self.render_error {
            ui.separator();
            ui.label(RichText::new(error).color(theme::ERROR));
        } else if !self.message.text.is_empty() {
            ui.separator();
            let colour = if self.message.error {
                theme::ERROR
            } else {
                theme::TEXT
            };
            ui.label(RichText::new(&self.message.text).color(colour));
        }
    }

    fn inspector(&mut self, ui: &mut Ui) -> Edit {
        let mut edit = Edit::default();
        if let Some(panel) = panels::strip::show(ui, &self.preset) {
            // The panels below scroll on the next pass.
            self.scroll_to = Some(panel);
            ui.ctx().request_repaint();
        }
        ui.add_space(theme::GRID * 0.5);
        ui.separator();
        let target = self.scroll_to.take();
        let Some(base) = self.camera.clone() else {
            panels::hint(ui, "No camera is loaded.");
            return edit;
        };
        let width = self
            .timing
            .as_ref()
            .map_or(f64::from(base.sensor.native_width), |timing| {
                f64::from(timing.developed_size().0)
            });
        let timing = self.timing.clone();
        let mode = self.preset.readout.mode;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().slider_width = panels::SLIDER_WIDTH;
                ui.add_space(theme::GRID);
                edit |= section(ui, target, PanelId::Scene, |ui| {
                    panels::scene::show(ui, &mut self.preset.scene)
                });
                edit |= section(ui, target, PanelId::Optics, |ui| {
                    self.panels.optics.show(ui, &base)
                });
                edit |= section(ui, target, PanelId::Sensor, |ui| {
                    panels::sensor::show(ui, &mut self.preset, &base)
                });
                edit |= section(ui, target, PanelId::Readout, |ui| {
                    panels::readout::show(ui, &mut self.preset, timing.as_ref(), self.running)
                });
                edit |= section(ui, target, PanelId::Clocks, |ui| {
                    self.panels.clocks.show(ui, &base, timing.as_ref())
                });
                edit |= section(ui, target, PanelId::Afe, |ui| {
                    self.panels.afe.show(ui, timing.as_ref())
                });
                edit |= section(ui, target, PanelId::Bus, |ui| self.panels.bus.show(ui));
                edit |= section(ui, target, PanelId::Clock, |ui| {
                    self.panels.reclock.show(ui, &base, mode, timing.as_ref())
                });
                edit |= section(ui, target, PanelId::Processor, |ui| {
                    panels::isp::show(ui, &mut self.preset, &base, width, target)
                });
                edit |= section(ui, target, PanelId::Memory, |ui| {
                    self.panels.memory.show(ui, &base)
                });
                if target == Some(PanelId::Power) {
                    ui.scroll_to_cursor(Some(Align::TOP));
                }
                edit |= self.panels.rail.show(ui, &base);
                ui.add_space(theme::GRID * 2.0);
            });
        edit
    }

    fn central(&mut self, ui: &mut Ui, show_original: bool) {
        let ctx = ui.ctx().clone();
        let Some(loaded) = &self.loaded else {
            self.empty_state(ui, &ctx);
            return;
        };
        let content = self.content_size().unwrap_or(Vec2::splat(1.0));
        let render = self.shown.as_ref().and(self.texture.as_ref());
        let picture = match render {
            Some(texture) if !show_original => Picture::Render(texture),
            _ => Picture::Original(&loaded.original),
        };
        let response = viewport::show(ui, &mut self.view, content, Some(picture));
        self.view_rect = response.rect;
        let label = if show_original {
            Some("Original")
        } else if render.is_some() {
            None
        } else if self.camera.is_none() {
            Some("Original; no camera loaded")
        } else if self.render_error.is_some() {
            Some("Original; the render failed")
        } else {
            Some("Original, rendering…")
        };
        if let Some(label) = label {
            ui.painter().text(
                response.rect.left_top() + egui::vec2(theme::GRID * 1.5, theme::GRID * 1.5),
                Align2::LEFT_TOP,
                label,
                egui::TextStyle::Body.resolve(ui.style()),
                theme::TEXT,
            );
        }
        drop_overlay(ui, response.rect);
    }

    fn empty_state(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, theme::CANVAS);
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space((rect.height() / 2.0 - theme::GRID * 8.0).max(0.0));
                ui.label(
                    RichText::new("Open a photo or a video to put it in front of the lens")
                        .text_style(egui::TextStyle::Heading)
                        .color(theme::TEXT),
                );
                ui.add_space(theme::GRID);
                if ui.button("Open…").clicked()
                    && let Some(path) = io::pick_source()
                {
                    self.open(ctx, path);
                }
                if let Some(sample) = presets::sample_photo() {
                    ui.add_space(theme::GRID * 0.5);
                    if ui.button("Open the sample photo").clicked() {
                        self.open(ctx, sample);
                    }
                }
                ui.add_space(theme::GRID);
                ui.label(RichText::new("or drop a file on the window").color(theme::TEXT_WEAK));
            });
        });
        drop_overlay(ui, rect);
    }
}

impl eframe::App for App {
    /// Takes in renders, frames, file operations and exports, and schedules the full render.
    ///
    /// eframe calls this before every `ui` pass and also while the window is
    /// hidden or its redraws are withheld, when `ui` is not called at all; so a render
    /// that finishes while nothing is drawn still lands, and the screenshot switch, whose
    /// request makes eframe paint even a hidden window, still fires.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.settle(ctx, self.dragging);
        if self.exporting.is_some() {
            ctx.request_repaint_after(EXPORT_REFRESH);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Play renders only while the window is drawn; `logic` has taken in the replies.
        self.drive_play();
        // Everything below may change the preset: shortcuts, file pickers, panels, camera choice.
        let before = self.preset.clone();
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .first()
                .map(|file| file.path().to_path_buf())
        });
        if let Some(path) = dropped.filter(|path| !path.as_os_str().is_empty()) {
            let is_preset = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"));
            if is_preset {
                self.load_preset_file(path);
            } else {
                self.open(&ctx, path);
            }
        }
        let actions = if self.dialog.is_none() {
            shortcuts::take(&ctx)
        } else {
            Vec::new()
        };
        let show_original = self.dialog.is_none() && shortcuts::holding_original(&ctx);
        for action in actions {
            self.perform(&ctx, action);
        }

        let bar = egui::Frame::new()
            .fill(theme::PANEL)
            .inner_margin(Margin::symmetric(
                theme::GRID as i8,
                (theme::GRID * 0.75) as i8,
            ));
        Panel::top("toolbar")
            .frame(bar)
            .show(ui, |ui| self.toolbar(ui));
        Panel::bottom("status")
            .frame(bar)
            .show(ui, |ui| self.status_line(ui));
        let mut edit = Edit::default();
        if self.video.is_some() {
            edit |= Panel::bottom("transport")
                .frame(bar)
                .show(ui, |ui| self.transport(ui))
                .inner;
        }
        edit |= Panel::right("inspector")
            .resizable(true)
            .default_size(INSPECTOR_WIDTH)
            .min_size(INSPECTOR_WIDTH - theme::GRID * 4.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(Margin::same(theme::GRID as i8 * 2)),
            )
            .show(ui, |ui| self.inspector(ui))
            .inner;
        CentralPanel::no_frame().show(ui, |ui| self.central(ui, show_original));
        self.show_dialog(&ctx);

        if edit.changed {
            self.panels.apply(&mut self.preset);
        }
        if edit.power_cycle {
            self.power_cycle += 1;
            self.running = None;
            self.refresh_timing();
            self.request(false);
        } else if self.preset != before {
            self.refresh_timing();
            self.request(true);
        }
        if edit.released && self.settle.is_some() {
            self.request(false);
        }
        self.dragging = edit.dragging;
        self.settle(&ctx, edit.dragging);
        let title = self.window_title();
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

/// Draws one inspector panel, scrolled into view if it is `target`, and a separator.
fn section(
    ui: &mut Ui,
    target: Option<PanelId>,
    id: PanelId,
    show: impl FnOnce(&mut Ui) -> Edit,
) -> Edit {
    if target == Some(id) {
        ui.scroll_to_cursor(Some(Align::TOP));
    }
    let edit = show(ui);
    ui.add_space(theme::GRID);
    ui.separator();
    ui.add_space(theme::GRID);
    edit
}

/// The export's progress bar, rate, time left and cancel button, right to left.
fn export_progress(ui: &mut Ui, job: &Export) {
    let progress = job.progress();
    if job.is_cancelling() {
        ui.label(RichText::new("cancelling…").color(theme::TEXT_WEAK));
    } else if ui
        .small_button("Cancel")
        .on_hover_text(format!("Stop exporting {}", job.output().display()))
        .clicked()
    {
        job.cancel();
    }
    let rate = if progress.total.is_some_and(|total| progress.done >= total) {
        "finishing the file".to_owned()
    } else {
        let left = progress
            .eta()
            .map_or_else(String::new, |eta| format!(", {} left", export::clock(eta)));
        format!("{:.1} fps{left}", progress.rate())
    };
    ui.label(RichText::new(rate).color(theme::TEXT_WEAK));
    let text = match progress.total {
        Some(total) => format!("{} / {total}", progress.done),
        None => format!("{}", progress.done),
    };
    ui.add(
        ProgressBar::new(progress.fraction().unwrap_or(0.0))
            .desired_width(theme::GRID * 20.0)
            .text(text),
    );
    ui.label(RichText::new("Exporting").color(theme::TEXT));
}

/// Darkens `rect` and asks for a drop while files hover over the window.
fn drop_overlay(ui: &Ui, rect: egui::Rect) {
    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    if hovering {
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            CornerRadius::ZERO,
            egui::Color32::from_black_alpha(160),
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Drop to open",
            egui::TextStyle::Heading.resolve(ui.style()),
            theme::ACCENT,
        );
    }
}

/// Saves the next painted frame of the window to `path` as PNG, then closes the window.
///
/// The window is brought to the front first: macOS does not redraw a window that other
/// windows cover completely, as one started from a background shell may be, and the
/// screenshot is taken only when the window is painted.
fn save_screenshot(ctx: &egui::Context, path: PathBuf) {
    ctx.send_viewport_cmd(ViewportCommand::Focus);
    let closer = ctx.clone();
    ctx.request_screenshot(move |image| {
        let [width, height] = image.size;
        let rgb = image
            .pixels
            .iter()
            .flat_map(|pixel| {
                let [r, g, b, _] = pixel.to_array();
                [r, g, b]
            })
            .collect();
        let frame = Frame::from_rgb(width as u32, height as u32, rgb);
        let path_text = path.display().to_string();
        match vcb_io::write_frame(&path, &frame, vcb_io::Format::Png) {
            Ok(()) => tracing::info!(name: "app.screenshot.done", path = path_text, "saved"),
            Err(error) => tracing::error!(name: "app.screenshot.failed", %error, "failed"),
        }
        closer.send_viewport_cmd(ViewportCommand::Close);
        closer.request_repaint();
    });
    ctx.request_repaint();
}

/// The file stem of `path`, or its whole name.
fn stem(path: &Path) -> String {
    path.file_stem().map_or_else(
        || path.display().to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}

/// A byte count in kB or MB.
fn file_size(bytes: u64) -> String {
    let kb = bytes as f64 / 1e3;
    if kb < 1e3 {
        format!("{kb:.0} kB")
    } else {
        format!("{:.1} MB", kb / 1e3)
    }
}
