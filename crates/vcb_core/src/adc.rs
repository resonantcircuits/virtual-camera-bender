//! Analog-to-digital conversion: input voltages quantised to output codes.
//!
//! The converter maps `0 V` to code 0 and `full_scale_v` to `2^bits - 1`, rounding to
//! the nearest code and clipping outside that range. The black pedestal therefore
//! appears as whatever code the AFE's `pedestal_v` maps to, nominally the camera's
//! `black_level`; the image processor measures and subtracts it from the optical-black
//! columns ([`isp::clamp_black`](crate::isp::clamp_black)).
//!
//! Before rounding, every sample receives a dither of [`DITHER_LSB`] codes peak to
//! peak, uniform and centred on zero, drawn from a hash of the sample's column and
//! image row, the stage seed and the frame index. It stands for the converter's own
//! noise (`docs/research/a520-technical.md`, section 5, quotes 0.8 codes rms for the
//! whole AD9949 chain) and keeps slow ramps from locking onto code boundaries. Keying
//! it to the image position rather than the time order keeps the readout direction
//! from changing the image.
//!
//! The codes in [`Codes`] are the words driven onto the parallel data bus, bit `k` on
//! line `Dk`, where the [`bus`](crate::bus) bend circuit can reach them.

use std::fmt::{self, Debug, Formatter};

use crate::afe::Analog;
use crate::noise;
use crate::readout::Layout;
use crate::{Camera, Error, Preset, Timing};

/// Salt of the converter's dither.
const SALT: u32 = noise::salt("adc");

/// Peak-to-peak span of the conversion dither in codes.
///
/// One third of a code: small against the AFE's own noise of about 0.8 codes rms
/// (AD9949 class, section 5), enough to break up quantisation contours.
pub const DITHER_LSB: f32 = 1.0 / 3.0;

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
    /// Seed of the conversion dither.
    pub seed: u64,
}

impl Params {
    /// Resolves the converter parameters from the camera, preset and derived timing.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let _ = timing;
        Ok(Self {
            bits: camera.adc.bits,
            full_scale_v: camera.adc.full_scale_v as f32,
            black_level: camera.adc.black_level,
            seed: noise::stage_seed(preset.seed, SALT),
        })
    }

    /// Returns the largest code, `2^bits - 1`.
    #[must_use]
    pub fn max_code(&self) -> u16 {
        ((1u32 << self.bits.min(16)) - 1) as u16
    }
}

/// Quantises the voltages of `analog` into `out`, with dither.
///
/// The dither of each sample depends on its position in the line, its image row and
/// the frame index carried by `analog`, so it differs from frame to frame and repeats
/// exactly for the same frame.
///
/// # Panics
///
/// Panics if `out` does not have the layout of `analog`.
pub fn convert(params: &Params, analog: &Analog, out: &mut Codes) {
    let layout = analog.layout();
    assert!(
        out.layout == *layout,
        "code layout {:?} does not match the analog layout {layout:?}",
        out.layout
    );
    let max = f32::from(params.max_code());
    let codes_per_volt = max / params.full_scale_v;
    let seed = params.seed ^ analog.index();
    let half_span = 0.5 * DITHER_LSB;
    let line_len = (layout.line_len() as usize).max(1);
    let lines = out
        .words
        .chunks_exact_mut(line_len)
        .zip(analog.volts().chunks_exact(line_len));
    for (line, (words, volts)) in (0..layout.rows()).zip(lines) {
        let row = layout.image_row(line);
        for (x, (word, volts)) in (0u32..).zip(words.iter_mut().zip(volts)) {
            let dither = half_span * noise::signed(x, row, SALT, seed);
            *word = (volts * codes_per_volt + dither).round().clamp(0.0, max) as u16;
        }
    }
}
