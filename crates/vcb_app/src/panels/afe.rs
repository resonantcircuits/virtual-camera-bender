//! The analog front end bend: an injected oscillator, gain modulation and CDS skew.
//!
//! The panel sets the preset's `[bend.afe]` section. The injected oscillator adds its
//! waveform at the AFE input, before the programmable gain, at a frequency in hertz
//! and an amplitude in volts; next to the frequency the panel shows how many lines of
//! the current readout one cycle spans, which sets the spacing of the bands. A second
//! oscillator modulates the gain by a relative depth. The correlated double sampler's
//! reset sample can be taken late by a number of pixel periods, subtracting a fraction
//! of an earlier level, which echoes every edge. [`AfePanel`] keeps every value while
//! a part, or the whole bend, is switched off.

use eframe::egui::Ui;
use vcb_core::afe::{GainMod, Inject, Wave};
use vcb_core::preset::AfeBend;
use vcb_core::{Preset, Timing};

use crate::panels::{self, Edit, Knob, Scale};

/// The oscillator frequency knobs: 1 Hz to 1 MHz, logarithmic.
pub const FREQUENCY: Knob = Knob::log(1.0..=1e6, 1.0, " Hz");

/// The injection amplitude knob: up to 1 V at the AFE input, logarithmic, with zero.
pub const AMPLITUDE: Knob = Knob::log(0.0..=1.0, 0.0005, " V");

/// The phase-skew knob: half a cycle either way per line.
pub const SKEW: Knob = Knob::linear(-0.5..=0.5, " cycles/line", 3);

/// The gain-modulation depth knob: 0 to 100 % of the gain.
pub const DEPTH: Knob = Knob::linear(0.0..=100.0, " %", 0).scaled(Scale::Times(100.0));

/// The CDS lag knob: up to 32 pixel periods.
pub const LAG: Knob = Knob::linear(0.0..=32.0, " px", 1);

/// The CDS mix knob: 0 to 100 % of the delayed level.
pub const MIX: Knob = Knob::linear(0.0..=100.0, " %", 0).scaled(Scale::Times(100.0));

/// The waveform choices, in panel order.
const WAVES: [(Wave, &str); 4] = [
    (Wave::Sine, "Sine"),
    (Wave::Square, "Square"),
    (Wave::Saw, "Saw"),
    (Wave::Noise, "S&H noise"),
];

/// The AFE bend's switches and values.
#[derive(Debug, Clone, PartialEq)]
pub struct AfePanel {
    /// Whether the bend is patched in.
    pub enabled: bool,
    /// Whether the injected oscillator is connected.
    pub inject_on: bool,
    /// The injected oscillator, kept while disconnected.
    pub inject: Inject,
    /// Whether the gain modulation is connected.
    pub gain_on: bool,
    /// The gain-modulating oscillator, kept while disconnected.
    pub gain_mod: GainMod,
    /// Delay of the CDS reset sample in pixel periods.
    pub cds_lag_px: f64,
    /// Fraction of the delayed level the late reset sample subtracts.
    pub cds_mix: f64,
}

impl Default for AfePanel {
    /// A 1.2 kHz square wave of 50 mV injected, no gain modulation, switched off.
    fn default() -> Self {
        Self {
            enabled: false,
            inject_on: true,
            inject: Inject {
                wave: Wave::Square,
                freq_hz: 1200.0,
                amplitude_v: 0.05,
                phase_skew: 0.0,
            },
            gain_on: false,
            gain_mod: GainMod {
                wave: Wave::Sine,
                freq_hz: 60.0,
                depth: 0.2,
            },
            cds_lag_px: 0.0,
            cds_mix: 0.0,
        }
    }
}

impl AfePanel {
    /// The panel showing `preset`'s AFE bend, or the default values switched off.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the panel from `preset`; parts the preset lacks keep their values, disconnected.
    pub fn load(&mut self, preset: &Preset) {
        let Some(afe) = &preset.bend.afe else {
            self.enabled = false;
            return;
        };
        self.enabled = true;
        self.inject_on = afe.inject.is_some();
        if let Some(inject) = &afe.inject {
            self.inject = inject.clone();
        }
        self.gain_on = afe.gain_mod.is_some();
        if let Some(gain_mod) = &afe.gain_mod {
            self.gain_mod = gain_mod.clone();
        }
        self.cds_lag_px = afe.cds_lag_px;
        self.cds_mix = afe.cds_mix;
    }

    /// The preset's `[bend.afe]` section; `None` while the bend is off.
    #[must_use]
    pub fn bend(&self) -> Option<AfeBend> {
        self.enabled.then(|| AfeBend {
            inject: self.inject_on.then(|| self.inject.clone()),
            gain_mod: self.gain_on.then(|| self.gain_mod.clone()),
            cds_lag_px: self.cds_lag_px,
            cds_mix: self.cds_mix,
        })
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.afe = self.bend();
    }

