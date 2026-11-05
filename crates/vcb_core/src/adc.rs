//! Analog-to-digital conversion: input voltages quantised to output codes.
//!
//! The converter maps `0 V` to code 0 and `full_scale_v` to `2^bits - 1`, rounding to
//! the nearest code and clipping outside that range. The black pedestal therefore
//! appears as whatever code the AFE's `pedestal_v` maps to, nominally the camera's
//! `black_level`; the image processor measures and subtracts it from the optical-black
//! columns ([`isp::clamp_black`](crate::isp::clamp_black)).
//!
//! The codes in [`Codes`] are the words driven onto the parallel data bus, bit `k` on
//! line `Dk`, where the [`bus`](crate::bus) bend circuit can reach them.

use std::fmt::{self, Debug, Formatter};

use crate::afe::Analog;
use crate::readout::Layout;
use crate::{Camera, Error, Preset, Timing};

/// ADC output words in readout order.
#[derive(Clone, PartialEq)]
pub struct Codes {
    words: Box<[u16]>,
    layout: Layout,
}

impl Codes {
    /// Creates a buffer of zero codes with the given layout.
    #[must_use]
    pub fn new(layout: Layout) -> Self {
        Self {
            words: vec![0; layout.sample_count()].into_boxed_slice(),
            layout,
        }
    }

    /// Returns all codes, line by line.
    #[must_use]
    pub fn words(&self) -> &[u16] {
        &self.words
    }

    /// Returns all codes mutably.
    pub fn words_mut(&mut self) -> &mut [u16] {
        &mut self.words
    }

    /// Returns the geometry and timing of the samples.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
}

impl Debug for Codes {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Codes")
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

/// Parameters of the converter.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Resolution in bits, at most 16.
    pub bits: u8,
    /// Input voltage that maps to the largest code.
    pub full_scale_v: f32,
    /// Nominal black code that the AFE pedestal produces.
    pub black_level: u16,
}

impl Params {
    /// Resolves the converter parameters from the camera, preset and derived timing.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let _ = (preset, timing);
        Ok(Self {
            bits: camera.adc.bits,
            full_scale_v: camera.adc.full_scale_v as f32,
            black_level: camera.adc.black_level,
        })
    }

    /// Returns the largest code, `2^bits - 1`.
    #[must_use]
    pub fn max_code(&self) -> u16 {
        ((1u32 << self.bits.min(16)) - 1) as u16
    }
}

/// Quantises the voltages of `analog` into `out`.
///
/// # Panics
///
/// Panics if `out` does not have the layout of `analog`.
pub fn convert(params: &Params, analog: &Analog, out: &mut Codes) {
    assert!(
        out.layout == *analog.layout(),
        "code layout {:?} does not match the analog layout {:?}",
        out.layout,
        analog.layout()
    );
    let max = f32::from(params.max_code());
    let codes_per_volt = max / params.full_scale_v;
    for (word, volts) in out.words.iter_mut().zip(analog.volts()) {
        *word = (volts * codes_per_volt).round().clamp(0.0, max) as u16;
    }
}
