//! The readout panel: readout mode, sensor resolution, orientation and the noise seed.
//!
//! Below the controls the panel shows the clocks the readout derives for the current
//! scene: sensor size, pixel clock, line and frame period.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

use eframe::egui::{DragValue, Grid, Ui};
use vcb_core::preset::{Mode, Orientation, Resolution};
use vcb_core::{Preset, Timing};

use crate::panels::{self, Edit};
use crate::theme;

/// Largest seed the "new seed" button draws; small seeds are easy to note down.
const MAX_NEW_SEED: u64 = 99_999;

/// Draws the readout controls for `preset`; `timing` is the derived readout, if valid.
pub fn show(ui: &mut Ui, preset: &mut Preset, timing: Option<&Timing>) -> Edit {
    let mut edit = Edit::default();
    panels::title(ui, "Readout");
    Grid::new("readout")
        .num_columns(2)
        .spacing([theme::GRID, theme::GRID])
        .show(ui, |ui| {
            ui.label("Mode");
            edit.record(&panels::choice(
                ui,
                &mut preset.readout.mode,
                &[(Mode::Still, "Still"), (Mode::Movie, "Movie")],
            ));
            ui.end_row();

            ui.label("Resolution");
            edit.record(&panels::choice(
                ui,
                &mut preset.readout.resolution,
                &[
                    (Resolution::Native, "Native"),
                    (Resolution::MatchInput, "Match input"),
                ],
            ));
            ui.end_row();

            ui.label("Orientation");
            edit.record(&panels::choice(
                ui,
                &mut preset.readout.orientation,
                &[
                    (Orientation::Auto, "Auto"),
                    (Orientation::Landscape, "Landscape"),
                    (Orientation::Portrait, "Portrait"),
                ],
            ));
            ui.end_row();

            ui.label("Seed");
            ui.horizontal(|ui| {
                edit.record(
                    &ui.add(
                        DragValue::new(&mut preset.seed)
                            .range(0..=u64::from(u32::MAX))
                            .clamp_existing_to_range(false)
                            .speed(0.2),
                    )
                    .on_hover_text(
                        "Seed of every noise source; the same seed renders the same bytes",
                    ),
                );
                let button = ui.button("New seed");
                if button.clicked() {
                    preset.seed = new_seed(preset.seed);
                    edit.changed = true;
                }
            });
            ui.end_row();
        });
    if let Some(timing) = timing {
        panels::hint(
            ui,
            &format!(
                "Sensor {} × {} photosites, {} rows read",
                timing.sensor_width, timing.sensor_height, timing.rows_read,
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
    edit
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
