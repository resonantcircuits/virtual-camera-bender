//! Exposure: scene radiance integrated into photosite charge behind the colour filters.
//!
//! The sensor is an interline CCD with an RGGB Bayer colour filter array: photosite
//! `(0, 0)` is red, `(1, 0)` and `(0, 1)` are green, `(1, 1)` is blue, repeating every
//! two columns and rows. Each photosite integrates the scene channel of its filter.
//!
//! The exposure is normalised so that reference white (scene value 1.0) fills
//! `white_fraction` of the full well; brighter light fills the well up to
//! `full_well_e`, where the charge clips. Negative scene values collect no charge.
//!
//! [`expose`] writes a [`Charge`] plane in electrons at sensor resolution, which
//! [`readout::read`](crate::readout::read) transfers out of the array.

use std::fmt::{self, Display, Formatter};

use crate::noise;
use crate::plane::Plane;
use crate::preset::Mode;
use crate::scene::Channel;
use crate::{Camera, Error, Preset, Scene, Timing};

/// Salt of the sensor's noise sources.
const SALT: u32 = noise::salt("sensor");

/// Layout of the colour filter array.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Cfa {
    /// Bayer pattern with red at `(0, 0)` and blue at `(1, 1)`.
    #[default]
    #[cfg_attr(feature = "serde", serde(rename = "RGGB"))]
    Rggb,
}

impl Cfa {
    /// Returns the filter colour of the photosite at column `x`, row `y`.
    #[must_use]
    pub fn channel(self, x: u32, y: u32) -> Channel {
        match self {
            Self::Rggb => match (x % 2, y % 2) {
                (0, 0) => Channel::Red,
                (1, 1) => Channel::Blue,
                _ => Channel::Green,
            },
        }
    }
}

impl Display for Cfa {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Rggb => "RGGB",
        })
    }
}

/// Electrons collected by every photosite, in colour filter array layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Charge {
    plane: Plane<f32>,
}

impl Charge {
    /// Creates an empty charge plane of `width` by `height` photosites.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            plane: Plane::new(width, height),
        }
    }

    /// Returns the charge in electrons per photosite.
    #[must_use]
    pub fn plane(&self) -> &Plane<f32> {
        &self.plane
    }

    /// Returns the charge plane mutably.
    pub fn plane_mut(&mut self) -> &mut Plane<f32> {
        &mut self.plane
    }
}

/// Physical parameters of the photosite array for one readout mode.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Full-well capacity in electrons; charge clips here.
    pub full_well_e: f32,
    /// Fraction of the full well that reference white fills.
    pub white_fraction: f32,
    /// Mean dark current in electrons per second.
    pub dark_e_per_s: f32,
    /// Photo-response non-uniformity, relative gain spread (one standard deviation).
    pub prnu: f32,
    /// Dark-signal non-uniformity in electrons (one standard deviation).
    pub dsnu_e: f32,
    /// Fraction of the charge above full well that the overflow drain removes.
    pub antibloom_ratio: f32,
    /// Interline smear relative to the signal in decibels.
    pub smear_db: f32,
    /// Hot photosites per megapixel.
    pub hot_pixels_per_mpix: f32,
    /// Colour filter array layout.
    pub cfa: Cfa,
    /// Integration time in seconds.
    pub exposure_s: f32,
    /// Whether the readout leaves the array exposed during transfer (electronic shutter).
    pub smear: bool,
    /// Seed of the sensor's noise sources.
    pub seed: u64,
}

impl Params {
    /// Resolves the sensor parameters from the camera, preset and derived timing.
    ///
    /// Movie mode integrates for one frame period and enables smear if the camera's
    /// movie section does; still mode uses the still exposure without smear.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let sensor = &camera.sensor;
        let (exposure_s, smear) = match timing.mode {
            Mode::Still => (camera.still.exposure_s, false),
            Mode::Movie => (1.0 / camera.movie.fps, camera.movie.smear),
        };
        Ok(Self {
            full_well_e: sensor.full_well_e as f32,
            white_fraction: sensor.white_fraction as f32,
            dark_e_per_s: sensor.dark_e_per_s as f32,
            prnu: sensor.prnu as f32,
            dsnu_e: sensor.dsnu_e as f32,
            antibloom_ratio: sensor.antibloom_ratio as f32,
            smear_db: sensor.smear_db as f32,
            hot_pixels_per_mpix: sensor.hot_pixels_per_mpix as f32,
            cfa: sensor.cfa,
            exposure_s: exposure_s as f32,
            smear,
            seed: noise::stage_seed(preset.seed, SALT),
        })
    }
}

/// Persistent state of the photosite array between frames.
///
/// An interline CCD empties every photosite at each transfer, so no charge carries
/// over between frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct State;

/// Integrates `scene` into photosite charge for frame `index`.
///
/// Each photosite collects its filter's scene channel scaled so that 1.0 fills
/// `white_fraction` of the well, clipped to `[0, full_well_e]`.
///
/// # Panics
///
/// Panics if `out` does not have the size of `scene`.
pub fn expose(params: &Params, state: &mut State, scene: &Scene, index: u64, out: &mut Charge) {
    let _ = (state, index);
    let (width, height) = (scene.width(), scene.height());
    assert!(
        out.plane.width() == width && out.plane.height() == height,
        "charge plane is {} x {} but the scene is {width} x {height}",
        out.plane.width(),
        out.plane.height()
    );
    let electrons_per_unit = params.white_fraction * params.full_well_e;
    for (y, row) in (0..height).zip(out.plane.rows_mut()) {
        let source = Channel::ALL.map(|channel| scene.plane(channel).row(y));
        for (x, charge) in (0..width).zip(row.iter_mut()) {
            let radiance = source[params.cfa.channel(x, y).index()][x as usize];
            *charge = (radiance * electrons_per_unit).clamp(0.0, params.full_well_e);
        }
    }
}
