//! The tone curve: contrast about middle grey, a black toe and a highlight shoulder.

use crate::srgb;

/// Linear-light middle grey, the pivot of the contrast: 18 % reflectance.
const MIDDLE_GREY: f32 = 0.18;

/// Intervals of the tone lookup table over `[0, white]`.
///
/// With 4096 intervals linear interpolation of the combined tone and sRGB curve stays
/// within 0.01 of an 8-bit code everywhere, for a 16 KiB table.
const LUT_INTERVALS: usize = 4096;

/// The tone curve applied to every channel in linear light before sRGB encoding.
///
/// The curve is `T(x) = S(D(C(x)))`, with reference white at 1.0:
///
/// - Contrast: `C(x) = g (x / g)^contrast` with middle grey `g = 0.18`, so middle grey
///   stays put and the slope there is `contrast`.
/// - Toe: `D(u) = u (1 - exp(-u / toe))`, which compresses levels below about `toe`
///   towards black (slope 0 at black) and leaves middle grey within 0.02 % for
///   `toe <= 0.02`.
/// - Shoulder: below the knee `k = 1 - shoulder`, `S(u) = u`; above it
///   `S(u) = k + (1 - k) f((u - k) / (1 - k))` with the extended Reinhard roll-off
///   `f(t) = t (1 + t / L^2) / (1 + t)` and `L = (D(C(white)) - k) / (1 - k)`. `S` has
///   slope 1 at the knee, rises monotonically and maps `white`, the level at which
///   the sensor saturates, to exactly 1, so everything below saturation stays below 1.
///
/// Each part is the identity when its parameter is neutral, so `(1, 0, 0)` is the
/// identity exactly. The curve is monotonic for any non-negative parameters.
///
/// # Examples
///
/// ```
/// use vcb_core::isp::Tone;
///
/// let neutral = Tone { contrast: 1.0, toe: 0.0, shoulder: 0.0, white: 1.4 };
/// assert_eq!(neutral.apply(0.37), 0.37);
/// let camera = Tone { contrast: 1.15, toe: 0.02, shoulder: 0.1, white: 1.4 };
/// assert!((camera.apply(1.4) - 1.0).abs() < 1e-6);
/// assert!(camera.apply(1.0) < 0.99);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tone {
    /// Slope at middle grey; 1 leaves the mid-tones alone.
    pub contrast: f32,
    /// Linear level below which shadows are compressed towards black; 0 for none.
    pub toe: f32,
    /// Fraction of the output range below white given to the highlight roll-off; 0 for none.
    pub shoulder: f32,
    /// Linear input level, reference white 1.0, that the shoulder maps to output white.
    pub white: f32,
}

impl Tone {
    /// Maps a linear-light level to the tone-mapped linear level.
    #[must_use]
    pub fn apply(&self, x: f32) -> f32 {
        let x = x.max(0.0);
        let shaped = self.toe(self.contrast(x));
        if self.shoulder <= 0.0 {
            return shaped;
        }
        let knee = 1.0 - self.shoulder.min(1.0);
        let span = 1.0 - knee;
        let top = self.toe(self.contrast(self.white));
        if shaped <= knee || top <= knee || span <= 0.0 {
            return shaped;
        }
        let limit = (top - knee) / span;
        let t = (shaped - knee) / span;
        let rolled = t * (1.0 + t / (limit * limit)) / (1.0 + t);
        span.mul_add(rolled, knee)
    }

    fn contrast(&self, x: f32) -> f32 {
        if (self.contrast - 1.0).abs() <= f32::EPSILON {
            x
        } else {
            MIDDLE_GREY * (x / MIDDLE_GREY).powf(self.contrast)
        }
    }

    fn toe(&self, u: f32) -> f32 {
        if self.toe <= 0.0 {
            u
        } else {
            u * (1.0 - (-u / self.toe).exp())
        }
    }

    /// Fills `lut` with the sRGB encoding, on the 8-bit scale, of the curve over `[0, white]`.
    pub(super) fn fill_lut(&self, lut: &mut Vec<f32>) {
        let step = self.white / LUT_INTERVALS as f32;
        lut.clear();
        lut.extend((0..=LUT_INTERVALS).map(|i| srgb::encode(self.apply(i as f32 * step)) * 255.0));
    }

    /// Returns the encoded level of `x` on the 8-bit scale from a table made by [`Self::fill_lut`].
    pub(super) fn lookup(&self, lut: &[f32], x: f32) -> f32 {
        let pos = x.max(0.0) * (LUT_INTERVALS as f32 / self.white);
        if pos >= LUT_INTERVALS as f32 {
            return lut[LUT_INTERVALS];
        }
        let i = pos as usize;
        let frac = pos - i as f32;
        frac.mul_add(lut[i + 1] - lut[i], lut[i])
    }
}
