//! The desktop app of the virtual camera bender.
//!
//! A window in which a photo, or the first frame of a video, sits in front of the
//! simulated camera: the user picks a camera and a preset, sets the readout and the
//! bus bend on a front panel drawn after the schematic, and sees the camera's output
//! as the controls move. Renders run on a worker thread ([`render`]): a reduced
//! preview while a control is dragged, the full preset when it is released. The
//! binary `vcb-app` calls [`run`].
//!
//! The modules are public so the pure parts (preset building, job coalescing, view
//! geometry, camera discovery) can be tested from `tests/`; the app has no library
//! users.

#![expect(
    clippy::multiple_crate_versions,
    reason = "eframe, wgpu and winit pull several versions of platform crates that this crate cannot unify"
)]

pub mod app;
pub mod io;
pub mod panels;
pub mod presets;
pub mod render;
pub mod theme;
pub mod viewport;

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use eframe::egui;

/// Title of the window and name of the app.
pub const TITLE: &str = "Virtual Camera Bender";

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
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, path)))),
    )
    .map_err(|error| anyhow!("cannot run the window: {error}"))
}
