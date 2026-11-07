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
//!
//! At the end of the panel the "Reset gate" block sets the preset's `[bend.reset]`
//! section with its own power switch, apart from the AFE bend's: missed reset pulses
//! at a mean rate in hertz, in runs of a mean length in native pixel periods, and the
//! reset's strength as the percentage of the floating diffusion's charge each pulse
//! clears, dimmed at the sound 100 %. A readout shows the output stage's swing in
//! electrons, `fd_swing_v / conv_gain`, and the packet above which a weak reset shows,
//! `reset_frac` times the swing. [`ResetPanel`] keeps the values while the pulses are
//! sound.
//!
//! Last, the "Serial link" block sets the preset's `[bend.serial]` section with its own
//! power switch, the SDATA contact sound or noisy: the probability that a bit of the
//! processor's register writes reads wrong at the AFE, shown in bits per thousand on a
//! logarithmic knob. The processor rewrites the AFE's seven modelled registers in every
//! vertical blanking, each write an 8-bit address and `[afe] serial_data_bits` of
//! data, and a readout shows those bits per frame with the expected corrupted writes
//! per frame, `7 (1 - (1 - p)^(8 + serial_data_bits))`, and the share of frames that
//! take at least one. [`SerialPanel`] keeps the value while the contact is sound.

use eframe::egui::Ui;
use vcb_core::afe::{GainMod, Inject, Wave};
use vcb_core::preset::{AfeBend, ResetBend, SerialBend};
use vcb_core::{Camera, Preset, Timing};

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

/// The missed-pulse knob: runs a second up to 100 kHz, logarithmic, with zero for none.
pub const DROPS: Knob = Knob::log(0.0..=100_000.0, 0.1, " Hz");

/// The run-length knob: 1 to 2000 native pixel periods, logarithmic.
pub const RUN: Knob = Knob::log(1.0..=2000.0, 1.0, " px");

/// The reset-strength knob: 0 to 100 % of the diffusion's charge cleared per pulse.
pub const STRENGTH: Knob = Knob::linear(0.0..=100.0, " %", 0).scaled(Scale::Times(100.0));

/// The missed-pulse rate of a fresh reset-gate bend, in runs a second.
pub const DEFAULT_DROP_HZ: f64 = 2000.0;

/// The bit-error knob: one bit in 100 000 to every bit, logarithmic, shown per thousand bits.
pub const BIT_ERROR: Knob =
    Knob::log(0.01..=1000.0, 0.01, " per 1000 bits").scaled(Scale::Times(1000.0));

/// The register writes the processor sends the AFE in every vertical blanking.
///
/// The modelled registers, rewritten as a set before each frame: the control word, the
/// VGA gain, the four pixel gains and the clamp level.
pub const SERIAL_WRITES: u32 = 7;

/// Bits of a serial write's register address, sent before its data field.
pub const SERIAL_ADDRESS_BITS: u32 = 8;

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

/// The reset-gate bend's switch and values, a block at the end of the AFE panel.
#[derive(Debug, Clone, PartialEq)]
pub struct ResetPanel {
    /// Whether the reset pulses are faulted: the section is written.
    pub enabled: bool,
    /// The values, kept while the pulses are sound.
    pub bend: ResetBend,
}

impl Default for ResetPanel {
    /// A flaky contact dropping 2000 runs of pulses a second, otherwise full resets; sound.
    fn default() -> Self {
        Self {
            enabled: false,
            bend: ResetBend {
                drop_hz: DEFAULT_DROP_HZ,
                ..ResetBend::default()
            },
        }
    }
}

impl ResetPanel {
    /// The panel showing `preset`'s reset bend, or the default values, sound.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the panel from `preset`; without a reset bend only the switch moves.
    pub fn load(&mut self, preset: &Preset) {
        match preset.bend.reset {
            Some(bend) => {
                self.enabled = true;
                self.bend = bend;
            }
            None => self.enabled = false,
        }
    }

    /// The preset's `[bend.reset]` section; `None` while the pulses are sound.
    #[must_use]
    pub fn bend(&self) -> Option<ResetBend> {
        self.enabled.then_some(self.bend)
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.reset = self.bend();
    }

    /// Draws the block for `camera`, whose output stage sets the swing.
    pub fn show(&mut self, ui: &mut Ui, camera: &Camera) -> Edit {
        let mut edit = Edit::default();
        edit.record(&panels::block_header(
            ui,
            "Reset gate",
            &mut self.enabled,
            "Reset pulses",
            ["sound", "faulted"],
        ));
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            let bend = &mut self.bend;
            panels::grid(ui, "afe-reset", |ui| {
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Missed pulses",
                    "Mean rate of runs of missed reset pulses, a flaky contact on the reset-gate wire: the floating diffusion keeps every packet of a run, so the CDS reads them as usual until their sum uses up the output stage's swing, then black until the next reset",
                    &mut bend.drop_hz,
                    &DROPS,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Run length",
                    "Mean length of a run of missed pulses, in pixel periods of the camera's own clock at every resolution",
                    &mut bend.hold_px,
                    &RUN,
                );
                panels::default_row(
                    ui,
                    &mut edit,
                    "Reset strength",
                    "Share of the diffusion's charge each reset pulse clears: what a weak pulse leaves behind builds up over bright runs until the swing is used up, so they dim and the brightest end in black; 100 % is a sound reset, shown dimmed",
                    &mut bend.reset_frac,
                    1.0,
                    &STRENGTH,
                );
            });
            panels::hint(ui, &reset_readout(camera, bend));
            panels::hint(ui, "On daylight scenes a weak reset takes about 20 to 25 % to see.");
        });
        edit
    }
}

