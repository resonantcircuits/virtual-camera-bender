//! The readout panel: readout mode, sensor resolution, long edge, orientation, seed and power.
//!
//! Below the controls the panel shows what the readout derives for the current scene:
//! the sensor size and rows read, the output frame (the movie readout's frame is
//! smaller than the sensor), the pixel clock, line and frame period. "Reset circuit"
//! powers the camera off and on: the oscillators, capacitor charge, servos and clock
//! faults that otherwise carry from one render to the next start afresh.
//!
//! The long edge limits a match-input sensor to a chosen size, so a full render shows
//! the bends at the scale of the previews; the knob is dimmed at native resolution.
//! [`ReadoutPanel`] keeps the knob's setting while the resolution is native, which
//! the preset cannot store.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

use eframe::egui::{DragValue, Ui};
use vcb_core::preset::{Mode, Orientation, Readout, Resolution};
use vcb_core::{Preset, Timing};

use crate::panels::{self, Edit, Knob};
use crate::shortcuts::Action;

/// Largest seed the "new seed" button draws; small seeds are easy to note down.
const MAX_NEW_SEED: u64 = 99_999;

/// The long-edge knob: 160 to 4096 px, logarithmic, its far left "off".
///
/// From about half a gallery thumbnail to the long side of a 4K frame, past the
/// camera's own 2272; off keeps the input's size. A typed value below 160 px is
/// taken, down to the core's 16 px.
pub const LONG_EDGE: Knob = Knob::log(0.0..=4096.0, 160.0, " px")
    .decimals(0)
    .zero_reads("off");

/// What the long edge is for, on the knob and its label.
const LONG_EDGE_HOVER: &str = "Limits the match-input sensor to this many pixels on its long side, the input's aspect kept. The bends are sized in sensor pixels and lines, so a small virtual sensor shows them large, as the previews do; off keeps the input's size. The camera's own 4 MP sensor is the Native resolution";

/// The readout panel's state that the preset cannot hold: the long-edge knob.
///
/// The default is off.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReadoutPanel {
    /// The long edge in pixels, 0 for off; kept while the resolution is native.
    pub long_edge_px: f64,
}

impl ReadoutPanel {
    /// The panel showing `preset`'s long edge, or off.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the knob from `preset`: its long edge, or off.
    pub fn load(&mut self, preset: &Preset) {
        self.long_edge_px = preset.readout.long_edge.map_or(0.0, f64::from);
    }

    /// The preset's `[readout] long_edge` for `resolution`: the knob's, at match input only.
    #[must_use]
    pub fn long_edge(&self, resolution: Resolution) -> Option<u32> {
        (resolution == Resolution::MatchInput && self.long_edge_px > 0.0)
            .then(|| long_edge_px(self.long_edge_px))
    }

    /// Writes the knob into `preset`, for its resolution.
    pub fn apply(&self, preset: &mut Preset) {
        preset.readout.long_edge = self.long_edge(preset.readout.resolution);
    }

    /// Draws the readout controls for `preset`.
    ///
    /// `timing` is the derived readout, if valid; `running` the number of frames the
    /// full render's simulator has run since power-on, if it has run.
    pub fn show(
        &mut self,
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
                    "Native: the camera's own sensor. Match input: a sensor the size of the input, or of the long edge",
                ),
            );
            ui.label("");
            ui.end_row();

            self.long_edge_row(ui, &mut edit, preset.readout.resolution);

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
                    .on_hover_text(
                        "Seed of every noise source; the same seed renders the same bytes",
                    ),
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

    /// The long-edge row: the knob, enabled at match input and dimmed at native.
    fn long_edge_row(&mut self, ui: &mut Ui, edit: &mut Edit, resolution: Resolution) {
        let enabled = resolution == Resolution::MatchInput;
        panels::row_label(ui, "Long edge").on_hover_text(LONG_EDGE_HOVER);
        let mut shown = self.long_edge_px;
        let response = ui
            .add_enabled_ui(enabled, |ui| panels::knob(ui, &mut shown, &LONG_EDGE))
            .inner
            .on_hover_text(LONG_EDGE_HOVER)
            .on_disabled_hover_text(
                "A limit for the match-input sensor; the Native resolution is the camera's own 4 MP sensor",
            );
        if response.changed() {
            self.long_edge_px = if shown > 0.0 {
                f64::from(long_edge_px(shown))
            } else {
                0.0
            };
        }
        edit.record(&response);
        ui.label("");
        ui.end_row();
    }
}

/// The long edge in whole pixels within the core's range, for a knob value above zero.
fn long_edge_px(shown: f64) -> u32 {
    let low = f64::from(Readout::MIN_LONG_EDGE);
    let high = f64::from(Readout::MAX_LONG_EDGE);
    shown.round().clamp(low, high) as u32
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
        panels::hint(ui, &sensor_readout(timing));
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

/// The derived sensor and output size, and the long edge that limits them, if any.
#[must_use]
pub fn sensor_readout(timing: &Timing) -> String {
    let limit = timing
        .long_edge
        .map(|edge| format!(" (long edge at most {edge} px)"))
        .unwrap_or_default();
    format!(
        "Sensor {} × {} photosites{limit}, {} rows read; output {} × {} px",
        timing.sensor_width,
        timing.sensor_height,
        timing.rows_read,
        timing.output_width,
        timing.output_height,
    )
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
