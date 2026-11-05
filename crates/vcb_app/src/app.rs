//! The application state and the window layout.
//!
//! The window has a top bar (files, presets, camera, view), the viewport in the
//! centre, the inspector on the right and a status line at the bottom. [`App`] owns the
//! source scene, the camera, the preset under edit and the frame on screen, and
//! turns every change of the preset into render jobs: a preview at once, and the full
//! render when the drag ends or the preset has been still for [`SETTLE`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, CentralPanel, ComboBox, CornerRadius, Key, Layout, Margin, Modifiers,
    Panel, RichText, ScrollArea, Ui, Vec2, ViewportCommand,
};
use vcb_core::{Camera, Frame, Preset, Timing};

use crate::io::{self, Source, Task};
use crate::panels::bus::BusPanel;
use crate::panels::{self, Edit};
use crate::presets::{self, Current, Entry};
use crate::render::{self, Generations, Job, Renderer, Reply, Worker};
use crate::theme;
use crate::viewport::{self, Picture, Texture, View};

/// How long the preset must stay unchanged, outside a drag, before the full render.
pub const SETTLE: Duration = Duration::from_millis(150);

/// Width of the inspector when the window opens, in points.
const INSPECTOR_WIDTH: f32 = 360.0;

/// Environment variable naming a PNG file: after the first full render the app saves
/// the window to it and quits, for checking the layout without a person at the screen.
pub const SCREENSHOT_ENV: &str = "VCB_APP_SCREENSHOT";

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

/// The desktop app: state, render scheduling and layout.
#[derive(Debug)]
pub struct App {
    camera_dir: PathBuf,
    cameras: Vec<Entry>,
    presets: Vec<Entry>,
    camera: Option<Arc<Camera>>,
    preset: Preset,
    current: Current,
    bus: BusPanel,
    timing: Option<Timing>,
    loaded: Option<Loaded>,
    shown: Option<Shown>,
    texture: Option<Texture>,
    worker: Option<Worker<Job, Reply>>,
    generations: Generations,
    latest_full: bool,
    settle: Option<Instant>,
    export: Option<PathBuf>,
    opening: Option<Task<Source>>,
    writing: Option<Task<PathBuf>>,
    view: View,
    view_rect: egui::Rect,
    message: Message,
    render_error: Option<String>,
    screenshot: Option<PathBuf>,
}

impl App {
    /// Sets up the theme, the render worker and the camera, then opens `path` if given.
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let ctx = cc.egui_ctx.clone();
        theme::apply(&ctx);
        let waker = ctx.clone();
        let mut renderer = Renderer::default();
        let worker = Worker::spawn(
            "vcb-render",
            move |job: Job| renderer.render(&job),
            move || waker.request_repaint(),
        );
        let camera_dir = presets::camera_dir();
        let mut app = Self {
            cameras: Vec::new(),
            presets: presets::discover(Path::new(presets::PRESETS_DIR)).unwrap_or_default(),
            camera: None,
            preset: Preset::new(""),
            current: Current::unsaved("untitled", &Preset::new("")),
            bus: BusPanel::default(),
            timing: None,
            loaded: None,
            shown: None,
            texture: None,
            worker: None,
            generations: Generations::default(),
            latest_full: false,
            settle: None,
            export: None,
            opening: None,
            writing: None,
            view: View::default(),
            view_rect: egui::Rect::NOTHING,
            message: Message::default(),
            render_error: None,
            screenshot: std::env::var_os(SCREENSHOT_ENV).map(PathBuf::from),
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

    /// Discovers the cameras and starts with the first one and a clean preset.
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
                self.bus = BusPanel::from_preset(&preset);
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

    /// Takes in finished file operations and render replies.
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
    }