    /// Draws the panel; `timing` is the readout of the current scene, if known.
    pub fn show(&mut self, ui: &mut Ui, timing: Option<&Timing>) -> Edit {
        let mut edit = Edit::default();
        if let Some(response) = panels::header(
            ui,
            "Analog front end",
            Some((
                &mut self.enabled,
                "Patch the bend into the analog front end",
            )),
        ) {
            edit.record(&response);
        }
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            edit |= self.show_inject(ui, timing);
            edit |= self.show_gain(ui, timing);
            edit |= self.show_cds(ui);
        });
        edit
    }

    fn show_inject(&mut self, ui: &mut Ui, timing: Option<&Timing>) -> Edit {
        let mut edit = Edit::default();
        part_header(
            ui,
            &mut edit,
            "Injected oscillator",
            &mut self.inject_on,
            "Couple an oscillator into the signal before the gain",
        );
        let inject = &mut self.inject;
        ui.scope(|ui| {
            if !self.inject_on {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            panels::grid(ui, "afe-inject", |ui| {
                wave_row(ui, &mut edit, &mut inject.wave);
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Frequency",
                    "Oscillator frequency; it runs on through blanking, so bands roll from frame to frame",
                    &mut inject.freq_hz,
                    &FREQUENCY,
                );
                if let Some(timing) = timing {
                    panels::hint_row(ui, &cycle_length(inject.freq_hz, timing));
                }
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Amplitude",
                    "Peak voltage at the AFE input; the gain amplifies it, the ADC's full scale is 1 V",
                    &mut inject.amplitude_v,
                    &AMPLITUDE,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Phase skew",
                    "Extra phase step per line in cycles, which tilts the bands",
                    &mut inject.phase_skew,
                    &SKEW,
                );
            });
        });
        edit
    }

    fn show_gain(&mut self, ui: &mut Ui, timing: Option<&Timing>) -> Edit {
        let mut edit = Edit::default();
        part_header(
            ui,
            &mut edit,
            "Gain modulation",
            &mut self.gain_on,
            "Let an oscillator swing the programmable gain",
        );
        let gain_mod = &mut self.gain_mod;
        ui.scope(|ui| {
            if !self.gain_on {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            panels::grid(ui, "afe-gain", |ui| {
                wave_row(ui, &mut edit, &mut gain_mod.wave);
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Frequency",
                    "Frequency of the gain-modulating oscillator",
                    &mut gain_mod.freq_hz,
                    &FREQUENCY,
                );
                if let Some(timing) = timing {
                    panels::hint_row(ui, &cycle_length(gain_mod.freq_hz, timing));
                }
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Depth",
                    "The gain swings within 1 ± depth times its setting",
                    &mut gain_mod.depth,
                    &DEPTH,
                );
            });
        });
        edit
    }

    fn show_cds(&mut self, ui: &mut Ui) -> Edit {
        let mut edit = Edit::default();
        panels::section(ui, "CDS clamp timing");
        panels::grid(ui, "afe-cds", |ui| {
            panels::knob_row(
                ui,
                &mut edit,
                "Reset lag",
                "Delay of the reset sample in pixel periods; 0 is correct timing",
                &mut self.cds_lag_px,
                &LAG,
            );
            panels::knob_row(
                ui,
                &mut edit,
                "Echo",
                "Fraction of the delayed level the late reset sample subtracts: a negative echo of each edge",
                &mut self.cds_mix,
                &MIX,
            );
        });
        edit
    }
}

/// A part's title with its own on-off switch.
fn part_header(ui: &mut Ui, edit: &mut Edit, text: &str, on: &mut bool, hover: &str) {
    ui.add_space(panels::SECTION_GAP);
    ui.horizontal(|ui| {
        panels::section_label(ui, text);
        edit.record(&panels::power(ui, on, hover));
    });
}

/// A grid row choosing an oscillator's waveform.
fn wave_row(ui: &mut Ui, edit: &mut Edit, wave: &mut Wave) {
    panels::row_label(ui, "Waveform");
    edit.record(&panels::choice(ui, wave, &WAVES));
    ui.label("");
    ui.end_row();
}

/// How long one cycle at `freq_hz` lasts in the readout: in lines, or in pixels below a line.
#[must_use]
pub fn cycle_length(freq_hz: f64, timing: &Timing) -> String {
    if freq_hz <= 0.0 {
        return "Stopped".to_owned();
    }
    let lines = timing.lines_per_cycle(freq_hz);
    if lines >= 1.0 {
        format!("{} lines per cycle", panels::significant(lines, 3))
    } else {
        format!(
            "{} px per cycle",
            panels::significant(timing.samples_per_cycle(freq_hz), 3)
        )
    }
}
