//! Highlight reconstruction: scene radiance above the input's white.
//!
//! An 8-bit input was tone-mapped by the camera that took it: a shoulder rolled its
//! highlights off towards white and everything brighter clipped at code 255. This
//! module undoes that as far as the data allows, on linear-light sRGB.
//!
//! **Shoulder.** The forward shoulder assumed for the source camera is the identity
//! below `shoulder_start = s` and above it `S(x) = s + (1 - s) f((x - s) / (1 - s))`
//! with the extended Reinhard curve `f(t) = t (1 + t / L^2) / (1 + t)`, where
//! `L = (headroom - s) / (1 - s)`, so that `S(headroom) = 1`. `f` has slope 1 at 0 and
//! rises monotonically, so the inverse
//! `E(v) = s + (1 - s) * 2y / (1 - y + sqrt((1 - y)^2 + 4y / L^2))`,
//! with `y = (v - s) / (1 - s)`, is C1 at `s`, monotonic, and maps 1 to `headroom`.
//! Values above 1 (from linear inputs with their own headroom) scale by `headroom`.
//!
//! **Coherent expansion.** Applying `E` to each channel separately would shift hues:
//! an orange whose red channel clipped would turn red. Instead the gain `E(m) / m` of
//! the pixel's brightest channel `m` multiplies all three channels, which keeps every
//! colour's channel ratios and is continuous across the edge of a clipped area.
//!
//! **Clipped channels.** A channel at 254/255 or above in the source is clipped: its
//! true value is unknown but at least white. For a pixel where some channels clipped
//! and others did not, the clipped ones are estimated from the colour of the
//! unclipped highlights nearby: the mean colour `n` of pixels in its neighbourhood
//! that lie above `shoulder_start` without clipping is scaled to match the pixel's
//! unclipped channels, `k = sum(v_u) / sum(n_u)`, and a clipped channel becomes
//! `max(v_c, k n_c)`. A clipped sky next to an unclipped one keeps its blue that way.
//! The neighbourhood statistics live on a 16 by 16 grid over the frame: per cell, the
//! summed colour and the count of unclipped highlight pixels, spread over the grid by
//! a Gaussian of two cells' standard deviation (so the estimate inside a large clipped
//! area comes from the nearest unclipped highlights and varies smoothly), then
//! interpolated bilinearly between cell centres. Where every channel clipped, or the
//! frame holds no unclipped highlight, the clipped channels stay at white and the
//! pixel reaches `headroom`.
//!
//! The method is a guess about lost data (the source camera's curve is unknown).

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use crate::{Scene, srgb};

/// Cells of the neighbourhood grid along each axis.
///
/// Sixteen cells give neighbourhoods of about 140 photosites on the A520's 2272-wide
/// sensor, large enough to hold unclipped sky around a clipped patch.
const GRID: usize = 16;

/// Lowest 8-bit code of a clipped channel: the source camera saturated it.
const CLIP_CODE: f32 = 254.0;

/// Standard deviation, in cells, of the spread of highlight statistics over the grid.
///
/// Two cells reach the nearest unclipped highlights from inside a clipped patch of a
/// quarter of the frame, while a clipped area still takes its colour mostly from the
/// cells next to it.
const SPREAD_CELLS: f64 = 2.0;

/// The inverse of the source camera's highlight shoulder.
#[derive(Debug, Clone, Copy)]
pub(super) struct Shoulder {
    start: f32,
    span: f32,
    limit_sq: f32,
    headroom: f32,
}

impl Shoulder {
    /// Creates the inverse shoulder mapping `start` to itself and 1 to `headroom`.
    pub(super) fn new(headroom: f32, start: f32) -> Self {
        let span = 1.0 - start;
        let limit = (headroom - start) / span;
        Self {
            start,
            span,
            limit_sq: limit * limit,
            headroom,
        }
    }

    /// Returns the scene level of input level `v`.
    pub(super) fn expand(&self, v: f32) -> f32 {
        if v <= self.start {
            v
        } else if v >= 1.0 {
            v * self.headroom
        } else {
            let y = (v - self.start) / self.span;
            let rest = 1.0 - y;
            let t = 2.0 * y / (rest + rest.mul_add(rest, 4.0 * y / self.limit_sq).sqrt());
            self.span.mul_add(t, self.start)
        }
    }
}

/// Per cell: summed `r, g, b` of unclipped highlight pixels, and their count.
type Grid = [[f64; 4]; GRID * GRID];

