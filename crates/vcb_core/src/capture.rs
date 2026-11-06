//! Processor capture: the bus words as the processor's own clock latches them.
//!
//! # Place in the chain
//!
//! Scene, sensor, readout, power, AFE, ADC, bus bend, then **[`reframe`]**, then the
//! black clamp, memory, development and codec. [`reframe`] rewrites the ADC words in
//! place, between the bus bend and the processor's black clamp.
//!
//! # Model
//!
//! A preset's `[bend.clock]` replaces the oscillator of the timing generator and the
//! AFE with one at `osc_mhz`, while the processor keeps its own crystal at the
//! camera's nominal pixel clock, generates vertical sync and latches the bus once per
//! [`Timing::dsp_pixel_period_s`](crate::Timing::dsp_pixel_period_s). The sensor side
//! runs on [`Timing::pixel_period_s`](crate::Timing::pixel_period_s); the two
//! differ by [`Timing::clock_ratio`](crate::Timing::clock_ratio).
//!
//! The reclocking itself, mapping each processor latch instant to the bus word valid
//! then, arrives in stage 12 of `docs/PLAN.md` (section 3.9, "Clock"). Until then
//! [`reframe`] passes the words through unchanged, with or without `[bend.clock]`.

use crate::adc::Codes;
use crate::preset::ReclockBend;
use crate::{Camera, Error, Preset, Timing};

/// The resolved capture, present while the preset has a `[bend.clock]` section.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Frequency of the replacement oscillator in hertz.
    pub osc_hz: f64,
    /// The camera's nominal pixel clock of the readout mode in hertz.
    pub nominal_hz: f64,
    /// Duration of the processor's capture period in seconds.
    pub dsp_pixel_period_s: f64,
    /// The preset's `[bend.clock]` section as written.
    pub bend: ReclockBend,
}

impl Params {
    /// Resolves the capture from the preset's `[bend.clock]` section; `None` without one.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value of the section is out of range.
    pub fn resolve(
        camera: &Camera,
        preset: &Preset,
        timing: &Timing,
    ) -> Result<Option<Self>, Error> {
        let Some(bend) = preset.bend.clock else {
            return Ok(None);
        };
        bend.validate()?;
        Ok(Some(Self {
            osc_hz: bend.osc_mhz * 1e6,
            nominal_hz: camera.clocking(timing.mode).pixel_clock_hz,
            dsp_pixel_period_s: timing.dsp_pixel_period_s,
            bend,
        }))
    }

    /// Returns the replacement oscillator's frequency over the nominal pixel clock.
    #[must_use]
    pub fn clock_ratio(&self) -> f64 {
        self.osc_hz / self.nominal_hz
    }
}

/// The capture's memory from frame to frame; the default is the power-on state.
///
/// Holds nothing yet: the drift oscillator's phase joins it with the reclocking.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct State;

/// Rewrites `codes` in place with the words the processor captures in frame `index`.
///
/// Without a bend (`params` is `None`) it does nothing. With one the words pass
/// through unchanged, as the reclocking is not yet in place.
pub fn reframe(params: Option<&Params>, state: &mut State, index: u64, codes: &mut Codes) {
    let Some(params) = params else {
        return;
    };
    let _ = (params, state, index, codes);
}
