//! The `[bend.leak]` section: light entering the body behind the lens.

use std::fmt::{self, Display, Formatter};

use crate::Error;
use crate::validate::non_negative;

/// Default radiance of the leak at the opening, in multiples of reference white's.
///
/// PROVISIONAL: a leak half as bright as reference white where it enters, plainly
/// visible over a daylight scene without clipping on its own.
const DEFAULT_RADIANCE: f64 = 0.5;

/// Default distance over which the leak's glow falls by `1/e`, as a fraction of the frame.
///
/// PROVISIONAL: the glow covers about a third of the frame before it fades, the size of
/// a leak through a seam or a drilled hole a few millimetres across.
const DEFAULT_SPREAD_FRAC: f64 = 0.3;

/// Largest accepted spread of the glow as a fraction of the frame's extent.
///
/// At twice the frame the glow still falls by 39 % from the opening to the far edge; a
/// flatter one is a veil over the whole frame, and a larger value is a typo.
const MAX_SPREAD_FRAC: f64 = 2.0;

/// The `[bend.leak]` section: a light leak glowing onto the sensor from one edge.
///
/// A hole drilled in the body, an opened seam or a loosened lens mount lets light in
/// behind the lens, past the aperture, the shutter and the IR-cut filter. Scattered by
/// the cavity, it falls on the sensor as a soft glow, brightest at the opening, with
/// the irradiance `radiance * e^(-d / spread_frac)` at the distance `d` from `edge`
/// as a fraction of the frame's extent across that edge. Its near infrared reaches the
/// sensor through `[sensor] nir_response`, and its charge blooms, clips and smears like
/// the scene's. See [`sensor`](crate::sensor).
///
/// `radiance = 0` leaves the camera as it is.
///
/// ```toml
/// [bend.leak]
/// edge = "left"              # or "right", "top", "bottom", in the sensor's landscape orientation
/// radiance = 0.5             # half of reference white's radiance at the opening
/// spread_frac = 0.3          # the glow falls by 1/e over 0.3 of the frame's width
/// colour = [1.0, 0.5, 0.3]   # through a red plastic wall
/// nir = 1.0                  # daylight's near infrared
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct LeakBend {
    /// The edge of the sensor the light enters from, in its landscape orientation.
    ///
    /// The edge belongs to the sensor, like the amplifier glow's corner: a portrait
    /// frame, turned back after development, shows the leak turned with it.
    pub edge: Edge,
    /// Radiance at the opening in multiples of reference white's visible radiance, at least 0.
    ///
    /// 0 lets no light in. The leak bypasses the aperture, so the exposure scale of
    /// auto-exposure, which stands for the aperture, does not apply to it.
    pub radiance: f64,
    /// Distance over which the glow falls by `1/e`, a fraction of the frame, in `(0, 2]`.
    ///
    /// The fraction is of the frame's extent across `edge`: its width for a left or
    /// right leak, its height for a top or bottom one.
    pub spread_frac: f64,
    /// The light's colour in linear sRGB relative to white, each at least 0.
    ///
    /// `[1, 1, 1]` is daylight, the colour of reference white; a plastic wall passes
    /// red.
    pub colour: [f64; 3],
    /// The light's near infrared relative to its visible radiance, at least 0.
    ///
    /// 1 is daylight. The leak passes behind the IR-cut filter, so its near infrared
    /// adds through `[sensor] nir_response` and a daylight leak develops pink under the
    /// daylight balance, as a full-spectrum camera does.
    pub nir: f64,
}

impl Default for LeakBend {
    /// Returns a daylight leak from the left edge, half as bright as reference white.
    fn default() -> Self {
        Self {
            edge: Edge::default(),
            radiance: DEFAULT_RADIANCE,
            spread_frac: DEFAULT_SPREAD_FRAC,
            colour: [1.0; 3],
            nir: 1.0,
        }
    }
}

impl LeakBend {
    /// Checks every value of the section.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        non_negative("bend.leak.radiance", self.radiance)?;
        if !(self.spread_frac.is_finite()
            && self.spread_frac > 0.0
            && self.spread_frac <= MAX_SPREAD_FRAC)
        {
            return Err(Error::config(format!(
                "bend.leak.spread_frac must lie in (0, {MAX_SPREAD_FRAC}], got {}",
                self.spread_frac
            )));
        }
        for (channel, value) in ["r", "g", "b"].iter().zip(self.colour) {
            non_negative(&format!("bend.leak.colour ({channel})"), value)?;
        }
        non_negative("bend.leak.nir", self.nir)
    }
}

/// An edge of the sensor, in its landscape orientation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Edge {
    /// The edge of the first column.
    #[default]
    Left,
    /// The edge of the last column.
    Right,
    /// The edge of the first row in image order.
    Top,
    /// The edge of the last row in image order.
    Bottom,
}

impl Display for Edge {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        })
    }
}
