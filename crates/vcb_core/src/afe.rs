//! Analog front end: charge samples converted to the voltage at the ADC input.
//!
//! The model follows an AD9949-class CCD signal processor
//! (`docs/research/a520-technical.md`, section 5). It runs in continuous time on the
//! pixel clock, `oversample` sub-steps per pixel period, in `f64`, and carries its
//! analog state ([`State`]) from line to line and from frame to frame.
//!
//! # Signal chain
//!
//! 1. **CCD output and CDS.** Correlated double sampling turns each charge packet into
//!    a level of `conv_gain_uv_per_e` microvolts per electron above the reset level,
//!    held for the pixel period. With a clamp-timing skew (`[bend.afe] cds_lag_px`,
//!    `cds_mix`) the reset sample is taken `cds_lag_px` pixel periods late, so the
//!    fraction `cds_mix` of the level that many periods earlier is subtracted: every
//!    edge gets a negative echo further along the line. Before the line's first
//!    sample lies blanking, which contributes no signal.
//! 2. **Read noise**, added at the input. A white Gaussian part of `read_noise_e`
//!    electrons rms per sample, and a flicker part: a first-order low-pass Gaussian
//!    process with corner `noise_1f_corner_hz` whose spectral density at the corner
//!    equals that of the white floor, so the noise is streaky along lines. Both come
//!    from one [`Pcg32`] stream seeded with the stage seed and the frame index.
//! 3. **Injection** ([`Inject`]). An oscillator adds its waveform at the input node. It
//!    runs in real time over active samples and blanking; `phase_skew` adds a fixed
//!    phase step per line, which tilts the bands.
//! 4. **Programmable gain** of `pga_gain_db`, optionally modulated multiplicatively by
//!    a second oscillator ([`GainMod`]) with its own phase.
//! 5. **Bandwidth and slew.** The amplified sum drives a first-order low-pass of cutoff
//!    `bandwidth_hz` whose output moves at most `slew_v_per_us`. Each sub-step holds
//!    its input constant, so the exponential response is exact within it; the slew
//!    limit then clamps the step. The ADC samples the node at the end of each pixel
//!    period.
//! 6. **Pedestal and reference.** The black-level clamp adds `pedestal_v`, so zero
//!    charge lands on the nominal black code. Voltages are referred to the nominal
//!    ADC reference [`Params::vref_v`] by the factor `vref_v / rail_v`, where
//!    [`State::rail_v`] is the reference the supply delivers. The supply is ideal
//!    here, so the factor is one; a supply-sag model sets `rail_v` instead.
//!
//! Without noise, bends or bandwidth limits the chain reduces to
//! `volts = pedestal_v + electrons * conv_gain * 10^(pga_gain_db / 20)`.
//!
//! # Blanking
//!
//! During horizontal and vertical blanking the CCD delivers no charge. Over each
//! blanking interval the node relaxes in closed form towards the input at the
//! interval's end (the amplified injection and flicker voltages), which is exact up
//! to the residual `exp(-duration / tau)`; the flicker process takes its exact
//! transition over the interval, and both oscillator phases advance exactly with
//! time. Patterns therefore roll on from line to line and frame to frame without
//! restarting.
//!
//! # Time scaling
//!
//! Bandwidth and slew rate describe the camera at its native pixel clock. At
//! match-input resolution the pixel period differs, so both are scaled by
//! `pixel_period_s / native_pixel_period_s` before use, which keeps the edge softness
//! the same in pixels. The flicker corner and the oscillator frequencies describe
//! time scales of hundreds of pixels or more and are used as given: match-input
//! timing keeps the active-line time and the frame period native, so they span the
//! same fraction of the image at every resolution. For the same reason the flicker
//! level is pinned to the white floor at the native pixel period.

use std::f64::consts::TAU;
use std::fmt::{self, Debug, Formatter};

use crate::noise::{self, Pcg32};
use crate::readout::{Layout, Stream};
use crate::{Camera, Error, Preset, Timing};

/// Salt of the analog front end's noise sources.
const SALT: u32 = noise::salt("afe");

/// Salt of the injected sample-and-hold oscillator's levels.
const INJECT_SALT: u32 = noise::salt("afe.inject");

/// Salt of the gain-modulating sample-and-hold oscillator's levels.
const GAIN_SALT: u32 = noise::salt("afe.gain_mod");

