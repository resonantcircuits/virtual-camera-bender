//! Image processing: from ADC codes to a developed 8-bit frame.
//!
//! The image processor (Canon's DIGIC in the A520) receives the ADC words line by
//! line. [`clamp_black`] measures the black level as the mean of the optical-black
//! samples of the frame and subtracts it, leaving a [`Raw`] Bayer plane of the active
//! area in code units above black, rows in image order. [`develop`] then turns it into
//! a [`Frame`]:
//!
//! 1. Normalisation: codes are divided by `white_code`, the code above black that
//!    reference white produces, so linear 1.0 is reference white again.
//! 2. White balance: the red and blue sites are multiplied by the daylight gains, or
//!    by grey-world gains that equalise the channel means ([`Wb::Auto`]).
//! 3. Demosaic: bilinear interpolation of the two missing channels at every site,
//!    mirrored at the borders with the Bayer phase preserved.
//! 4. Colour correction: a 3 by 3 matrix from sensor RGB to linear sRGB.
//! 5. Geometry: movie mode averages `h_bin` adjacent columns, and if the result
//!    differs from the output size it is resampled to it.
//! 6. Encoding: the sRGB transfer curve, clipped and rounded to 8 bits.
//!
//! The tone curve, noise reduction and sharpening values in [`Params`] are not applied
//! by this pipeline; it reproduces the scene's colours up to quantisation and
//! demosaicing error.

use std::fmt::{self, Debug, Formatter};

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use crate::adc::Codes;
use crate::plane::Plane;
use crate::preset::Mode;
use crate::resample::{self, Rect};
use crate::scene::Channel;
use crate::sensor::Cfa;
use crate::srgb;
use crate::{Camera, Error, Frame, Preset, Scene, Timing};

/// Which white balance the processor applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum WbMode {
    /// Fixed daylight gains.
    #[default]
    Daylight,
    /// Gains measured from the frame (grey world).
    Auto,
}

/// White balance with its resolved gains.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wb {
    /// Fixed gains of the red and blue channels relative to green.
    Daylight {
        /// Red gain.
        r_gain: f32,
        /// Blue gain.
        b_gain: f32,
    },
    /// Gains that make the mean of every channel equal to the green mean.
    Auto,
}

/// The tone curve applied to luminance before encoding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tone {
    /// Slope at mid-grey.
    pub contrast: f32,
    /// Lift in the shadows.
    pub toe: f32,
    /// Compression in the highlights.
    pub shoulder: f32,
}

/// Unsharp masking of luminance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sharpen {
    /// Blur radius as a fraction of the frame width.
    pub radius_frac: f32,
    /// Fraction of the high-pass detail added back.
    pub gain: f32,
}

/// Demosaicing algorithm.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Demosaic {
    /// Bilinear interpolation of each channel from its nearest sites.
    #[default]
    Bilinear,
}

/// Size of the developed frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Output {
    /// Adjacent columns averaged into one, 1 for stills.
    pub h_bin: u32,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
}

/// The active-area Bayer plane after the black clamp, in codes above black.
#[derive(Clone, PartialEq)]
pub struct Raw {
    plane: Plane<f32>,
    black_level: f32,
}

impl Raw {
    /// Creates a zero raw plane of `width` by `height` sites.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            plane: Plane::new(width, height),
            black_level: 0.0,
        }
    }

    /// Returns the sites in codes above black, rows in image order, red at `(0, 0)`.
    #[must_use]
    pub fn plane(&self) -> &Plane<f32> {
        &self.plane
    }

    /// Returns the sites mutably.
    pub fn plane_mut(&mut self) -> &mut Plane<f32> {
        &mut self.plane
    }

    /// Returns the black level the clamp subtracted, in ADC codes.
    #[must_use]
    pub fn black_level(&self) -> f32 {
        self.black_level
    }
}

impl Debug for Raw {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Raw")
            .field("width", &self.plane.width())
            .field("height", &self.plane.height())
            .field("black_level", &self.black_level)
            .finish_non_exhaustive()
    }
}

