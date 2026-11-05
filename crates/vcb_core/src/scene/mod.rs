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
//! When the camera is held in portrait, the input is first turned 90 degrees clockwise
//! so that it lies on the landscape sensor; the simulator turns the developed frame
//! back.
//!
//! The colours are then converted from linear sRGB into the sensor's own primaries:
//! the camera's colour matrix ([`Params::srgb_to_cam`]) followed by division by the
//! daylight white-balance gains, the inverse of what the image processor applies. A
//! saturated colour outside the sensor's gamut can map to a negative response in one
//! channel; a photosite cannot hold negative charge, so it is clipped to zero there.
//!
//! The output is a [`Scene`] at sensor resolution in linear light, reference white at
//! 1.0 in the brightest channel; [`sensor::expose`](crate::sensor::expose) consumes it.

pub(crate) mod model;

use crate::resample::{self, Turn};
use crate::{Camera, Error, Preset, Scene, Timing, colour};

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
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Whether the camera is held in portrait: the scene is turned 90 degrees clockwise.
    pub rotated: bool,
    /// Matrix from linear sRGB to white-balanced camera RGB, row major.
    pub srgb_to_cam: [[f32; 3]; 3],
    /// Daylight white-balance gains `[r, 1, b]` of the sensor, divided out of the scene.
    pub wb_gains: [f32; 3],
}

impl Params {
    /// Resolves the scene parameters from the camera, preset and derived timing.
    ///
    /// The sensor response always comes from the camera file: its `xyz_to_cam` matrix
    /// and daylight gains. Preset overrides change only what the image processor
    /// assumes, so they show up as colour errors, as on a real camera.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if the camera's colour matrix is not invertible.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let _ = preset;
        let srgb_to_cam = colour::srgb_to_cam(&camera.isp.xyz_to_cam)?;
        Ok(Self {
            rotated: timing.rotated,
            srgb_to_cam: colour::to_f32(&srgb_to_cam),
            wb_gains: [
                camera.isp.wb_r_gain as f32,
                1.0,
                camera.isp.wb_b_gain as f32,
            ],
        })
    }
}

/// Maps `input` onto the sensor-sized scene `out`, in the sensor's colour primaries.
///
/// The output size is the size of `out`; `input` may have any non-zero size.
///
/// # Panics
///
/// Panics if `input` is empty.
pub fn prepare(params: &Params, input: &Scene, out: &mut Scene) {
    assert!(
        input.width() > 0 && input.height() > 0,
        "cannot prepare an empty {} x {} scene",
        input.width(),
        input.height()
    );
    let (turn, width, height) = if params.rotated {
        (Turn::Clockwise, input.height(), input.width())
    } else {
        (Turn::None, input.width(), input.height())
    };
    let crop = resample::cover_crop(width, height, out.width(), out.height());
    for (src, dst) in input.planes().into_iter().zip(out.planes_mut()) {
        resample::resample(src, turn, crop, dst);
    }
    to_sensor_primaries(params, out);
}

/// Converts linear sRGB to un-white-balanced camera RGB in place, clipping at zero.
fn to_sensor_primaries(params: &Params, scene: &mut Scene) {
    let scale = params.wb_gains.map(|gain| 1.0 / gain);
    let [red, green, blue] = scene.planes_mut();
    for ((r, g), b) in red
        .data_mut()
        .iter_mut()
        .zip(green.data_mut())
        .zip(blue.data_mut())
    {
        let cam = colour::apply(&params.srgb_to_cam, [*r, *g, *b]);
        *r = (cam[0] * scale[0]).max(0.0);
        *g = (cam[1] * scale[1]).max(0.0);
        *b = (cam[2] * scale[2]).max(0.0);
    }
}
