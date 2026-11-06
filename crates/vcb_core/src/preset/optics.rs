//! The `[bend.optics]` section: the IR-cut filter slid off the sensor.

use crate::Error;
use crate::validate::{non_negative, within};

/// Default NIR radiance weights of linear sRGB red, green and blue.
///
/// Red-weighted, since most surfaces reflect the near infrared about as they reflect
/// red (soil, wood, brick, skin), and with almost no blue, so a clear sky, blue by
/// Rayleigh scattering that falls off as the fourth power of the wavelength, stays
/// dark: pure blue gives 4 % of white's NIR. The weights sum to one, so a neutral
/// surface's NIR is `nir_scale` times its visible level. PROVISIONAL: a mild
/// assumption about the scene, tuned by eye on full-spectrum looks.
const DEFAULT_NIR_WEIGHTS: [f64; 3] = [0.58, 0.38, 0.04];

/// Default NIR reflectance of foliage relative to its visible green: off.
///
/// Green leaves reflect four to six times more near infrared than green light (the
/// Wood effect), but taking green to mean chlorophyll is an assumption about the
/// scene, so the estimate makes it only when a preset asks; 4 is a typical leaf.
const DEFAULT_FOLIAGE_NIR: f64 = 0.0;

/// The `[bend.optics]` section: the IR-cut filter removed from in front of the sensor.
///
/// In the DSC-V1 class of cameras a magnet slides the hot mirror off the sensor, and
/// without it the silicon sees near infrared (NIR) through all three colour dyes.
///
/// Two kinds of value meet here. The filter's position, the dyes' NIR response
/// (`[sensor] nir_response`), the defocus of the NIR image and of the filter's edge
/// (`[lens]`) and the camera's reaction (auto-exposure pulls the extra light back,
/// daylight white balance turns it pink, grey-world white balance fights the cast)
/// are the camera's physics. The NIR radiance of the scene is not: an RGB photo
/// records nothing beyond 700 nm, so it cannot be recovered, only estimated, as a
/// weighted sum of the scene's linear sRGB (`nir_weights`, a mild assumption) raised
/// on foliage by `foliage_nir` (an explicit assumption about the scene: that green
/// means chlorophyll; off by default) and scaled by `nir_scale`. Real NIR data would
/// replace the estimate as a fourth scene plane at the same point in the
/// [`scene`](crate::scene) stage, with nothing downstream changing.
///
/// ```toml
/// [bend.optics]
/// ir_cut_pulled_frac = 1.0       # the filter is fully off the sensor
/// nir_weights = [0.58, 0.38, 0.04]
/// foliage_nir = 0.0              # 4 lets foliage glow (the Wood effect)
/// nir_scale = 1.0
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct OpticsBend {
    /// Fraction of the frame height the filter has slid off, from the top, in `[0, 1]`.
    ///
    /// 1 is the filter removed, 0 the filter in place. The filter's shadow edge is
    /// defocused over the camera's `[lens] filter_edge_blur_frac` of the height; the
    /// edge travels from half that width above the top at 0 to half below the bottom
    /// at 1, so 0 leaves every row covered and 1 every row uncovered.
    pub ir_cut_pulled_frac: f64,
    /// Weights of the scene's linear sRGB red, green and blue in its NIR radiance.
    ///
    /// Dimensionless, each finite and at least 0: the NIR radiance of a pixel is
    /// `nir_weights · [r, g, b]` before the foliage and scale factors.
    pub nir_weights: [f64; 3],
    /// NIR reflectance of chlorophyll relative to its visible green, at least 0.
    ///
    /// Adds `foliage_nir * greenness * g` to the weighted sum, the greenness rising
    /// from 0 (green not above both red and 1.5 times blue) to 1 (green twice the
    /// larger of the two, a leaf): the Wood effect. 0, the default, treats foliage as
    /// any other surface; 4 is a typical leaf.
    pub foliage_nir: f64,
    /// Overall NIR radiance relative to visible for a scene of equal energy, at least 0.
    pub nir_scale: f64,
}

impl Default for OpticsBend {
    /// Returns the filter fully removed, the NIR estimated from the weighted sum alone.
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
