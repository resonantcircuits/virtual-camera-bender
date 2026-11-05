//! Scene preparation: map the input image onto the virtual sensor.
//!
//! The first stage of the chain turns an input image, already decoded to linear light
//! as a [`Scene`], into the radiance that falls on each photosite of the sensor. The
//! input is centre-cropped to the sensor's aspect ratio and resampled to the sensor
//! size: area averaging when downscaling (every photosite integrates the light over
//! its footprint), linear interpolation when upscaling, a plain copy when the sizes
//! match. An input that exceeds the sensor by at most one pixel per axis, as in
//! match-input mode with an odd input size, loses its last row or column instead.
//!
//! The output is a [`Scene`] at sensor resolution in the same linear-light units as
//! the input; [`sensor::expose`](crate::sensor::expose) consumes it.

pub(crate) mod model;

use crate::resample;
use crate::{Camera, Error, Preset, Scene, Timing};

/// One colour channel of a [`Scene`] or a colour filter site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Long-wavelength channel.
    Red,
    /// Medium-wavelength channel.
    Green,
    /// Short-wavelength channel.
    Blue,
}

impl Channel {
    /// All channels in `R, G, B` order.
    pub const ALL: [Self; 3] = [Self::Red, Self::Green, Self::Blue];

    /// Returns the channel's position in `R, G, B` order.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Red => 0,
            Self::Green => 1,
            Self::Blue => 2,
        }
    }
}

/// Parameters of scene preparation.
///
/// Cropping and resampling follow from the input and sensor sizes alone, so the
/// current model has no tunable values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Params;

impl Params {
    /// Resolves the scene parameters from the camera, preset and derived timing.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let _ = (camera, preset, timing);
        Ok(Self)
    }
}

/// Crops and resamples `input` onto the sensor-sized scene `out`.
///
/// The output size is the size of `out`; `input` may have any non-zero size.
///
/// # Panics
///
/// Panics if `input` is empty.
pub fn prepare(params: &Params, input: &Scene, out: &mut Scene) {
    let _ = params;
    assert!(
        input.width() > 0 && input.height() > 0,
        "cannot prepare an empty {} x {} scene",
        input.width(),
        input.height()
    );
    let crop = resample::cover_crop(input.width(), input.height(), out.width(), out.height());
    for (src, dst) in input.planes().into_iter().zip(out.planes_mut()) {
        resample::resample(src, crop, dst);
    }
}