/// Scale that maps a 32-bit integer onto `[0, 2)`.
const TWO_OVER_2_32: f64 = 2.0 / 4_294_967_296.0;

/// Waveform of an injected oscillator.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Wave {
    /// A sinusoid.
    Sine,
    /// A 50 % duty-cycle square wave, as from a 555 timer.
    #[default]
    Square,
    /// A rising sawtooth.
    Saw,
    /// Sample-and-hold noise: a new uniform random level every cycle.
    Noise,
}

/// An oscillator coupled into the analog signal path at the AFE input.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Inject {
    /// Shape of the waveform.
    pub wave: Wave,
    /// Oscillator frequency in hertz.
    pub freq_hz: f64,
    /// Peak amplitude at the AFE input in volts; the programmable gain amplifies it.
    pub amplitude_v: f64,
    /// Extra phase step per line in cycles, which tilts the bands.
    #[cfg_attr(feature = "serde", serde(default))]
    pub phase_skew: f64,
}

/// An oscillator modulating the programmable gain.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct GainMod {
    /// Shape of the waveform.
    pub wave: Wave,
    /// Oscillator frequency in hertz.
    pub freq_hz: f64,
    /// Relative gain swing; the gain moves within `1 ± depth` times its setting.
    pub depth: f64,
}

/// Voltages at the ADC input in readout order.
#[derive(Clone, PartialEq)]
pub struct Analog {
    volts: Box<[f32]>,
    index: u64,
    layout: Layout,
}

impl Analog {
    /// Creates a buffer of zero volts with the given layout.
    #[must_use]
    pub fn new(layout: Layout) -> Self {
        Self {
            volts: vec![0.0; layout.sample_count()].into_boxed_slice(),
            index: 0,
            layout,
        }
    }

    /// Returns all voltages, line by line.
    #[must_use]
    pub fn volts(&self) -> &[f32] {
        &self.volts
    }

    /// Returns all voltages mutably.
    pub fn volts_mut(&mut self) -> &mut [f32] {
        &mut self.volts
    }

    /// Returns the index of the frame the voltages belong to.
    #[must_use]
    pub fn index(&self) -> u64 {
        self.index
    }

    /// Returns the geometry and timing of the samples.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
}

impl Debug for Analog {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Analog")
            .field("index", &self.index)
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

/// Parameters of the analog chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Output amplifier conversion gain in microvolts per electron.
    pub conv_gain_uv_per_e: f64,
    /// White read noise in electrons, one standard deviation per sample.
    pub read_noise_e: f64,
    /// Read-noise corner in hertz, below which flicker exceeds the white floor; 0 disables it.
    pub noise_1f_corner_hz: f64,
    /// Programmable gain amplifier gain in decibels.
    pub pga_gain_db: f64,
    /// Voltage at the ADC input for zero charge.
    pub pedestal_v: f64,
    /// Small-signal bandwidth at the native pixel clock in hertz.
    pub bandwidth_hz: f64,
    /// Slew-rate limit at the native pixel clock in volts per microsecond.
    pub slew_v_per_us: f64,
    /// Duration of one pixel period of the camera's own clock in seconds.
    pub native_pixel_period_s: f64,
    /// Simulation sub-steps per pixel period.
    pub oversample: u32,
    /// Nominal ADC reference: the input voltage of the largest code.
    pub vref_v: f64,
    /// Delay of the CDS reset sample in pixel periods.
    pub cds_lag_px: f64,
    /// Fraction of the delayed level that the late reset sample subtracts.
    pub cds_mix: f64,
    /// Injected oscillator, if the preset patches one in.
    pub inject: Option<Inject>,
    /// Gain-modulating oscillator, if the preset patches one in.
    pub gain_mod: Option<GainMod>,
    /// Seed of the analog noise sources.
    pub seed: u64,
}