/// Parameters of the processing pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Code above black that reference white produces; divides every site.
    pub white_code: f32,
    /// Colour filter array of the raw plane.
    pub cfa: Cfa,
    /// White balance.
    pub wb: Wb,
    /// Colour correction matrix from sensor RGB to linear sRGB, row major.
    pub ccm: [[f32; 3]; 3],
    /// Tone curve.
    pub tone: Tone,
    /// Chroma noise reduction radius as a fraction of the frame width.
    pub chroma_nr_frac: f32,
    /// Luma noise reduction radius as a fraction of the frame width.
    pub luma_nr_frac: f32,
    /// Unsharp masking.
    pub sharpen: Sharpen,
    /// Demosaicing algorithm.
    pub demosaic: Demosaic,
    /// Auto-exposure target luminance.
    pub ae_target: f32,
    /// Size of the developed frame.
    pub output: Output,
}

impl Params {
    /// Resolves the processing parameters from the camera, preset and derived timing.
    ///
    /// `white_code` follows from the analog chain: reference white fills
    /// `white_fraction` of the full well, which the conversion gain, PGA gain and ADC
    /// scale map to a code above the pedestal. Preset overrides replace camera values.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if the analog chain maps reference white to no code.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let sensor = &camera.sensor;
        let white_volts = sensor.white_fraction
            * sensor.full_well_e
            * sensor.conv_gain_uv_per_e
            * 1e-6
            * 10f64.powf(camera.afe.pga_gain_db / 20.0);
        let white_code = white_volts / camera.adc.full_scale_v * f64::from(camera.max_code());
        if !(white_code.is_finite() && white_code >= 1.0) {
            return Err(Error::config(format!(
                "reference white reaches only {white_code:.3} ADC codes above black"
            )));
        }

        let isp = &camera.isp;
        let o = preset.isp.clone().unwrap_or_default();
        let pick = |value: Option<f64>, default: f64| value.unwrap_or(default) as f32;
        let wb = match o.wb.unwrap_or_default() {
            WbMode::Daylight => Wb::Daylight {
                r_gain: pick(o.wb_r_gain, isp.wb_r_gain),
                b_gain: pick(o.wb_b_gain, isp.wb_b_gain),
            },
            WbMode::Auto => Wb::Auto,
        };
        let ccm = o
            .ccm
            .unwrap_or(isp.ccm)
            .map(|row| row.map(|value| value as f32));
        let h_bin = match timing.mode {
            Mode::Still => 1,
            Mode::Movie => camera.movie.h_bin,
        };
        Ok(Self {
            white_code: white_code as f32,
            cfa: sensor.cfa,
            wb,
            ccm,
            tone: Tone {
                contrast: pick(o.tone_contrast, isp.tone_contrast),
                toe: pick(o.tone_toe, isp.tone_toe),
                shoulder: pick(o.tone_shoulder, isp.tone_shoulder),
            },
            chroma_nr_frac: pick(o.chroma_nr_frac, isp.chroma_nr_frac),
            luma_nr_frac: pick(o.luma_nr_frac, isp.luma_nr_frac),
            sharpen: Sharpen {
                radius_frac: pick(o.sharpen_radius_frac, isp.sharpen_radius_frac),
                gain: pick(o.sharpen_gain, isp.sharpen_gain),
            },
            demosaic: Demosaic::Bilinear,
            ae_target: pick(o.ae_target, isp.ae_target),
            output: Output {
                h_bin,
                width: timing.output_width,
                height: timing.output_height,
            },
        })
    }
}

/// Persistent state of the image processor: working planes reused across frames.
#[derive(Debug, Clone)]
pub struct State {
    rgb: Scene,
    binned: Scene,
    sized: Scene,
}

impl Default for State {
    fn default() -> Self {
        Self {
            rgb: Scene::new(0, 0),
            binned: Scene::new(0, 0),
            sized: Scene::new(0, 0),
        }
    }
}

