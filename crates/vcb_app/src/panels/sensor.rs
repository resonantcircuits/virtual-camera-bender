//! The sensor panel: the exposure time and whether the readout smears.
//!
//! The exposure time is the preset's `[sensor] exposure_s`. It starts dimmed at the
//! camera's own value; moving the knob writes the override and its reset button
//! removes it again (and the section with it). The collected light scales with the
//! exposure over the camera's nominal exposure, so a longer time brightens the frame
//! until the well or the converter clips, unless auto-exposure compensates within its
//! two stops; dark charge and hot photosites grow with it too. A new exposure reaches
//! the running camera without a power cycle.
//!
//! The knob sets the still exposure. In movie mode the panel shows, read-only, what the
//! electronic shutter integrates: the preset's or the camera's movie exposure, at most
//! one frame period. The smear flag belongs to the readout mode's shutter and is shown
//! read-only too: the mechanical shutter of the still readout closes during the
//! transfer, the electronic shutter of the movie readout leaves the array lit.

use eframe::egui::Ui;
use vcb_core::preset::{Mode, SensorOverride};
use vcb_core::{Camera, Preset};

use crate::panels::{self, Edit, Knob, Scale};

/// The exposure knob: 0.1 ms to 1 s, logarithmic, stored in seconds.
pub const EXPOSURE: Knob = Knob::log(0.1..=1000.0, 0.1, " ms").scaled(Scale::Times(1e3));

/// The preset's exposure override, if it sets one.
#[must_use]
pub fn exposure_override(preset: &Preset) -> Option<f64> {
    preset.sensor.and_then(|sensor| sensor.exposure_s)
}

/// Sets the preset's exposure override, or removes it with `None`.
///
/// Removing the last value of the `[sensor]` section removes the section.
pub fn set_exposure(sensor: &mut Option<SensorOverride>, exposure_s: Option<f64>) {
    if exposure_s.is_some() {
        sensor
            .get_or_insert_with(SensorOverride::default)
            .exposure_s = exposure_s;
    } else if let Some(section) = sensor.as_mut() {
        section.exposure_s = None;
        if *section == SensorOverride::default() {
            *sensor = None;
        }
    }
}

/// The time the sensor integrates in `mode`, as the simulator resolves it, in seconds.
///
/// Still: the preset's exposure, else the camera's still exposure. Movie: the preset's
/// exposure, else the camera's movie exposure, at most one frame period.
#[must_use]
pub fn integration_s(camera: &Camera, preset: &Preset, mode: Mode) -> f64 {
    let set = exposure_override(preset);
    match mode {
        Mode::Still => set.unwrap_or(camera.still.exposure_s),
        Mode::Movie => set
            .unwrap_or(camera.movie.exposure_s)
            .min(1.0 / camera.movie.fps),
    }
}

/// Draws the sensor controls for `preset` on `camera`.
pub fn show(ui: &mut Ui, preset: &mut Preset, camera: &Camera) -> Edit {
    let mut edit = Edit::default();
    panels::header(ui, "Sensor", None);
    let mode = preset.readout.mode;
    panels::grid(ui, "sensor", |ui| {
        match mode {
            Mode::Still => {
                let mut exposure = exposure_override(preset);
                let before = exposure;
                panels::override_row(
                    ui,
                    &mut edit,
                    "Exposure",
                    "Integration time: the light collected, the dark charge and the hot photosites grow with it",
                    &mut exposure,
                    camera.still.exposure_s,
                    &EXPOSURE,
                );
                if exposure != before {
                    set_exposure(&mut preset.sensor, exposure);
                }
            }
            Mode::Movie => {
                let frame_s = 1.0 / camera.movie.fps;
                let source = if exposure_override(preset).is_some() {
                    "from the preset"
                } else {
                    "the camera's"
                };
                panels::value_row(
                    ui,
                    "Exposure",
                    &format!(
                        "{:.2} ms, {source}",
                        integration_s(camera, preset, Mode::Movie) * 1e3
                    ),
                    &format!(
                        "The electronic shutter integrates at most one frame, {:.1} ms at {} fps; set the exposure in still mode",
                        frame_s * 1e3,
                        camera.movie.fps
                    ),
                );
            }
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
    let nominal = camera.sensor.nominal_exposure_s;
    let exposure = integration_s(camera, preset, mode);
    panels::hint(
        ui,
        &format!(
            "{:+.2} stops of light against the nominal {:.2} ms; auto-exposure may take back up to two.",
            (exposure / nominal).log2(),
            nominal * 1e3
        ),
    );
    edit
}
