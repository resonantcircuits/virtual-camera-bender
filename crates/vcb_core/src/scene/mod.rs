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
//! 5. Near infrared, with `[bend.optics]` only: where the IR-cut filter has slid off
//!    the sensor, the scene's estimated near infrared, defocused, adds charge to every
//!    channel ([`Optics`], described below).
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
//! # Near infrared without the IR-cut filter
//!
//! A hot mirror in front of the sensor reflects the light beyond about 700 nm. Silicon
//! stays sensitive to about 1100 nm and the Bayer dyes turn nearly transparent beyond
//! about 750 nm, so where the filter is slid away (the magnet of the DSC-V1's night
//! mode; a removed or clear-glass filter in other cameras) every photosite collects
//! near infrared (NIR) on top of its visible band, red most.
//!
//! Two kinds of value meet in this model:
//!
//! - The camera's physics: where the filter sits (`ir_cut_pulled_frac`), the charge
//!   each dye passes from NIR (`[sensor] nir_response`), how far out of focus the NIR
//!   image lies (`[lens] nir_defocus_frac`) and how soft the filter's shadow is
//!   (`[lens] filter_edge_blur_frac`), and the camera's reaction downstream: scene
//!   auto-exposure meters the prepared scene, NIR included, and takes the extra light
//!   back within its two stops; daylight white balance turns the red-heavy NIR pink;
//!   grey-world and servo white balance fight the cast. None of these is changed by
//!   the bend; the look emerges from the chain.
//! - An estimate: the NIR radiance of the scene. An RGB photograph records nothing
//!   beyond 700 nm, so the scene's NIR cannot be recovered from it. It is estimated, a
//!   documented guess like highlight reconstruction: `nir_weights` is a mild
//!   assumption (most surfaces reflect NIR about as they reflect red, and clear sky,
//!   blue by Rayleigh scattering, carries little), `foliage_nir` an explicit
//!   assumption about the scene, that green means chlorophyll, and off by default.
//!
//! Real NIR data would enter at one point: a fourth scene plane holding the NIR
//! radiance would replace the estimate of step 1 below, and nothing downstream would
//! change.
//!
//! 1. Estimate, from linear sRGB after highlight reconstruction and vignetting, before
//!    the colour conversion, in units where reference white's visible radiance is 1:
//!    `nir = nir_scale * (nir_weights · [r, g, b] + foliage_nir * greenness * g)`.
//!    The greenness is the green excess `e = (g - max(r, 1.5 b)) / g`, which is at
//!    most 0 unless green exceeds both red and one and a half times blue (never for
//!    grey, blue, red, skin or sky), through a smoothstep: `t = clamp(e / 0.5, 0, 1)`,
//!    `greenness = t^2 (3 - 2 t)`. It reaches 1 where green is twice the larger of red
//!    and 1.5 blue, as on sunlit and shaded leaves (chlorophyll absorbs blue and red,
//!    and the leaf's cells reflect four to six times its green in the NIR: the Wood
//!    effect), and about one half on a yellow-green lawn; the smoothstep keeps faintly
//!    green greys near 0. Blue weighs more than red because leaves lean yellow-green
//!    (their pigments absorb blue more deeply than red) while the commonest
//!    cyan-green, water, absorbs the NIR and must not glow. The greenness ignores
//!    brightness, so the estimate scales with the light and the lens falloff applies
//!    to it as to the visible. A grey card gives `nir_scale` times the weighted sum
//!    of its level; with the default weights, which sum to one, that is `nir_scale`
//!    times its level, and pure blue gives 4 % of white.
//! 2. Defocus: the lens is corrected for visible light, so NIR focuses behind the
//!    sensor. The NIR image is blurred with a Gaussian of standard deviation
//!    `nir_defocus_frac` of the width, over three standard deviations each side, the
//!    edge pixels repeated beyond the frame. The Gaussian also stands for NIR
//!    photo-charge, generated deep in the silicon, spreading to neighbouring sites.
//! 3. Filter position: the filter has slid off the top `ir_cut_pulled_frac` of the
//!    height. It stands out of focus in front of the sensor, so a photosite sees the
//!    lens's exit pupil as a uniformly lit disc in the filter's plane, and the edge
//!    blocks the part of the disc on the filter's side. With the shadow
//!    `w = filter_edge_blur_frac * height` wide and a row's centre `d` below the edge,
//!    `s = 2 d / w`, the uncovered fraction is the area of a circular segment,
//!    `u = 1/2 - (s sqrt(1 - s^2) + asin s) / π` for `|s| < 1`, 1 above the shadow and
//!    0 below it: a smooth step close to a smoothstep of the same width, flat at both
//!    ends. The edge lies at `ir_cut_pulled_frac * (height + w) - w / 2` rows from the
//!    top: it travels from half a shadow above the top to half a shadow below the
//!    bottom, so at 0 the shadow lies wholly above the frame and every row has
//!    exactly `u = 0` (no NIR leaks into the top rows), at 1 every row has exactly
//!    `u = 1`, and at 0.5 the edge crosses the middle. A row with `u = 0` is computed
//!    exactly as without the bend; with the filter in place the stage skips the NIR
//!    altogether. Rows count from the top of the sensor, which is the left of the
//!    upright frame when the camera is held in portrait.
//! 4. Charge: after the colour conversion and the clip at zero, each channel `c` gains
//!    `nir_response[c] * u * nir`, in the prepared scene's units (reference white
//!    gives 1 in green, the channel the white balance leaves alone).
//!
//! Sizes are fractions of the frame, so a match-input render is the native look
//! downscaled. Left out: the deep red (650 to 700 nm) that the hot mirror also
//! attenuates, which without it would lift red a little further; the lens's focus
//! shift within the visible; any wavelength dependence of the vignetting; the
//! aperture's effect on the width of the filter's shadow. The bend has neither memory
//! nor randomness: the filter's position is a knob, and the NIR image is rebuilt every
//! frame in the working planes of [`State`], allocated on the first frame.
//!
//! The output is a [`Scene`] at sensor resolution in linear light, reference white at
//! 1.0 in the brightest channel; [`sensor::expose`](crate::sensor::expose) consumes it.