    /// Makes a freshly opened source the working scene.
    fn install(&mut self, ctx: &egui::Context, mut source: Source) {
        let image = std::mem::take(&mut source.original);
        let original = Texture::upload(ctx, "original", image);
        ctx.send_viewport_cmd(ViewportCommand::Title(format!(
            "{} — Virtual Camera Bender",
            source.name()
        )));
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
    fn request(&mut self, preview: bool) {
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

    /// The frame on screen if it is the full render of the current preset.
    fn full_on_screen(&self) -> Option<&Shown> {
        self.shown.as_ref().filter(|shown| {
            !shown.preview && self.generations.is_latest(shown.generation) && self.settle.is_none()
        })
    }

    /// Exports the full render to `path`, rendering it first if only a preview is shown.
    fn export_to(&mut self, ctx: &egui::Context, path: PathBuf) {
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
        self.bus.load(&preset);
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
                if let Ok(list) = presets::discover(Path::new(presets::PRESETS_DIR)) {
                    self.presets = list;
                }
            }
            Err(error) => self.message = Message::error(format!("{error:#}")),
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
                .on_hover_text("Open an image or a video")
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
                    Some(path) => format!("Save to {}", path.display()),
                    None => "Save to a new file".to_owned(),
                });
            if save.clicked() {
                self.save_preset(false);
            }
            if ui.button("Save as…").clicked() {
                self.save_preset(true);
            }
            let can_export = self.loaded.is_some() && self.camera.is_some();
            let export = ui.add_enabled(can_export, egui::Button::new("Export image…"));
            if export.clicked() {
                let name = format!(
                    "{}-{}.png",
                    self.loaded
                        .as_ref()
                        .map_or_else(|| "render".to_owned(), |loaded| stem(&loaded.source.path)),
                    self.current.name
                );
                if let Some(path) = io::pick_export(&name) {
                    self.export_to(&ctx, path);
                }
            }
            ui.separator();
            self.pickers(ui);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let content = self.content_size().unwrap_or(Vec2::ZERO);
                if ui
                    .button("1:1")
                    .on_hover_text("One image pixel per screen pixel")
                    .clicked()
                {
                    self.view
                        .actual_pixels(content, ui.ctx().pixels_per_point());
                }
                if ui.selectable_label(self.view.is_fit(), "Fit").clicked() {
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

    fn status_line(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let weak = |text: String| RichText::new(text).color(theme::TEXT_WEAK);
            match &self.loaded {
                Some(loaded) => {
                    let scene = &loaded.source.scene;
                    ui.label(loaded.source.name());
                    ui.label(weak(format!(
                        "{} × {} px, {}",
                        scene.width(),
                        scene.height(),
                        file_size(loaded.source.bytes)
                    )));
                }
                None => {
                    ui.label(weak("No image".to_owned()));
                }
            }
            if let Some(shown) = &self.shown {
                ui.separator();
                ui.label(weak(format!(
                    "sensor {} × {}",
                    shown.sensor[0], shown.sensor[1]
                )));
                ui.separator();
                let kind = if shown.preview { "preview" } else { "full" };
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
            if self.generations.is_pending() || self.opening.is_some() || self.writing.is_some() {
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
            });
        });
    }

    fn inspector(&mut self, ui: &mut Ui) -> Edit {
        let mut edit = Edit::default();
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(theme::GRID);
                edit |= panels::readout::show(ui, &mut self.preset, self.timing.as_ref());
                ui.add_space(theme::GRID);
                ui.separator();
                ui.add_space(theme::GRID);
                edit |= self.bus.show(ui);
                ui.add_space(theme::GRID);
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
                ui.add_space((rect.height() / 2.0 - theme::GRID * 6.0).max(0.0));
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
                ui.add_space(theme::GRID);
                ui.label(RichText::new("or drop a file on the window").color(theme::TEXT_WEAK));
            });
        });
        drop_overlay(ui, rect);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        // Everything below may change the preset: file pickers, panels, camera choice.
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
        let show_original = !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| {
                i.consume_key(Modifiers::NONE, Key::Space);
                i.key_down(Key::Space)
            });

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

        let edit = Panel::right("inspector")
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

        if edit.changed {
            self.bus.apply(&mut self.preset);
        }
        if self.preset != before {
            self.refresh_timing();
            self.request(true);
        }
        if edit.released && self.settle.is_some() {
            self.request(false);
        }
        self.settle(&ctx, edit.dragging);
    }
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
fn save_screenshot(ctx: &egui::Context, path: PathBuf) {
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