/// Subtracts the frame's optical-black mean from the active samples of `codes`.
///
/// Writes the active area into `out` with rows in image order and records the
/// subtracted level as [`Raw::black_level`].
///
/// # Panics
///
/// Panics if `out` is not `cols` by `rows` of the code layout.
pub fn clamp_black(codes: &Codes, out: &mut Raw) {
    let layout = codes.layout();
    assert!(
        out.plane.width() == layout.cols() && out.plane.height() == layout.rows(),
        "raw plane is {} x {} but the readout is {} x {}",
        out.plane.width(),
        out.plane.height(),
        layout.cols(),
        layout.rows()
    );
    let line_len = layout.line_len() as usize;
    let ob = layout.ob_cols() as usize;
    let black_sum: u64 = codes
        .words()
        .chunks_exact(line_len)
        .flat_map(|line| &line[..ob])
        .map(|&code| u64::from(code))
        .sum();
    let black_count = ob * layout.rows() as usize;
    let black = if black_count == 0 {
        0.0
    } else {
        (black_sum as f64 / black_count as f64) as f32
    };
    for (line, words) in (0..layout.rows()).zip(codes.words().chunks_exact(line_len)) {
        let row = out.plane.row_mut(layout.image_row(line));
        for (site, &code) in row.iter_mut().zip(&words[ob..]) {
            *site = f32::from(code) - black;
        }
    }
    out.black_level = black;
}

/// Develops the raw plane into the 8-bit frame `out`.
///
/// # Panics
///
/// Panics if `out` is not the output size in `params`, or if the raw plane is
/// smaller than 2 by 2 sites.
pub fn develop(params: &Params, state: &mut State, raw: &Raw, out: &mut Frame) {
    let output = params.output;
    assert!(
        out.width() == output.width && out.height() == output.height,
        "frame is {} x {} but the output is {} x {}",
        out.width(),
        out.height(),
        output.width,
        output.height
    );
    let (width, height) = (raw.plane.width(), raw.plane.height());
    assert!(
        width >= 2 && height >= 2,
        "cannot demosaic a {width} x {height} raw plane"
    );
    let State { rgb, binned, sized } = state;

    let balance = match params.wb {
        Wb::Daylight { r_gain, b_gain } => [r_gain, 1.0, b_gain],
        Wb::Auto => grey_world(&raw.plane, params.cfa),
    };
    let gains = balance.map(|gain| gain / params.white_code);
    fit(rgb, width, height);
    match params.demosaic {
        Demosaic::Bilinear => demosaic_bilinear(&raw.plane, params.cfa, gains, rgb),
    }
    correct_colour(&params.ccm, rgb);

    let h_bin = output.h_bin.max(1);
    let binned: &Scene = if h_bin > 1 {
        fit(binned, width / h_bin, height);
        for (src, dst) in rgb.planes().into_iter().zip(binned.planes_mut()) {
            resample::bin_columns(src, h_bin, dst);
        }
        binned
    } else {
        rgb
    };
    let sized: &Scene = if (binned.width(), binned.height()) == (output.width, output.height) {
        binned
    } else {
        fit(sized, output.width, output.height);
        let whole = Rect {
            x: 0,
            y: 0,
            width: binned.width(),
            height: binned.height(),
        };
        for (src, dst) in binned.planes().into_iter().zip(sized.planes_mut()) {
            resample::resample(src, whole, dst);
        }
        sized
    };
    encode(sized, out);
}

/// Reallocates `scene` if it is not `width` by `height`.
fn fit(scene: &mut Scene, width: u32, height: u32) {
    if scene.width() != width || scene.height() != height {
        *scene = Scene::new(width, height);
    }
}

/// Returns `[r, g, b]` gains that equalise the red and blue means with green.
fn grey_world(raw: &Plane<f32>, cfa: Cfa) -> [f32; 3] {
    let mut sums = [0.0f64; 3];
    let mut counts = [0u64; 3];
    for (y, row) in (0..raw.height()).zip(raw.rows()) {
        for (x, &value) in (0..raw.width()).zip(row) {
            let channel = cfa.channel(x, y).index();
            sums[channel] += f64::from(value);
            counts[channel] += 1;
        }
    }
    let means: [f64; 3] = std::array::from_fn(|c| sums[c] / counts[c].max(1) as f64);
    let gain = |mean: f64| {
        if mean > 0.0 && means[1] > 0.0 {
            (means[1] / mean) as f32
        } else {
            1.0
        }
    };
    [gain(means[0]), 1.0, gain(means[2])]
}