impl Params {
    /// Resolves the analog parameters from the camera, preset and derived timing.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let afe = &camera.afe;
        let bend = preset.bend.afe.clone().unwrap_or_default();
        Ok(Self {
            conv_gain_uv_per_e: camera.sensor.conv_gain_uv_per_e,
            read_noise_e: camera.sensor.read_noise_e,
            noise_1f_corner_hz: afe.noise_1f_corner_hz,
            pga_gain_db: afe.pga_gain_db,
            pedestal_v: afe.pedestal_v,
            bandwidth_hz: afe.bandwidth_hz,
            slew_v_per_us: afe.slew_v_per_us,
            native_pixel_period_s: timing.native_pixel_period_s,
            oversample: timing.oversample,
            vref_v: camera.adc.full_scale_v,
            cds_lag_px: bend.cds_lag_px,
            cds_mix: bend.cds_mix,
            inject: bend.inject,
            gain_mod: bend.gain_mod,
            seed: noise::stage_seed(preset.seed, SALT),
        })
    }

    /// Returns the linear gain of the programmable gain amplifier.
    #[must_use]
    pub fn pga_gain(&self) -> f64 {
        10f64.powf(self.pga_gain_db / 20.0)
    }

    /// Returns the total gain from electrons to volts at the ADC input.
    #[must_use]
    pub fn volts_per_electron(&self) -> f64 {
        self.conv_gain_uv_per_e * 1e-6 * self.pga_gain()
    }
}

/// An oscillator phase: whole cycles since power-on and the fraction of the current one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Phase {
    cycle: u64,
    frac: f64,
}

impl Phase {
    /// Returns the phase `cycles` later.
    fn advanced(self, cycles: f64) -> Self {
        let total = self.frac + cycles;
        let whole = total.floor();
        let mut next = Self {
            cycle: self.cycle.wrapping_add_signed(whole as i64),
            frac: total - whole,
        };
        if next.frac >= 1.0 {
            next.frac -= 1.0;
            next.cycle = next.cycle.wrapping_add(1);
        }
        next
    }
}

/// Persistent analog state carried from one frame to the next.
///
/// Holds the low-pass node, the flicker-noise process, the phases of both
/// oscillators and the supply rail. The default is the power-on state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    node_v: f64,
    flicker_v: f64,
    inject: Phase,
    gain: Phase,
    rail_v: f64,
}

impl State {
    /// Returns the injected oscillator's phase at the start of the next frame, in cycles.
    ///
    /// Always in `[0, 1)`.
    #[must_use]
    pub fn phase(&self) -> f64 {
        self.inject.frac
    }

    /// Returns the low-pass node voltage above the pedestal after the last frame.
    #[must_use]
    pub fn node_v(&self) -> f64 {
        self.node_v
    }

    /// Returns the ADC reference the supply delivered during the last frame, in volts.
    ///
    /// Zero before the first frame. The supply is ideal, so after a frame it equals
    /// [`Params::vref_v`]; a supply-sag model sets it below that, and the conversion
    /// scales every voltage by `vref_v / rail_v` as a ratiometric converter does.
    #[must_use]
    pub fn rail_v(&self) -> f64 {
        self.rail_v
    }
}

/// One oscillator of the chain with the seed of its sample-and-hold levels.
#[derive(Debug, Clone, Copy)]
struct Osc {
    wave: Wave,
    freq_hz: f64,
    skew: f64,
    salt: u32,
    seed: u64,
}

impl Osc {
    /// Returns the phase at time `t_s` of the frame, in line `line`, from the frame's start phase.
    fn phase(&self, start: Phase, t_s: f64, line: u32) -> Phase {
        start.advanced(self.freq_hz * t_s + self.skew * f64::from(line))
    }

    /// Returns a generator of the waveform from `phase` in steps of `step_s` seconds.
    fn sweep(&self, phase: Phase, step_s: f64) -> Sweep {
        let step = self.freq_hz * step_s;
        let (sin, cos) = (TAU * phase.frac).sin_cos();
        let (rot_sin, rot_cos) = (TAU * step).sin_cos();
        Sweep {
            osc: *self,
            base: phase.cycle,
            offset: phase.frac,
            step,
            phasor: (cos, sin),
            rotation: (rot_cos, rot_sin),
            held: None,
        }
    }

    /// Returns the waveform's value, in `[-1, 1]`, at `phase`.
    fn value(&self, phase: Phase) -> f64 {
        self.sweep(phase, 0.0).next()
    }
}