mod highlight;
pub(crate) mod model;
mod optics;

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use crate::plane::Plane;
use crate::resample::{self, Turn};
use crate::{Camera, Error, Preset, Scene, Timing, colour};

#[doc(inline)]
pub use optics::Optics;

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
    /// The IR-cut filter slid off the sensor; `None` while it covers the whole sensor.
    pub optics: Option<Optics>,
}

impl Params {
    /// Resolves the scene parameters from the camera, preset and derived timing.
    ///
    /// The sensor response always comes from the camera file: its `xyz_to_cam` matrix
    /// and daylight gains. Preset overrides change only what the image processor
    /// assumes, so they show up as colour errors, as on a real camera. The highlight
    /// reconstruction comes from the preset's `[scene]` section, the vignetting from
    /// the camera's `[lens]`. The preset's `[bend.optics]` with the filter pulled at
    /// all, together with the camera's `[sensor] nir_response` and `[lens]` blurs,
    /// gives [`Optics`]; with the filter in place (`ir_cut_pulled_frac = 0`) there is
    /// none and the stage computes exactly as without the bend.
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
            optics: preset
                .bend
                .optics
                .as_ref()
                .and_then(|bend| Optics::resolve(bend, camera)),
        })
    }
}

/// Working memory of scene preparation, reused across frames.
///
/// Holds the planes of the near-infrared image and its defocus kernel, used only with
/// [`Params::optics`]. They carry nothing from one frame to the next, so the stage
/// has no memory: the default, the state after a reset, allocates nothing, and the
/// planes are sized on the first frame with the filter pulled.
#[derive(Debug, Clone)]
pub struct State {
    /// The NIR estimate, then the defocused image; and the horizontal blur in between.
    nir: [Plane<f32>; 2],
    kernel: Vec<f32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            nir: [Plane::new(0, 0), Plane::new(0, 0)],
            kernel: Vec::new(),
        }
    }
}