/// Expands the highlights of a linear-light sRGB scene in place.
pub(super) fn reconstruct(shoulder: &Shoulder, scene: &mut Scene) {
    let (width, height) = (scene.width() as usize, scene.height() as usize);
    if width == 0 || height == 0 {
        return;
    }
    let clip = srgb::decode(CLIP_CODE / 255.0);
    let grid = spread(&neighbourhood(scene, shoulder.start, clip));
    let [red, green, blue] = scene.planes_mut();
    red.data_mut()
        .par_chunks_mut(width)
        .zip(green.data_mut().par_chunks_mut(width))
        .zip(blue.data_mut().par_chunks_mut(width))
        .enumerate()
        .for_each(|(y, ((rs, gs), bs))| {
            for (x, ((r, g), b)) in rs.iter_mut().zip(gs).zip(bs).enumerate() {
                let mut rgb = [*r, *g, *b];
                if rgb.iter().all(|&v| v <= shoulder.start) {
                    continue;
                }
                let clipped = rgb.map(|v| v >= clip);
                if clipped.contains(&true)
                    && clipped.contains(&false)
                    && let Some(colour) = sample(&grid, x, y, width, height)
                {
                    estimate_clipped(&mut rgb, clipped, colour);
                }
                let brightest = rgb.iter().copied().fold(0.0f32, f32::max);
                let gain = shoulder.expand(brightest) / brightest;
                [*r, *g, *b] = rgb.map(|v| v * gain);
            }
        });
}

/// Raises the clipped channels of `rgb` to the neighbourhood colour scaled to the unclipped ones.
fn estimate_clipped(rgb: &mut [f32; 3], clipped: [bool; 3], colour: [f32; 3]) {
    let (mut own, mut near) = (0.0, 0.0);
    for c in 0..3 {
        if !clipped[c] {
            own += rgb[c];
            near += colour[c];
        }
    }
    if near <= 0.0 {
        return;
    }
    let k = own / near;
    for c in 0..3 {
        if clipped[c] {
            rgb[c] = rgb[c].max(k * colour[c]);
        }
    }
}

/// Accumulates the unclipped highlight pixels of every grid cell.
fn neighbourhood(scene: &Scene, start: f32, clip: f32) -> Grid {
    let (width, height) = (scene.width() as usize, scene.height() as usize);
    let planes = scene.planes();
    let mut grid: Grid = [[0.0; 4]; GRID * GRID];
    grid.par_chunks_mut(GRID)
        .enumerate()
        .for_each(|(cell_y, cells)| {
            let rows = (cell_y * height).div_ceil(GRID)..((cell_y + 1) * height).div_ceil(GRID);
            for y in rows {
                let channels = planes.map(|plane| plane.row(y as u32));
                for x in 0..width {
                    let rgb = [channels[0][x], channels[1][x], channels[2][x]];
                    let brightest = rgb.iter().copied().fold(0.0f32, f32::max);
                    if brightest > start && rgb.iter().all(|&v| v < clip) {
                        let cell = &mut cells[x * GRID / width];
                        for (sum, value) in cell.iter_mut().zip(rgb) {
                            *sum += f64::from(value);
                        }
                        cell[3] += 1.0;
                    }
                }
            }
        });
    grid
}

/// Blurs the grid statistics with a Gaussian of [`SPREAD_CELLS`], separably.
///
/// Weights never reach zero within the grid, so every cell receives some colour as
/// soon as the frame holds one unclipped highlight.
fn spread(grid: &Grid) -> Grid {
    let weights: [f64; GRID] =
        std::array::from_fn(|d| (-((d * d) as f64) / (2.0 * SPREAD_CELLS * SPREAD_CELLS)).exp());
    let mut across: Grid = [[0.0; 4]; GRID * GRID];
    for y in 0..GRID {
        for x in 0..GRID {
            let out = &mut across[y * GRID + x];
            for u in 0..GRID {
                let weight = weights[x.abs_diff(u)];
                for (o, &v) in out.iter_mut().zip(&grid[y * GRID + u]) {
                    *o += weight * v;
                }
            }
        }
    }
    let mut both: Grid = [[0.0; 4]; GRID * GRID];
    for y in 0..GRID {
        for x in 0..GRID {
            let out = &mut both[y * GRID + x];
            for v in 0..GRID {
                let weight = weights[y.abs_diff(v)];
                for (o, &value) in out.iter_mut().zip(&across[v * GRID + x]) {
                    *o += weight * value;
                }
            }
        }
    }
    both
}

/// Returns the mean colour of the unclipped highlights around pixel `(x, y)`, if any.
fn sample(grid: &Grid, x: usize, y: usize, width: usize, height: usize) -> Option<[f32; 3]> {
    let axis = |i: usize, len: usize| {
        let pos = ((i as f32 + 0.5) * GRID as f32 / len as f32 - 0.5).clamp(0.0, (GRID - 1) as f32);
        let lo = pos as usize;
        (lo, (lo + 1).min(GRID - 1), pos - lo as f32)
    };
    let (x0, x1, fx) = axis(x, width);
    let (y0, y1, fy) = axis(y, height);
    let mut total = [0.0f64; 4];
    for (cy, wy) in [(y0, 1.0 - fy), (y1, fy)] {
        for (cx, wx) in [(x0, 1.0 - fx), (x1, fx)] {
            let weight = f64::from(wy * wx);
            for (t, &v) in total.iter_mut().zip(&grid[cy * GRID + cx]) {
                *t += weight * v;
            }
        }
    }
    (total[3] > 0.0).then(|| std::array::from_fn(|c| (total[c] / total[3]) as f32))
}
