//! The IR-cut filter slid off the sensor: the scene's near infrared, estimated, defocused
//! and added to the photosites it reaches.
//!
//! The model and its formulas are documented in the [`scene`](super) module header.

use std::f64::consts::PI;
use std::fmt::{self, Debug, Formatter};
use std::sync::{Mutex, PoisonError};

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use super::Vignette;
use crate::plane::Plane;
use crate::preset::OpticsBend;
use crate::{Camera, Scene};

/// Green excess `(g - max(r, 1.5 b)) / g` at which a pixel counts fully as foliage.
///
/// Green leaves reflect two to three times more green than red or blue (chlorophyll
/// absorbs at 430 and 660 nm), an excess of 0.5 to 0.67; foliage in sRGB photographs,
/// where the channels overlap, measures 0.3 to 0.65. At 0.5, green twice the larger
/// of red and 1.5 blue, sunlit and shaded leaves count fully, a yellow-green lawn
/// about half. PROVISIONAL: a choice, like the rest of the estimate.
const LEAF_EXCESS: f32 = 0.5;

/// Weight of blue against red in the green excess.
///
/// Chlorophyll and the carotenoids beside it absorb blue more deeply than red, so
/// leaves lean yellow-green, red above blue. Cyan-greens lean the other way, and the
/// commonest, water over a pale bottom, absorbs the near infrared: at 1.5 a turquoise
/// lake measured in `samples/sample-01.jpg` (green excess 0.14 with blue weighed as red)
/// counts as no foliage, while its mullein leaves keep their 0.7. PROVISIONAL.
const LEAF_BLUE_WEIGHT: f32 = 1.5;

/// Half-width of the defocus kernel in standard deviations; the tails beyond hold 0.3 %.
const KERNEL_SIGMAS: f32 = 3.0;

/// The IR-cut filter's position and the scene's estimated near infrared, resolved.
///
/// Built by [`Params::resolve`](super::Params::resolve) from the preset's
/// `[bend.optics]` and the camera's `[sensor] nir_response` and `[lens]` blurs; absent
/// while the filter covers the whole sensor. The model is described in the
/// [`scene`](super) module header.
#[derive(Debug, Clone, PartialEq)]
pub struct Optics {
    pulled_frac: f64,
    edge_blur_frac: f64,
    defocus_frac: f32,
    nir_weights: [f32; 3],
    foliage_nir: f32,
    nir_scale: f32,
    nir_response: [f32; 3],
    scratch: Scratch,
}

impl Optics {
    /// Resolves the bend for the camera; `None` while the filter covers the whole sensor.
    pub(super) fn resolve(bend: &OpticsBend, camera: &Camera) -> Option<Self> {
        (bend.ir_cut_pulled_frac > 0.0).then(|| Self {
            pulled_frac: bend.ir_cut_pulled_frac,
            edge_blur_frac: camera.lens.filter_edge_blur_frac,
            defocus_frac: camera.lens.nir_defocus_frac as f32,
            nir_weights: bend.nir_weights.map(|w| w as f32),
            foliage_nir: bend.foliage_nir as f32,
            nir_scale: bend.nir_scale as f32,
            nir_response: camera.sensor.nir_response.map(|k| k as f32),
            scratch: Scratch::default(),
        })
    }

    /// Returns the estimated NIR radiance of one linear sRGB pixel; 1 is reference white's.
    ///
    /// `nir_scale * (nir_weights · rgb + foliage_nir * greenness * g)`, with negative
    /// channels taken as zero. The greenness is 0 for any pixel whose green does not
    /// exceed both red and one and a half times blue (greys, blues, reds, skin, sky,
    /// cyan water) and rises smoothly to 1 where green is twice the larger of the two
    /// (leaves); see the [`scene`](super) module header. Scaling `rgb` scales the
    /// result.
    #[must_use]
    pub fn nir(&self, rgb: [f32; 3]) -> f32 {
        let [r, g, b] = rgb.map(|v| v.max(0.0));
        let w = self.nir_weights;
        let weighted = w[0].mul_add(r, w[1].mul_add(g, w[2] * b));
        let foliage = if self.foliage_nir > 0.0 {
            self.foliage_nir * greenness(r, g, b) * g
        } else {
            0.0
        };
        self.nir_scale * (weighted + foliage)
    }

