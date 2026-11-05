//! The sensor panel: the still exposure time and whether the readout smears.
//!
//! The exposure time is a setting of the camera, not of the preset: preset files have
//! no exposure field. The app keeps it beside the preset as an adjustment of its copy
//! of the camera ([`adjusted`]); it is not saved with the preset and applies to stills
//! only, since the movie readout integrates for one frame period. In the model,
//! reference white fills a fixed fraction of the well whatever the exposure, and
//! auto-exposure sets the brightness, so the integration time shows as dark charge,
//! glowing hot photosites and, through the readout, the optical-black level.
//!
//! The smear flag belongs to the readout mode's shutter and is shown read-only: the
//! mechanical shutter of the still readout closes during the transfer, the electronic
//! shutter of the movie readout leaves the array lit.

use eframe::egui::Ui;
use vcb_core::Camera;
use vcb_core::preset::Mode;

use crate::panels::{self, Edit, Knob, Scale};

/// The exposure knob: 0.1 ms to 1 s, logarithmic, stored in seconds.
pub const EXPOSURE: Knob = Knob::log(0.1..=1000.0, 0.1, " ms").scaled(Scale::Times(1e3));

/// Returns `camera` with the still exposure time `exposure_s`, if one is set.
#[must_use]
pub fn adjusted(camera: &Camera, exposure_s: Option<f64>) -> Camera {
    let mut camera = camera.clone();
    if let Some(exposure_s) = exposure_s {
        camera.still.exposure_s = exposure_s;
    }
    camera
}

/// The integration time of a readout mode in seconds, with the adjustment applied.
#[must_use]
pub fn integration_s(camera: &Camera, mode: Mode, exposure_s: Option<f64>) -> f64 {
    match mode {
        Mode::Still => exposure_s.unwrap_or(camera.still.exposure_s),
        Mode::Movie => 1.0 / camera.movie.fps,
    }
}

/// Draws the sensor controls; `camera` is the camera as loaded, without the adjustment.
pub fn show(ui: &mut Ui, exposure_s: &mut Option<f64>, camera: &Camera, mode: Mode) -> Edit {
    let mut edit = Edit::default();
    panels::header(ui, "Sensor", None);
    panels::grid(ui, "sensor", |ui| {
        match mode {
            Mode::Still => panels::override_row(
                ui,
                &mut edit,
                "Exposure",
                "Integration time of the still: the dark charge and hot photosites grow with it",
                exposure_s,
                camera.still.exposure_s,
                &EXPOSURE,
            ),
            Mode::Movie => panels::value_row(
                ui,
                "Exposure",
                &format!(
                    "{:.1} ms, one frame at {} fps",
                    1e3 / camera.movie.fps,
                    camera.movie.fps
                ),
                "The movie readout integrates for one frame period",
            ),
        }
        let (smear, shutter) = match mode {
            Mode::Still => (camera.still.smear, "mechanical shutter"),
            Mode::Movie => (camera.movie.smear, "electronic shutter"),
        };
        panels::value_row(
            ui,
            "Smear",
            &format!("{}, {shutter}", if smear { "on" } else { "off" }),
            "Whether light reaches the vertical register during the transfer; set by the readout mode",
        );
    });
    if mode == Mode::Still {
        panels::hint(
            ui,
            "A camera setting, not stored in presets. Brightness follows reference white and auto-exposure.",
        );
    }
    edit
}