/// An oscillator's waveform sampled at evenly spaced instants.
///
/// The sine is generated by rotating a phasor, the other shapes from the phase.
struct Sweep {
    osc: Osc,
    /// Whole cycles at the first instant.
    base: u64,
    /// Phase of the next instant in cycles past `base`; never negative.
    offset: f64,
    /// Phase advance per instant in cycles.
    step: f64,
    /// Cosine and sine of the next instant's phase.
    phasor: (f64, f64),
    /// Cosine and sine of one step.
    rotation: (f64, f64),
    /// The current sample-and-hold cycle and its level.
    held: Option<(u64, f64)>,
}

impl Sweep {
    /// Returns the waveform at the next instant, in `[-1, 1]`.
    fn next(&mut self) -> f64 {
        let value = match self.osc.wave {
            Wave::Sine => {
                let (cos, sin) = self.phasor;
                let (rot_cos, rot_sin) = self.rotation;
                self.phasor = (cos * rot_cos - sin * rot_sin, sin * rot_cos + cos * rot_sin);
                sin
            }
            Wave::Square => {
                if self.offset.fract() < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Saw => 2.0 * self.offset.fract() - 1.0,
            Wave::Noise => {
                let cycle = self.base.wrapping_add(self.offset as u64);
                match self.held {
                    Some((held, level)) if held == cycle => level,
                    _ => {
                        let level = f64::from(noise::signed(
                            cycle as u32,
                            (cycle >> 32) as u32,
                            self.osc.salt,
                            self.osc.seed,
                        ));
                        self.held = Some((cycle, level));
                        level
                    }
                }
            }
        };
        self.offset += self.step;
        value
    }
}

/// Returns two independent standard normal values (Marsaglia's polar method).
fn normal_pair(rng: &mut Pcg32) -> (f64, f64) {
    loop {
        let u = f64::from(rng.next_u32()) * TWO_OVER_2_32 - 1.0;
        let v = f64::from(rng.next_u32()) * TWO_OVER_2_32 - 1.0;
        let s = u * u + v * v;
        if s > 0.0 && s < 1.0 {
            let m = (-2.0 * s.ln() / s).sqrt();
            return (u * m, v * m);
        }
    }
}

/// Read noise at the AFE input: white plus first-order flicker, in volts.
struct ReadNoise {
    rng: Pcg32,
    enabled: bool,
    white_v: f64,
    flicker_sigma_v: f64,
    corner_hz: f64,
    /// Flicker decay per pixel period.
    decay: f64,
    /// Flicker innovation per pixel period, in volts per unit normal.
    kick_v: f64,
    flicker_v: f64,
}

impl ReadNoise {
    fn new(params: &Params, pixel_s: f64, index: u64, flicker_v: f64) -> Self {
        let white_v = params.read_noise_e * params.conv_gain_uv_per_e * 1e-6;
        let corner_hz = params.noise_1f_corner_hz;
        // The white floor's one-sided density is 2 white_v^2 T at the native period T.
        // A first-order process with density 2 * 2 white_v^2 T at DC, and so the
        // floor's density at its corner, has variance 2 pi corner T white_v^2.
        let flicker_sigma_v = white_v * (TAU * corner_hz * params.native_pixel_period_s).sqrt();
        let decay = (-TAU * corner_hz * pixel_s).exp();
        Self {
            rng: Pcg32::new(params.seed, index),
            enabled: white_v > 0.0 || flicker_sigma_v > 0.0,
            white_v,
            flicker_sigma_v,
            corner_hz,
            decay,
            kick_v: flicker_sigma_v * (1.0 - decay * decay).sqrt(),
            flicker_v,
        }
    }

    /// Returns the noise of the next sample.
    fn sample(&mut self) -> f64 {
        if !self.enabled {
            return 0.0;
        }
        let (white, flicker) = normal_pair(&mut self.rng);
        self.flicker_v = self.flicker_v * self.decay + self.kick_v * flicker;
        self.white_v * white + self.flicker_v
    }