    /// Returns the fraction of the light on row `row` of `rows` that bypasses the filter.
    ///
    /// 1 above the filter's edge, 0 below it, and between the two across the edge's
    /// shadow, which is `filter_edge_blur_frac` of the height wide; exactly 1 on every
    /// row with the filter removed. Rows count from the top of the sensor.
    #[must_use]
    pub fn uncovered_frac(&self, row: u32, rows: u32) -> f32 {
        let height = f64::from(rows);
        let shadow_px = self.edge_blur_frac * height;
        // The edge travels from half its shadow above the top to half below the bottom.
        let edge_px = self
            .pulled_frac
            .mul_add(height + shadow_px, -0.5 * shadow_px);
        let below_px = f64::from(row) + 0.5 - edge_px;
        if shadow_px <= 0.0 {
            return if below_px < 0.0 { 1.0 } else { 0.0 };
        }
        let s = below_px / (0.5 * shadow_px);
        if s <= -1.0 {
            1.0
        } else if s >= 1.0 {
            0.0
        } else {
            // The part of the pupil's disc on the filter's side of the edge.
            let blocked = 0.5 + s.mul_add((1.0 - s * s).sqrt(), s.asin()) / PI;
            (1.0 - blocked) as f32
        }
    }

    /// Returns the charge per unit NIR that row `row` of `rows` adds to each channel.
    ///
    /// `nir_response` times the row's uncovered fraction; `None` where the filter covers
    /// the row entirely, so a covered row is computed exactly as without the bend.
    pub(super) fn row_gains(&self, row: u32, rows: u32) -> Option<[f32; 3]> {
        let uncovered = self.uncovered_frac(row, rows);
        (uncovered > 0.0).then(|| self.nir_response.map(|k| k * uncovered))
    }

    /// Computes the defocused NIR image of `scene`, then calls `add` with both.
    ///
    /// `scene` is linear sRGB before the lens and `vignette` the falloff applied to
    /// it; the image lives in the working planes, allocated on the first frame.
    pub(super) fn with_defocused_nir(
        &self,
        scene: &mut Scene,
        vignette: &Vignette,
        add: impl FnOnce(&mut Scene, &Plane<f32>),
    ) {
        let mut scratch = self
            .scratch
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Planes { planes, kernel } = &mut *scratch;
        self.defocused_nir(scene, vignette, planes, kernel);
        add(scene, &planes[0]);
    }

    /// Writes the defocused NIR image of the linear sRGB `scene` into `nir`.
    ///
    /// `nir` and `tmp` are fitted to the scene; `kernel` holds the defocus kernel.
    fn defocused_nir(
        &self,
        scene: &Scene,
        vignette: &Vignette,
        [nir, tmp]: &mut [Plane<f32>; 2],
        kernel: &mut Vec<f32>,
    ) {
        let (width, height) = (scene.width(), scene.height());
        fit(nir, width, height);
        fit(tmp, width, height);
        let width = width as usize;
        let sigma_px = self.defocus_frac * width as f32;
        let blur = sigma_px > 0.0;
        if blur {
            gaussian_kernel(sigma_px, kernel);
        }
        let [red, green, blue] = scene.planes();
        let kernel = &*kernel;
        nir.data_mut()
            .par_chunks_mut(width)
            .zip(tmp.data_mut().par_chunks_mut(width))
            .enumerate()
            .for_each(|(y, (estimate, across))| {
                let dy = vignette.dy(y);
                let rgb = [red, green, blue].map(|plane| plane.row(y as u32));
                for (x, value) in estimate.iter_mut().enumerate() {
                    let pixel = [rgb[0][x], rgb[1][x], rgb[2][x]];
                    *value = self.nir(pixel) * vignette.falloff(x, dy);
                }
                if blur {
                    horizontal(estimate, across, kernel);
                }
            });
        if blur {
            vertical(tmp, nir, kernel);
        }
    }
}

/// Returns how chlorophyll-like a linear sRGB colour is, from 0 (not at all) to 1 (a leaf).
///
/// The green excess `e = (g - max(r, LEAF_BLUE_WEIGHT b)) / g` through a smoothstep that
/// reaches 1 at [`LEAF_EXCESS`]: `t = clamp(e / LEAF_EXCESS, 0, 1)`, `t^2 (3 - 2 t)`.
fn greenness(r: f32, g: f32, b: f32) -> f32 {
    if g <= 0.0 {
        return 0.0;
    }
    let t = ((g - r.max(LEAF_BLUE_WEIGHT * b)) / (g * LEAF_EXCESS)).clamp(0.0, 1.0);
    t * t * 2.0f32.mul_add(-t, 3.0)
}

