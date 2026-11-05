//! Scene preparation: map the input image onto the virtual sensor.
//!
//! The first stage of the chain turns an input image, already decoded to linear light
//! as a [`Scene`], into the radiance that falls on each photosite of the sensor:
//!
//! 1. Geometry. When the camera is held in portrait, the input is first turned 90
//!    degrees clockwise so that it lies on the landscape sensor; the simulator turns
//!    the developed frame back. The input is centre-cropped to the sensor's aspect
//!    ratio and resampled to the sensor size: area averaging when downscaling (every
//!    photosite integrates the light over its footprint), linear interpolation when
//!    upscaling, a plain copy when the sizes match. An input that exceeds the sensor
//!    by at most one pixel per axis, as in match-input mode with an odd input size,
//!    loses its last row or column instead.
//! 2. Highlight reconstruction. The input's highlights were compressed and clipped by
//!    the camera that took it; they are expanded so that clipped white reaches
//!    `headroom` times reference white, coherently across channels. The method is
//!    described in the preset's [`SceneInput`](crate::preset::SceneInput) and below.
//! 3. Lens. Natural vignetting dims the image towards the corners by the cos^4 law:
//!    with `r` the distance from the centre relative to the half-diagonal, the light
//!    is multiplied by `(1 + r^2 tan^2 a)^-2`, where the corner field angle `a` is
//!    chosen so the corner loses `vignette_stops`: `tan^2 a = 2^(stops / 2) - 1`.
//! 4. Colour. Linear sRGB is converted into the sensor's own primaries: the camera's
//!    colour matrix ([`Params::srgb_to_cam`]) followed by division by the daylight
//!    white-balance gains, the inverse of what the image processor applies. A
//!    saturated colour outside the sensor's gamut can map to a negative response in
//!    one channel; a photosite cannot hold negative charge, so it is clipped to zero.
//!
//! # Highlight reconstruction
//!
//! Below `shoulder_start` (linear light) the input is unchanged. Above it, the inverse
//! of a soft shoulder, C1 continuous at `shoulder_start` and monotonic, maps input
//! white to `headroom`; the gain of a pixel's brightest channel is applied to all
//! three, so colours keep their channel ratios. Channels at 254/255 or above in the
//! 8-bit source are clipped: where a pixel also has unclipped channels, its clipped
//! channels are estimated from the unclipped ones and the colour of the unclipped
//! highlights in its neighbourhood (a 16 by 16 grid over the frame). Where every
//! channel clipped, the pixel becomes `headroom` times white. Detection happens
//! after resampling, so on a downscaled sensor a photosite counts as clipped only
//! where its whole footprint clipped. `headroom = 1` leaves the input unchanged.
//!
//! The output is a [`Scene`] at sensor resolution in linear light, reference white at
//! 1.0 in the brightest channel; [`sensor::expose`](crate::sensor::expose) consumes it.

mod highlight;
pub(crate) mod model;

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

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
    /// Brightness of the input's clipped white in multiples of reference white.
    pub headroom: f32,
    /// Linear input level where the highlight shoulder begins.
    pub shoulder_start: f32,
    /// Illumination falloff of the lens at the image corner in stops.
    pub vignette_stops: f32,
}

impl Params {
    /// Resolves the scene parameters from the camera, preset and derived timing.
    ///
    /// The sensor response always comes from the camera file: its `xyz_to_cam` matrix
    /// and daylight gains. Preset overrides change only what the image processor
    /// assumes, so they show up as colour errors, as on a real camera. The highlight
    /// reconstruction comes from the preset's `[scene]` section, the vignetting from
    /// the camera's `[lens]`.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if the camera's colour matrix is not invertible.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let srgb_to_cam = colour::srgb_to_cam(&camera.isp.xyz_to_cam)?;
        Ok(Self {
            rotated: timing.rotated,
            srgb_to_cam: colour::to_f32(&srgb_to_cam),
            wb_gains: [
                camera.isp.wb_r_gain as f32,
                1.0,
                camera.isp.wb_b_gain as f32,
            ],
            headroom: preset.scene.headroom as f32,
            shoulder_start: preset.scene.shoulder_start as f32,
            vignette_stops: camera.lens.vignette_stops as f32,
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
    if params.headroom > 1.0 {
        let shoulder = highlight::Shoulder::new(params.headroom, params.shoulder_start);
        highlight::reconstruct(&shoulder, out);
    }
    to_sensor(params, out);
}

/// Applies the lens falloff and converts linear sRGB to un-white-balanced camera RGB in place.
///
/// Negative responses are clipped at zero.
fn to_sensor(params: &Params, scene: &mut Scene) {
    let (width, height) = (scene.width() as usize, scene.height() as usize);
    if width == 0 {
        return;
    }
    let scale = params.wb_gains.map(|gain| 1.0 / gain);
    let tan_sq = 2f32.powf(0.5 * params.vignette_stops) - 1.0;
    let (cx, cy) = (0.5 * width as f32, 0.5 * height as f32);
    let inv_diagonal_sq = 1.0 / cx.mul_add(cx, cy * cy);
    let [red, green, blue] = scene.planes_mut();
    red.data_mut()
        .par_chunks_mut(width)
        .zip(green.data_mut().par_chunks_mut(width))
        .zip(blue.data_mut().par_chunks_mut(width))
        .enumerate()
        .for_each(|(y, ((rs, gs), bs))| {
            let dy = y as f32 + 0.5 - cy;
            for (x, ((r, g), b)) in rs.iter_mut().zip(gs).zip(bs).enumerate() {
                let dx = x as f32 + 0.5 - cx;
                let falloff = if tan_sq > 0.0 {
                    let r_sq = dx.mul_add(dx, dy * dy) * inv_diagonal_sq;
                    let base = r_sq.mul_add(tan_sq, 1.0);
                    1.0 / (base * base)
                } else {
                    1.0
                };
                let cam = colour::apply(&params.srgb_to_cam, [*r, *g, *b]);
                *r = (cam[0] * scale[0] * falloff).max(0.0);
                *g = (cam[1] * scale[1] * falloff).max(0.0);
                *b = (cam[2] * scale[2] * falloff).max(0.0);
            }
        });
}
