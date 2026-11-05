//! The readout panel: readout mode, sensor resolution, orientation, seed and power.
//!
//! Below the controls the panel shows what the readout derives for the current scene:
//! the sensor size and rows read, the output frame (the movie readout's frame is
//! smaller than the sensor), the pixel clock, line and frame period. "Reset circuit"
//! powers the camera off and on: the oscillators, capacitor charge, servos and clock
//! faults that otherwise carry from one render to the next start afresh.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

use eframe::egui::{DragValue, Ui};
use vcb_core::preset::{Mode, Orientation, Resolution};
use vcb_core::{Preset, Timing};

use crate::panels::{self, Edit};
use crate::shortcuts::Action;

/// Largest seed the "new seed" button draws; small seeds are easy to note down.
const MAX_NEW_SEED: u64 = 99_999;

/// Draws the readout controls for `preset`.
///
/// `timing` is the derived readout, if valid; `running` the number of frames the full
/// render's simulator has run since power-on, if it has run.
pub fn show(
    ui: &mut Ui,
    preset: &mut Preset,
    timing: Option<&Timing>,
    running: Option<u64>,
) -> Edit {
    let mut edit = Edit::default();
    panels::header(ui, "Readout", None);
    panels::grid(ui, "readout", |ui| {
        panels::row_label(ui, "Mode");
        edit.record(
            &panels::choice(
                ui,
                &mut preset.readout.mode,
                &[(Mode::Still, "Still"), (Mode::Movie, "Movie")],
            )
            .on_hover_text(
                "Still: every row behind the mechanical shutter. Movie: line-skipped live view",
            ),
        );
        ui.label("");
        ui.end_row();

        panels::row_label(ui, "Resolution");
        edit.record(
            &panels::choice(
                ui,
                &mut preset.readout.resolution,
                &[
                    (Resolution::Native, "Native"),
                    (Resolution::MatchInput, "Match input"),
                ],
            )
            .on_hover_text(
                "Native: the camera's own sensor. Match input: a sensor the size of the input",
            ),
        );
        ui.label("");
        ui.end_row();

        panels::row_label(ui, "Orientation");
        edit.record(&panels::choice(
            ui,
            &mut preset.readout.orientation,
            &[
                (Orientation::Auto, "Auto"),
                (Orientation::Landscape, "Landscape"),
                (Orientation::Portrait, "Portrait"),
            ],
        ));
        ui.label("");
        ui.end_row();

        panels::row_label(ui, "Seed");
        ui.horizontal(|ui| {
            edit.record(
                &ui.add(
                    DragValue::new(&mut preset.seed)
                        .range(0..=u64::from(u32::MAX))
                        .clamp_existing_to_range(false)
                        .speed(0.2),
                )
                .on_hover_text("Seed of every noise source; the same seed renders the same bytes"),
            );
            let button = ui
                .button("New seed")
                .on_hover_text(format!("Draw another seed ({})", Action::NewSeed.label()));
            if button.clicked() {
                preset.seed = new_seed(preset.seed);
                edit.changed = true;
            }
        });
        ui.label("");
        ui.end_row();

        panels::row_label(ui, "Power");
        let reset = ui.button("Reset circuit").on_hover_text(
            "Power-cycle the camera: oscillator phases, capacitor charge, servos and clock faults start afresh",
        );
        edit.power_cycle |= reset.clicked();
        ui.label("");
        ui.end_row();
    });
    derived(ui, timing, running);
    edit
}

/// The hints under the controls: power-on age and the derived readout.
fn derived(ui: &mut Ui, timing: Option<&Timing>, running: Option<u64>) {
    if let Some(running) = running {
        panels::hint(
            ui,
            &format!(
                "The camera has run {running} frame{} since power-on; each render is the next frame",
                if running == 1 { "" } else { "s" }
            ),
        );
    }
    if let Some(timing) = timing {
        panels::hint(
            ui,
            &format!(
                "Sensor {} × {} photosites, {} rows read; output {} × {} px",
                timing.sensor_width,
                timing.sensor_height,
                timing.rows_read,
                timing.output_width,
                timing.output_height,
            ),
        );
        panels::hint(
            ui,
            &format!(
                "Pixel clock {:.2} MHz, line {:.1} µs, frame {:.1} ms",
                timing.pixel_clock_hz / 1e6,
                timing.line_period_s * 1e6,
                timing.frame_period_s * 1e3
            ),
        );
    }
}

/// Draws a seed between 1 and 99 999 that differs from `current`.
#[must_use]
pub fn new_seed(current: u64) -> u64 {
    // The standard library's per-process random hasher keys are the entropy source.
    let random = RandomState::new().hash_one(current);
    let seed = random % MAX_NEW_SEED + 1;
    if seed == current {
        seed % MAX_NEW_SEED + 1
    } else {
        seed
    }
}