/// The working planes of the NIR image and the defocus kernel.
#[derive(Debug)]
struct Planes {
    /// The estimate, then the defocused image; and the horizontal blur in between.
    planes: [Plane<f32>; 2],
    kernel: Vec<f32>,
}

/// Working buffers of the NIR image, allocated on the first frame and reused.
///
/// They carry nothing from one frame to the next, so a clone starts empty and any two
/// compare equal. The lock lets [`prepare`](super::prepare) take the parameters by
/// shared reference.
struct Scratch(Mutex<Planes>);

impl Default for Scratch {
    fn default() -> Self {
        Self(Mutex::new(Planes {
            planes: [Plane::new(0, 0), Plane::new(0, 0)],
            kernel: Vec::new(),
        }))
    }
}

impl Clone for Scratch {
    /// Returns empty buffers: the working planes hold no parameter.
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for Scratch {
    /// Returns true: the working planes hold no parameter.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Debug for Scratch {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("Scratch")
    }
}

/// Reallocates `plane` if it is not `width` by `height`.
fn fit(plane: &mut Plane<f32>, width: u32, height: u32) {
    if plane.width() != width || plane.height() != height {
        *plane = Plane::new(width, height);
    }
}

/// Fills `kernel` with the one-sided weights of a normalised Gaussian of `sigma` pixels.
///
/// `kernel[0]` is the centre weight and `kernel[k]` the weight at offsets `-k` and `+k`;
/// the full kernel sums to one.
fn gaussian_kernel(sigma: f32, kernel: &mut Vec<f32>) {
    let radius = (KERNEL_SIGMAS * sigma).ceil().max(1.0) as usize;
    let scale = -0.5 / (sigma * sigma);
    kernel.clear();
    kernel.extend((0..=radius).map(|k| (scale * (k * k) as f32).exp()));
    let total = kernel[0] + 2.0 * kernel[1..].iter().sum::<f32>();
    for weight in kernel.iter_mut() {
        *weight /= total;
    }
}

/// Blurs one row with `kernel` into `out`, the row's ends repeated beyond its edges.
fn horizontal(row: &[f32], out: &mut [f32], kernel: &[f32]) {
    let width = row.len();
    let radius = kernel.len() - 1;
    let last = width - 1;
    let clamped = |x: usize| {
        let mut acc = kernel[0] * row[x];
        for (k, &weight) in kernel.iter().enumerate().skip(1) {
            acc += weight * (row[x.saturating_sub(k)] + row[(x + k).min(last)]);
        }
        acc
    };
    if width <= 2 * radius {
        for (x, value) in out.iter_mut().enumerate() {
            *value = clamped(x);
        }
        return;
    }
    // The interior, one kernel tap at a time over contiguous slices.
    let inner = radius..width - radius;
    for (value, &sample) in out[inner.clone()].iter_mut().zip(&row[inner.clone()]) {
        *value = kernel[0] * sample;
    }
    for (k, &weight) in kernel.iter().enumerate().skip(1) {
        let left = &row[inner.start - k..inner.end - k];
        let right = &row[inner.start + k..inner.end + k];
        for ((value, &l), &r) in out[inner.clone()].iter_mut().zip(left).zip(right) {
            *value += weight * (l + r);
        }
    }
    for x in (0..radius).chain(inner.end..width) {
        out[x] = clamped(x);
    }
}

/// Blurs `src` down its columns with `kernel` into `dst`, the end rows repeated beyond.
fn vertical(src: &Plane<f32>, dst: &mut Plane<f32>, kernel: &[f32]) {
    let width = src.width() as usize;
    let last = src.height() as usize - 1;
    dst.data_mut()
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, out)| {
            for (value, &sample) in out.iter_mut().zip(src.row(y as u32)) {
                *value = kernel[0] * sample;
            }
            for (k, &weight) in kernel.iter().enumerate().skip(1) {
                let above = src.row(y.saturating_sub(k) as u32);
                let below = src.row((y + k).min(last) as u32);
                for ((value, &a), &b) in out.iter_mut().zip(above).zip(below) {
                    *value += weight * (a + b);
                }
            }
        });
}