    /// Advances the flicker process over a blanking interval with its exact transition.
    fn skip(&mut self, duration_s: f64) {
        if self.flicker_sigma_v > 0.0 {
            let decay = (-TAU * self.corner_hz * duration_s).exp();
            let (kick, _) = normal_pair(&mut self.rng);
            self.flicker_v =
                self.flicker_v * decay + self.flicker_sigma_v * (1.0 - decay * decay).sqrt() * kick;
        }
    }
}

/// The analog chain over one frame: derived constants and the evolving node.
struct Front<'a> {
    params: &'a Params,
    volts_per_e: f64,
    gain: f64,
    oversample: u32,
    sub_s: f64,
    tau_s: f64,
    /// Node decay per sub-step and per pixel period.
    decay: (f64, f64),
    slew_v_per_s: f64,
    /// Largest node step per sub-step in volts.
    sub_slew_v: f64,
    /// CDS reset-sample delay in sub-steps.
    lag_sub: f64,
    /// Injected oscillator, its amplitude and its phase at the frame start.
    inject: Option<(Osc, f64, Phase)>,
    /// Gain-modulating oscillator, its depth and its phase at the frame start.
    gain_mod: Option<(Osc, f64, Phase)>,
    /// Factor that refers the ADC input to the nominal reference.
    reference: f64,
    noise: ReadNoise,
    node_v: f64,
}

impl<'a> Front<'a> {
    fn new(params: &'a Params, state: &State, layout: &Layout, index: u64) -> Self {
        let pixel_s = layout.pixel_period_s();
        let oversample = params.oversample.max(1);
        let sub_s = pixel_s / f64::from(oversample);
        // Bandwidth and slew rate were measured at the native pixel period: scale them by
        // pixel_period_s / native_pixel_period_s so they keep their meaning in pixels.
        let scale = pixel_s / params.native_pixel_period_s;
        let tau_s = scale / (TAU * params.bandwidth_hz);
        let slew_v_per_s = params.slew_v_per_us * 1e6 / scale;
        let decay = (-sub_s / tau_s).exp();
        let inject = params.inject.as_ref().map(|inject| {
            let osc = Osc {
                wave: inject.wave,
                freq_hz: inject.freq_hz,
                skew: inject.phase_skew,
                salt: INJECT_SALT,
                seed: params.seed,
            };
            (osc, inject.amplitude_v, state.inject)
        });
        let gain_mod = params.gain_mod.as_ref().map(|gain_mod| {
            let osc = Osc {
                wave: gain_mod.wave,
                freq_hz: gain_mod.freq_hz,
                skew: 0.0,
                salt: GAIN_SALT,
                seed: params.seed,
            };
            (osc, gain_mod.depth, state.gain)
        });
        Self {
            params,
            volts_per_e: params.conv_gain_uv_per_e * 1e-6,
            gain: params.pga_gain(),
            oversample,
            sub_s,
            tau_s,
            decay: (decay, decay.powf(f64::from(oversample))),
            slew_v_per_s,
            sub_slew_v: slew_v_per_s * sub_s,
            lag_sub: params.cds_lag_px * f64::from(oversample),
            inject,
            gain_mod,
            reference: params.vref_v / state.rail_v,
            noise: ReadNoise::new(params, pixel_s, index, state.flicker_v),
            node_v: state.node_v,
        }
    }

    /// Moves the node one sub-step towards `target_v`, within the slew limit.
    fn settle(&mut self, target_v: f64) {
        let next = target_v + (self.node_v - target_v) * self.decay.0;
        self.node_v += (next - self.node_v).clamp(-self.sub_slew_v, self.sub_slew_v);
    }

    /// Moves the node one pixel period towards a constant `target_v`.
    fn settle_pixel(&mut self, target_v: f64) {
        // The first sub-step moves furthest; if it is within the slew limit, all are,
        // and the whole period is one exponential step.
        if (target_v - self.node_v).abs() * (1.0 - self.decay.0) <= self.sub_slew_v {
            self.node_v = target_v + (self.node_v - target_v) * self.decay.1;
        } else {
            for _ in 0..self.oversample {
                self.settle(target_v);
            }
        }
    }

    /// Returns the CCD level the late reset sample sees at sub-step `sub` of `sample`, in volts.
    fn lagged(&self, electrons: &[f32], sample: usize, sub: u32) -> f64 {
        let oversample = f64::from(self.oversample);
        let at = (sample as f64 * oversample + f64::from(sub) + 0.5 - self.lag_sub) / oversample;
        if at < 0.0 {
            0.0
        } else {
            f64::from(electrons[at as usize]) * self.volts_per_e
        }
    }

