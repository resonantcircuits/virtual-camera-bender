//! The clock bend: a lossy charge transfer and faults on the CCD's transfer clocks.
//!
//! The panel sets the preset's `[bend.clocks]` section. The charge-transfer
//! efficiency is shown as the charge lost per transfer in parts per million and
//! stored as `cte`; left at the camera's value it is not written. Vertical clock
//! faults (stalls and double pulses) and horizontal register glitches come at mean
//! rates per second of readout, with their length in native lines and their shift as
//! a percentage of the line. [`ClocksPanel`] keeps the values while the bend is off.

use eframe::egui::Ui;
use vcb_core::preset::ClockBend;
use vcb_core::{Camera, Preset, Timing};

use crate::panels::{self, Edit, Knob, Scale};

/// The transfer-loss knob: 0.1 to 100 000 parts per million, logarithmic, stored as `cte`.
///
/// The A520's CCD loses about 10 ppm; at 1000 ppm a packet that crosses the whole array
/// leaves about 1.7 lines of trail, at 100 000 ppm a melt of some 190 lines.
pub const LOSS: Knob = Knob::log(0.1..=100_000.0, 0.1, " ppm").scaled(Scale::LossPpm);

/// The fault-rate knobs: up to 1 kHz, logarithmic, with zero for none.
pub const RATE: Knob = Knob::log(0.0..=1000.0, 0.01, " Hz");

/// The stall-length knob: 1 to 500 native lines, logarithmic.
pub const STALL: Knob = Knob::log(1.0..=500.0, 1.0, " lines");

/// The glitch-shift knob: up to the whole line, in percent.
pub const SHIFT: Knob = Knob::linear(0.0..=100.0, " % of line", 0).scaled(Scale::Times(100.0));

/// The clock bend's switch and values.
#[derive(Debug, Clone, PartialEq)]
pub struct ClocksPanel {
    /// Whether the bend is patched in.
    pub enabled: bool,
    /// The values, kept while the bend is off.
    pub bend: ClockBend,
}

impl Default for ClocksPanel {
    /// A visibly melting transfer with occasional faults, switched off.
    fn default() -> Self {
        Self {
            enabled: false,
            bend: ClockBend {
                cte: Some(0.998),
                v_pulse_drop_hz: 2.0,
                h_glitch_hz: 1.0,
                ..ClockBend::default()
            },
        }
    }
}

impl ClocksPanel {
    /// The panel showing `preset`'s clock bend, or the default values switched off.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the panel from `preset`; without a clock bend only the switch moves.
    pub fn load(&mut self, preset: &Preset) {
        match &preset.bend.clocks {
            Some(bend) => {
                self.enabled = true;
                self.bend = bend.clone();
            }
            None => self.enabled = false,
        }
    }

    /// The preset's `[bend.clocks]` section; `None` while the bend is off.
    #[must_use]
    pub fn bend(&self) -> Option<ClockBend> {
        self.enabled.then(|| self.bend.clone())
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.clocks = self.bend();
    }

    /// Draws the panel; `timing` is the readout of the current scene, if known.
    pub fn show(&mut self, ui: &mut Ui, camera: &Camera, timing: Option<&Timing>) -> Edit {
        let mut edit = Edit::default();
        if let Some(response) = panels::header(
            ui,
            "Clocks",
            Some((&mut self.enabled, "Bend the CCD's transfer clocks")),
        ) {
            edit.record(&response);
        }
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            let bend = &mut self.bend;
            panels::grid(ui, "clocks", |ui| {
                panels::override_row(
                    ui,
                    &mut edit,
                    "Transfer loss",
                    "Charge left behind per vertical transfer (1 - CTE); trails grow along the readout",
                    &mut bend.cte,
                    camera.sensor.cte,
                    &LOSS,
                );
                let cte = bend.cte.unwrap_or(camera.sensor.cte);
                panels::hint_row(
                    ui,
                    &format!(
                        "Trail of {} lines from the far edge",
                        panels::significant(trail_lines(cte, camera.sensor.native_height), 2)
                    ),
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "V pulse drops",
                    "Mean rate of vertical clock faults: stalls repeat rows, double pulses skip them",
                    &mut bend.v_pulse_drop_hz,
                    &RATE,
                );
                if let Some(timing) = timing {
                    panels::hint_row(ui, &per_frame(bend.v_pulse_drop_hz, timing));
                }
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Stall length",
                    "Mean length of a vertical fault in lines of the full-size sensor",
                    &mut bend.v_stall_lines,
                    &STALL,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "H glitches",
                    "Mean rate of horizontal register glitches, each a band of shifted lines",
                    &mut bend.h_glitch_hz,
                    &RATE,
                );
                if let Some(timing) = timing {
                    panels::hint_row(ui, &per_frame(bend.h_glitch_hz, timing));
                }
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Glitch shift",
                    "Largest sideways shift of a glitch band, with wraparound",
                    &mut bend.h_shift_frac,
                    &SHIFT,
                );
            });
        });
        edit
    }
}

/// Mean trail, in lines, of a packet that crosses `transfers` transfers at efficiency `cte`.
#[must_use]
pub fn trail_lines(cte: f64, transfers: u32) -> f64 {
    f64::from(transfers) * (1.0 - cte) / cte.max(f64::EPSILON)
}

/// "About n per frame" for a fault rate at the readout's frame period.
fn per_frame(rate_hz: f64, timing: &Timing) -> String {
    format!(
        "About {} per frame",
        panels::significant(rate_hz * timing.frame_period_s, 2)
    )
}
