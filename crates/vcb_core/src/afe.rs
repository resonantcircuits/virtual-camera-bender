//! Analog front end: charge samples converted to the voltage at the ADC input.
//!
//! The CCD's output amplifier turns each charge packet into a voltage step of
//! `conv_gain_uv_per_e` microvolts per electron. Correlated double sampling removes
//! the reset level, the programmable gain amplifier multiplies the signal by
//! `pga_gain_db`, and the black-level clamp adds `pedestal_v`, so zero charge sits at
//! the pedestal and the signal rises above it:
//!
//! `volts = pedestal_v + electrons * conv_gain * 10^(pga_gain_db / 20)`
//!
//! The analog chain runs on the pixel clock in continuous time. An injected
//! oscillator ([`Inject`]) is free-running: its phase, kept in [`State`], advances by
//! `freq_hz` times the frame period on every frame, blanking included, so patterns
//! roll continuously from one frame to the next instead of restarting.

use std::fmt::{self, Debug, Formatter};

use crate::noise;
use crate::readout::{Layout, Stream};
use crate::{Camera, Error, Preset, Timing};

/// Salt of the analog front end's noise sources.
const SALT: u32 = noise::salt("afe");

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
    /// Sample-and-hold noise, a new random level every cycle.
    Noise,
}

/// An oscillator coupled into the analog signal path ahead of the ADC.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Inject {
    /// Shape of the waveform.
    pub wave: Wave,
    /// Oscillator frequency in hertz.
    pub freq_hz: f64,
    /// Peak amplitude at the ADC input in volts.
    pub amplitude_v: f64,
}

/// Voltages at the ADC input in readout order.
#[derive(Clone, PartialEq)]
pub struct Analog {
    volts: Box<[f32]>,
    layout: Layout,
}

impl Analog {
    /// Creates a buffer of zero volts with the given layout.
    #[must_use]
    pub fn new(layout: Layout) -> Self {
        Self {
            volts: vec![0.0; layout.sample_count()].into_boxed_slice(),
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

    /// Returns the geometry and timing of the samples.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
}

impl Debug for Analog {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Analog")
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

/// Parameters of the analog chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Output amplifier conversion gain in microvolts per electron.
    pub conv_gain_uv_per_e: f32,
    /// Programmable gain amplifier gain in decibels.
    pub pga_gain_db: f32,
    /// Voltage at the ADC input for zero charge.
    pub pedestal_v: f32,
    /// Small-signal bandwidth in hertz.
    pub bandwidth_hz: f64,
    /// Slew-rate limit in volts per microsecond.
    pub slew_v_per_us: f64,
    /// Simulation sub-steps per pixel period.
    pub oversample: u32,
    /// Injected oscillator, if the preset patches one in.
    pub inject: Option<Inject>,
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
        Ok(Self {
            conv_gain_uv_per_e: camera.sensor.conv_gain_uv_per_e as f32,
            pga_gain_db: afe.pga_gain_db as f32,
            pedestal_v: afe.pedestal_v as f32,
            bandwidth_hz: afe.bandwidth_hz,
            slew_v_per_us: afe.slew_v_per_us,
            oversample: timing.oversample,
            inject: preset.bend.afe.as_ref().and_then(|afe| afe.inject.clone()),
            seed: noise::stage_seed(preset.seed, SALT),
        })
    }

    /// Returns the total gain from electrons to volts at the ADC input.
    #[must_use]
    pub fn volts_per_electron(&self) -> f32 {
        self.conv_gain_uv_per_e * 1e-6 * 10f32.powf(self.pga_gain_db / 20.0)
    }
}

/// Persistent analog state carried from one frame to the next.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// Oscillator phase at the start of the next frame, in cycles.
    ///
    /// Always in `[0, 1)`.
    pub phase: f64,
}

/// Converts the charge stream to volts at the ADC input for frame `index`.
///
/// Applies conversion gain, PGA gain and the pedestal to every sample, then advances
/// the injected oscillator's phase by one frame period.
///
/// # Panics
///
/// Panics if `out` does not have the layout of `stream`.
pub fn process(params: &Params, state: &mut State, stream: &Stream, index: u64, out: &mut Analog) {
    let _ = index;
    assert!(
        out.layout == *stream.layout(),
        "analog layout {:?} does not match the stream layout {:?}",
        out.layout,
        stream.layout()
    );
    let gain = params.volts_per_electron();
    for (volts, electrons) in out.volts.iter_mut().zip(stream.samples()) {
        *volts = electrons.mul_add(gain, params.pedestal_v);
    }
    if let Some(inject) = &params.inject {
        let cycles = inject.freq_hz * stream.layout().frame_period_s();
        state.phase = (state.phase + cycles).rem_euclid(1.0);
    }
}