/// Mirrors index `i` into `[0, n)` with period-two symmetry, preserving the Bayer phase.
fn reflect(i: i64, n: u32) -> usize {
    let n = i64::from(n);
    let mirrored = if i < 0 {
        -i
    } else if i >= n {
        2 * (n - 1) - i
    } else {
        i
    };
    mirrored as usize
}

/// Interpolates the full RGB image from the Bayer plane, scaling each channel by `gains`.
fn demosaic_bilinear(raw: &Plane<f32>, cfa: Cfa, gains: [f32; 3], out: &mut Scene) {
    let (width, height) = (raw.width(), raw.height());
    let [red, green, blue] = out.planes_mut();
    let row_len = width as usize;
    red.data_mut()
        .par_chunks_mut(row_len)
        .zip(green.data_mut().par_chunks_mut(row_len))
        .zip(blue.data_mut().par_chunks_mut(row_len))
        .enumerate()
        .for_each(|(y, ((red_row, green_row), blue_row))| {
            let y = y as u32;
            let up = raw.row(reflect(i64::from(y) - 1, height) as u32);
            let mid = raw.row(y);
            let down = raw.row(reflect(i64::from(y) + 1, height) as u32);
            for x in 0..width {
                let c = x as usize;
                let left = reflect(i64::from(x) - 1, width);
                let right = reflect(i64::from(x) + 1, width);
                let here = mid[c];
                let cross = 0.25 * (up[c] + down[c] + mid[left] + mid[right]);
                let diagonal = 0.25 * (up[left] + up[right] + down[left] + down[right]);
                let across = 0.5 * (mid[left] + mid[right]);
                let vertical = 0.5 * (up[c] + down[c]);
                let rgb = match cfa.channel(x, y) {
                    Channel::Red => [here, cross, diagonal],
                    Channel::Blue => [diagonal, cross, here],
                    Channel::Green if cfa.channel(x ^ 1, y) == Channel::Red => {
                        [across, here, vertical]
                    }
                    Channel::Green => [vertical, here, across],
                };
                red_row[c] = rgb[0] * gains[0];
                green_row[c] = rgb[1] * gains[1];
                blue_row[c] = rgb[2] * gains[2];
            }
        });
}

/// Multiplies every pixel by the colour correction matrix.
fn correct_colour(ccm: &[[f32; 3]; 3], image: &mut Scene) {
    let row_len = image.width().max(1) as usize;
    let [red, green, blue] = image.planes_mut();
    red.data_mut()
        .par_chunks_mut(row_len)
        .zip(green.data_mut().par_chunks_mut(row_len))
        .zip(blue.data_mut().par_chunks_mut(row_len))
        .for_each(|((red_row, green_row), blue_row)| {
            for ((r, g), b) in red_row.iter_mut().zip(green_row).zip(blue_row) {
                let input = [*r, *g, *b];
                let [out_r, out_g, out_b] = ccm.map(|row| {
                    row[0].mul_add(input[0], row[1].mul_add(input[1], row[2] * input[2]))
                });
                (*r, *g, *b) = (out_r, out_g, out_b);
            }
        });
}

/// Encodes a linear-light image into the interleaved 8-bit sRGB frame.
fn encode(image: &Scene, out: &mut Frame) {
    let width = out.width() as usize;
    if width == 0 {
        return;
    }
    let [red, green, blue] = image.planes();
    out.rgb_mut()
        .par_chunks_mut(width * 3)
        .enumerate()
        .for_each(|(y, bytes)| {
            let y = y as u32;
            let rows = [red.row(y), green.row(y), blue.row(y)];
            for (x, pixel) in bytes.chunks_exact_mut(3).enumerate() {
                for (byte, row) in pixel.iter_mut().zip(rows) {
                    *byte = srgb::encode8(row[x]);
                }
            }
        });
}