/// The swing of the CCD's output stage in electrons, `fd_swing_v / conv_gain`.
///
/// Charge on the floating diffusion beyond it no longer moves the output.
#[must_use]
pub fn swing_e(camera: &Camera) -> f64 {
    camera.afe.fd_swing_v / (camera.sensor.conv_gain_uv_per_e * 1e-6)
}

/// The readout under the reset gate's knobs: the swing, and the packet a weak reset shows on.
///
/// A run of packets of at most `reset_frac` times the swing never reaches it under a
/// weak reset and reads unchanged; brighter runs dim.
#[must_use]
pub fn reset_readout(camera: &Camera, bend: &ResetBend) -> String {
    let swing = swing_e(camera);
    if bend.reset_frac >= 1.0 {
        format!("Swing {swing:.0} e; a full reset leaves no charge behind.")
    } else {
        format!(
            "Swing {swing:.0} e; a weak reset shows on packets above {:.0} e.",
            bend.reset_frac * swing
        )
    }
}

/// The serial link's switch and bit error, a block at the end of the AFE panel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SerialPanel {
    /// Whether the SDATA contact is noisy: the section is written.
    pub enabled: bool,
    /// The value, kept while the contact is sound.
    pub bend: SerialBend,
}

impl SerialPanel {
    /// The panel showing `preset`'s serial bend, or the default value, sound.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the panel from `preset`; without a serial bend only the switch moves.
    pub fn load(&mut self, preset: &Preset) {
        match preset.bend.serial {
            Some(bend) => {
                self.enabled = true;
                self.bend = bend;
            }
            None => self.enabled = false,
        }
    }

    /// The preset's `[bend.serial]` section; `None` while the contact is sound.
    #[must_use]
    pub fn bend(&self) -> Option<SerialBend> {
        self.enabled.then_some(self.bend)
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.serial = self.bend();
    }

    /// Draws the block for `camera`, whose `[afe]` section sets the length of a write.
    pub fn show(&mut self, ui: &mut Ui, camera: &Camera) -> Edit {
        let mut edit = Edit::default();
        edit.record(&panels::block_header(
            ui,
            "Serial link",
            &mut self.enabled,
            "SDATA contact",
            ["sound", "noisy"],
        ));
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            panels::grid(ui, "afe-serial", |ui| {
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Bit error",
                    "Bits that read wrong at the AFE, per thousand sent: a noisy contact on the data line of the serial link over which the processor rewrites the VGA gain, the four pixel gains, the black clamp and the power controls before every frame. A flipped data bit lands a wrong value for the frame; a flipped address bit lands it on another register, or loses it, and the register keeps the last frame's value",
                    &mut self.bend.bit_error,
                    &BIT_ERROR,
                );
            });
            panels::hint(ui, &serial_readout(camera, &self.bend));
        });
        edit
    }
}

/// Bits of one serial write: the register address and the data field, `8 + serial_data_bits`.
#[must_use]
pub fn write_bits(camera: &Camera) -> u32 {
    SERIAL_ADDRESS_BITS + u32::from(camera.afe.serial_data_bits)
}

/// Bits the processor sends the AFE in every vertical blanking: [`SERIAL_WRITES`] writes.
#[must_use]
pub fn serial_bits_per_frame(camera: &Camera) -> u32 {
    SERIAL_WRITES * write_bits(camera)
}

/// The expected writes per frame with at least one bit in error, `7 (1 - (1 - p)^(8 + n))`.
///
/// `p` is the bend's `bit_error` and `n` the camera's `serial_data_bits`.
#[must_use]
pub fn corrupted_writes(camera: &Camera, bend: &SerialBend) -> f64 {
    let clean = (1.0 - bend.bit_error).powf(f64::from(write_bits(camera)));
    f64::from(SERIAL_WRITES) * (1.0 - clean)
}

/// The share of frames whose writes take at least one bit in error, `1 - (1 - p)^bits`.
#[must_use]
pub fn corrupted_frames(camera: &Camera, bend: &SerialBend) -> f64 {
    1.0 - (1.0 - bend.bit_error).powf(f64::from(serial_bits_per_frame(camera)))
}

/// The readout under the bit-error knob: the bits sent a frame and how many writes they corrupt.
#[must_use]
pub fn serial_readout(camera: &Camera, bend: &SerialBend) -> String {
    let sent = format!(
        "{} bits a frame in {SERIAL_WRITES} writes of {SERIAL_ADDRESS_BITS} + {}",
        serial_bits_per_frame(camera),
        camera.afe.serial_data_bits
    );
    if bend.bit_error <= 0.0 {
        return format!("{sent}; none is corrupted.");
    }
    format!(
        "{sent}; on average {} writes corrupted a frame, at least one in {} % of the frames.",
        panels::significant(corrupted_writes(camera, bend), 3),
        panels::significant(corrupted_frames(camera, bend) * 100.0, 3),
    )
}