    /// Runs the stored samples of `line`, which starts `start_s` into the frame.
    fn line(&mut self, line: u32, start_s: f64, electrons: &[f32], volts: &mut [f32]) {
        let first_s = start_s + 0.5 * self.sub_s;
        let mut inject = self.inject.map(|(osc, amplitude, start)| {
            (
                osc.sweep(osc.phase(start, first_s, line), self.sub_s),
                amplitude,
            )
        });
        let mut gain_mod = self.gain_mod.map(|(osc, depth, start)| {
            (
                osc.sweep(osc.phase(start, first_s, line), self.sub_s),
                depth,
            )
        });
        let cds_mix = self.params.cds_mix;
        let plain = inject.is_none() && gain_mod.is_none() && cds_mix <= 0.0;
        let pedestal_v = self.params.pedestal_v;
        for (sample, (volts, &charge)) in volts.iter_mut().zip(electrons).enumerate() {
            let level = f64::from(charge) * self.volts_per_e + self.noise.sample();
            if plain {
                self.settle_pixel(level * self.gain);
            } else {
                for sub in 0..self.oversample {
                    let mut input = level;
                    if cds_mix > 0.0 {
                        input -= cds_mix * self.lagged(electrons, sample, sub);
                    }
                    if let Some((sweep, amplitude)) = inject.as_mut() {
                        input += *amplitude * sweep.next();
                    }
                    let gain = gain_mod.as_mut().map_or(self.gain, |(sweep, depth)| {
                        self.gain * (1.0 + *depth * sweep.next())
                    });
                    self.settle(input * gain);
                }
            }
            *volts = ((pedestal_v + self.node_v) * self.reference) as f32;
        }
    }

    /// Advances the chain over a blanking interval of `duration_s` that ends at `end_s` in `end_line`.
    fn blank(&mut self, duration_s: f64, end_s: f64, end_line: u32) {
        self.noise.skip(duration_s);
        let mut input = self.noise.flicker_v;
        if let Some((osc, amplitude, start)) = self.inject {
            input += amplitude * osc.value(osc.phase(start, end_s, end_line));
        }
        let gain = self.gain_mod.map_or(self.gain, |(osc, depth, start)| {
            self.gain * (1.0 + depth * osc.value(osc.phase(start, end_s, end_line)))
        });
        let target_v = input * gain;
        let next = target_v + (self.node_v - target_v) * (-duration_s / self.tau_s).exp();
        let limit = self.slew_v_per_s * duration_s;
        self.node_v += (next - self.node_v).clamp(-limit, limit);
    }
}

/// Converts the charge stream to volts at the ADC input for frame `index`.
///
/// Runs the analog chain described in the [module documentation](self) over every
/// stored sample and every blanking interval of the frame, and leaves the analog
/// state ready for the next frame.
///
/// # Panics
///
/// Panics if `out` does not have the layout of `stream`.
pub fn process(params: &Params, state: &mut State, stream: &Stream, index: u64, out: &mut Analog) {
    let layout = stream.layout();
    assert!(
        out.layout == *layout,
        "analog layout {:?} does not match the stream layout {layout:?}",
        out.layout
    );
    state.rail_v = params.vref_v;
    let mut front = Front::new(params, state, layout, index);
    let line_s = layout.line_period_s();
    let hblank_s = f64::from(layout.hblank_px()) * layout.pixel_period_s();
    let line_len = layout.line_len() as usize;
    let lines = out
        .volts
        .chunks_exact_mut(line_len)
        .zip(stream.samples().chunks_exact(line_len));
    for (line, (volts, electrons)) in (0..layout.rows()).zip(lines) {
        let start_s = f64::from(line) * line_s;
        front.line(line, start_s, electrons, volts);
        front.blank(hblank_s, start_s + line_s, line + 1);
    }
    let frame_lines = layout.rows() + layout.vblank_lines();
    let frame_s = f64::from(frame_lines) * line_s;
    front.blank(
        f64::from(layout.vblank_lines()) * line_s,
        frame_s,
        frame_lines,
    );
    if let Some((osc, _, start)) = front.inject {
        state.inject = osc.phase(start, frame_s, frame_lines);
    }
    if let Some((osc, _, start)) = front.gain_mod {
        state.gain = osc.phase(start, frame_s, frame_lines);
    }
    state.node_v = front.node_v;
    state.flicker_v = front.noise.flicker_v;
    out.index = index;
}
