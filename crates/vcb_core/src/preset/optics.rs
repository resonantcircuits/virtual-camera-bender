//! The `[bend.optics]` section: the IR-cut filter slid off the sensor.

use crate::Error;
use crate::validate::{non_negative, within};

/// Default NIR radiance weights of linear sRGB red, green and blue.
///
/// Red-weighted, so warm surfaces glow, and with almost no blue, so a clear sky, whose
/// blue is Rayleigh scattering that falls off towards the infrared, stays dark.
/// PROVISIONAL: a choice in the spirit of full-spectrum photographs, to be tuned by eye.
const DEFAULT_NIR_WEIGHTS: [f64; 3] = [0.5, 0.32, 0.05];

/// Default NIR reflectance of foliage relative to its visible green.
///
/// Chlorophyll-bearing leaves reflect roughly four to six times more in the near
/// infrared than in the green (the Wood effect); PROVISIONAL at the low end.
const DEFAULT_FOLIAGE_NIR: f64 = 4.0;

/// The `[bend.optics]` section: the IR-cut filter removed from in front of the sensor.
///
/// In the DSC-V1 class of cameras a magnet slides the hot mirror off the sensor, and
/// without it the silicon sees near infrared (NIR) through all three colour dyes.
/// The NIR of the scene is unknowable from an sRGB photo, so it is estimated: a
/// weighted sum of the scene's linear sRGB (`nir_weights`), raised on foliage by
/// `foliage_nir` and scaled by `nir_scale`. The camera's `[sensor] nir_response`
/// turns it into charge and its `[lens]` section blurs it; see
/// [`scene`](crate::scene).
///
/// ```toml
/// [bend.optics]
/// ir_cut_pulled_frac = 1.0       # the filter is fully off the sensor
/// nir_weights = [0.5, 0.32, 0.05]
/// foliage_nir = 4.0
/// nir_scale = 1.0
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct OpticsBend {
    /// Fraction of the frame height the filter has slid off, from the top, in `[0, 1]`.
    ///
    /// 1 is the filter removed, 0 the filter in place. The filter's shadow edge is
    /// defocused over the camera's `[lens] filter_edge_blur_frac` of the height.
    pub ir_cut_pulled_frac: f64,
    /// Weights of the scene's linear sRGB red, green and blue in its NIR radiance.
    ///
    /// Dimensionless, each finite and at least 0: the NIR radiance of a pixel is
    /// `nir_weights · [r, g, b]` before the foliage and scale factors.
    pub nir_weights: [f64; 3],
    /// NIR reflectance of chlorophyll relative to its visible green, at least 0.
    ///
    /// Applied in proportion to a pixel's greenness (the Wood effect); 1 treats
    /// foliage as any other surface.
    pub foliage_nir: f64,
    /// Overall NIR radiance relative to visible for a scene of equal energy, at least 0.
    pub nir_scale: f64,
}

impl Default for OpticsBend {
    /// Returns the filter fully removed with the default NIR estimator.
    fn default() -> Self {
        Self {
            ir_cut_pulled_frac: 1.0,
            nir_weights: DEFAULT_NIR_WEIGHTS,
            foliage_nir: DEFAULT_FOLIAGE_NIR,
            nir_scale: 1.0,
        }
    }
}

impl OpticsBend {
    /// Checks every value of the section.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        within(
            "bend.optics.ir_cut_pulled_frac",
            self.ir_cut_pulled_frac,
            0.0,
            1.0,
        )?;
        for (channel, weight) in ["r", "g", "b"].iter().zip(self.nir_weights) {
            non_negative(&format!("bend.optics.nir_weights ({channel})"), weight)?;
        }
        non_negative("bend.optics.foliage_nir", self.foliage_nir)?;
        non_negative("bend.optics.nir_scale", self.nir_scale)
    }
}
