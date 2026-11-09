//! The desktop app of the virtual camera bender.
//!
//! A window in which a photo or a video sits in front of the simulated camera: the
//! user picks a camera and a preset, sets every stage of the signal chain on panels in
//! physical units (scene and optics, sensor, readout and clocks, analog front end, the
//! bus bend drawn after its schematic, the master clock, the processor, the frame
//! memory's address pins, the supply rail), and sees the camera's output as the
//! controls move. Renders run on a worker thread ([`render`]) whose simulators keep
//! running between renders: a reduced preview while a control is dragged, the full
//! preset when it is released. Videos are read frame by frame on demand, played
//! through the preview simulator ([`video`]) and exported through a fresh full-size
//! simulator on a background thread ([`export`]). The [`gallery`] shows every preset
//! for the camera rendered on the loaded image, the worker rendering its thumbnails
//! behind the previews. The binary `vcb-app` calls [`run`].
//!
//! The modules are public so the pure parts (preset building, job coalescing, the
//! frame cache, the export job, shortcuts, view geometry, camera discovery, the
//! gallery's cache) can be tested from `tests/`; the app has no library users.

#![expect(
    clippy::multiple_crate_versions,
    reason = "eframe, wgpu and winit pull several versions of platform crates that this crate cannot unify"
)]

pub mod app;
pub mod export;
pub mod gallery;
pub mod icon;
pub mod io;
pub mod panels;
pub mod presets;
pub mod render;
pub mod shortcuts;
pub mod theme;
pub mod video;
pub mod viewport;

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use eframe::egui;

/// Title of the window and name of the app.
pub const TITLE: &str = "Virtual Camera Bender";

/// Edge of the window icon in pixels.
const ICON_SIZE: u32 = 256;

/// Opens the app's window and runs it until it is closed.
///
/// `path`, if given, is opened as the first scene.
///
/// # Errors
///
/// Fails if the window or its graphics context cannot be created.
pub fn run(path: Option<PathBuf>) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_drag_and_drop(true)
            .with_icon(icon::icon_data(ICON_SIZE)),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, path)))),
    )
    .map_err(|error| anyhow!("cannot run the window: {error}"))
}