/// Maps `input` onto the sensor-sized scene `out`, in the sensor's colour primaries.
///
/// The output size is the size of `out`; `input` may have any non-zero size. `state`
/// holds the working planes of the near-infrared image.
///
/// # Panics
///
/// Panics if `input` is empty.
pub fn prepare(params: &Params, state: &mut State, input: &Scene, out: &mut Scene) {
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
    let vignette = Vignette::new(params.vignette_stops, out.width(), out.height());
    match &params.optics {
        None => to_sensor(params, &vignette, out, None),
        Some(optics) => {
            let State { nir, kernel } = state;
            optics.defocused_nir(out, &vignette, nir, kernel);
            to_sensor(params, &vignette, out, Some((optics, &nir[0])));
        }
    }
}

/// The cos^4 falloff of the lens over a sensor of a given size.
#[derive(Debug, Clone, Copy)]
struct Vignette {
    /// `tan^2` of the corner field angle; 0 for an even lens.
    tan_sq: f32,
    /// Centre column in pixels.
    cx: f32,
    /// Centre row in pixels.
    cy: f32,
    /// Inverse of the squared half-diagonal in pixels.
    inv_diagonal_sq: f32,
}

impl Vignette {
    /// Returns the falloff of `stops` at the corners of a `width` by `height` sensor.
    fn new(stops: f32, width: u32, height: u32) -> Self {
        let (cx, cy) = (0.5 * width as f32, 0.5 * height as f32);
        Self {
            tan_sq: 2f32.powf(0.5 * stops) - 1.0,
            cx,
            cy,
            inv_diagonal_sq: 1.0 / cx.mul_add(cx, cy * cy),
        }
    }

    /// Returns the vertical offset of row `y`'s centre from the optical centre.
    fn dy(&self, y: usize) -> f32 {
        y as f32 + 0.5 - self.cy
    }

    /// Returns the relative illumination at column `x` of the row at offset `dy`.
    fn falloff(&self, x: usize, dy: f32) -> f32 {
        if self.tan_sq > 0.0 {
            let dx = x as f32 + 0.5 - self.cx;
            let r_sq = dx.mul_add(dx, dy * dy) * self.inv_diagonal_sq;
            let base = r_sq.mul_add(self.tan_sq, 1.0);
            1.0 / (base * base)
        } else {
            1.0
        }
    }
}

/// Applies the lens, converts linear sRGB to camera RGB and adds the NIR, in place.
///
/// The camera RGB is not white-balanced. Negative visible responses are clipped at zero
/// before the near infrared, the defocused NIR image `nir` weighed by each row's
/// [`Optics::row_gains`], is added.
fn to_sensor(
    params: &Params,
    vignette: &Vignette,
    scene: &mut Scene,
    nir: Option<(&Optics, &Plane<f32>)>,
) {
    let (width, height) = (scene.width() as usize, scene.height());
    if width == 0 {
        return;
    }
    let scale = params.wb_gains.map(|gain| 1.0 / gain);
    let [red, green, blue] = scene.planes_mut();
    red.data_mut()
        .par_chunks_mut(width)
        .zip(green.data_mut().par_chunks_mut(width))
        .zip(blue.data_mut().par_chunks_mut(width))
        .enumerate()
        .for_each(|(y, ((rs, gs), bs))| {
            let dy = vignette.dy(y);
            for (x, ((r, g), b)) in rs
                .iter_mut()
                .zip(gs.iter_mut())
                .zip(bs.iter_mut())
                .enumerate()
            {
                let falloff = vignette.falloff(x, dy);
                let cam = colour::apply(&params.srgb_to_cam, [*r, *g, *b]);
                *r = (cam[0] * scale[0] * falloff).max(0.0);
                *g = (cam[1] * scale[1] * falloff).max(0.0);
                *b = (cam[2] * scale[2] * falloff).max(0.0);
            }
            let leak = nir.and_then(|(optics, plane)| {
                optics
                    .row_gains(y as u32, height)
                    .map(|gains| (gains, plane.row(y as u32)))
            });
            if let Some((gains, nir)) = leak {
                for (channel, gain) in [&mut *rs, &mut *gs, &mut *bs].into_iter().zip(gains) {
                    for (value, &radiance) in channel.iter_mut().zip(nir) {
                        *value = gain.mul_add(radiance, *value);
                    }
                }
            }
        });
}
