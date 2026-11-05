//! Cropping and separable resampling of planes.
//!
//! Each axis independently copies (equal size), area-averages (downscale: every output
//! sample is the mean of the source footprint it covers) or interpolates linearly
//! (upscale, pixel centres aligned). Weights are computed on the fly, so resampling
//! allocates nothing.

use rayon::iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use crate::plane::Plane;

/// An axis-aligned rectangle of source pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Returns the centred crop of a source with the destination's aspect ratio.
///
/// A source that exceeds the destination by at most one pixel per axis (an odd size
/// rounded down to even) is cropped to the destination size exactly, so no resampling
/// blur is introduced.
pub(crate) fn cover_crop(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Rect {
    if (dst_w..=dst_w + 1).contains(&src_w) && (dst_h..=dst_h + 1).contains(&src_h) {
        return Rect {
            x: 0,
            y: 0,
            width: dst_w,
            height: dst_h,
        };
    }
    let wide = u64::from(src_w) * u64::from(dst_h) > u64::from(dst_w) * u64::from(src_h);
    if wide {
        let width = (f64::from(src_h) * f64::from(dst_w) / f64::from(dst_h)).round() as u32;
        let width = width.clamp(1, src_w);
        Rect {
            x: (src_w - width) / 2,
            y: 0,
            width,
            height: src_h,
        }
    } else {
        let height = (f64::from(src_w) * f64::from(dst_h) / f64::from(dst_w)).round() as u32;
        let height = height.clamp(1, src_h);
        Rect {
            x: 0,
            y: (src_h - height) / 2,
            width: src_w,
            height,
        }
    }
}

/// Resampling of one axis from a source span onto `out` samples.
#[derive(Debug, Clone, Copy)]
struct Axis {
    start: u32,
    len: u32,
    scale: f64,
    kernel: Kernel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kernel {
    Copy,
    Area,
    Linear,
}

impl Axis {
    fn new(start: u32, len: u32, out: u32) -> Self {
        let kernel = match len.cmp(&out) {
            std::cmp::Ordering::Equal => Kernel::Copy,
            std::cmp::Ordering::Greater => Kernel::Area,
            std::cmp::Ordering::Less => Kernel::Linear,
        };
        Self {
            start,
            len,
            scale: f64::from(len) / f64::from(out),
            kernel,
        }
    }

    /// Calls `tap(source_index, weight)` for every source sample contributing to output `i`.
    fn taps(&self, i: usize, mut tap: impl FnMut(usize, f32)) {
        let first = self.start as usize;
        let last = first + self.len as usize - 1;
        match self.kernel {
            Kernel::Copy => tap(first + i, 1.0),
            Kernel::Area => {
                let a = f64::from(self.start) + i as f64 * self.scale;
                let b = a + self.scale;
                let lo = a.floor() as usize;
                let hi = (b.ceil() as usize).min(last + 1);
                for u in lo..hi {
                    let overlap = (b.min(u as f64 + 1.0) - a.max(u as f64)) / self.scale;
                    if overlap > 0.0 {
                        tap(u, overlap as f32);
                    }
                }
            }
            Kernel::Linear => {
                let pos = (f64::from(self.start) + (i as f64 + 0.5) * self.scale - 0.5)
                    .clamp(first as f64, last as f64);
                let u0 = pos.floor() as usize;
                let t = (pos - u0 as f64) as f32;
                tap(u0, 1.0 - t);
                if t > 0.0 {
                    tap((u0 + 1).min(last), t);
                }
            }
        }
    }
}

/// How the source is turned before cropping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Turn {
    /// The source as stored.
    None,
    /// The source turned 90 degrees clockwise: its left column becomes the top row.
    Clockwise,
}

/// Resamples the `crop` region of `src`, turned by `turn`, onto the whole of `dst`.
///
/// `crop` is in the coordinates of the turned source.
///
/// # Panics
///
/// Panics if `crop` is empty or extends beyond the turned source.
pub(crate) fn resample(src: &Plane<f32>, turn: Turn, crop: Rect, dst: &mut Plane<f32>) {
    let (view_w, view_h) = match turn {
        Turn::None => (src.width(), src.height()),
        Turn::Clockwise => (src.height(), src.width()),
    };
    assert!(
        crop.width > 0
            && crop.height > 0
            && crop.x + crop.width <= view_w
            && crop.y + crop.height <= view_h,
        "crop {crop:?} does not fit a {view_w} x {view_h} source"
    );
    if dst.width() == 0 || dst.height() == 0 {
        return;
    }
    let ax = Axis::new(crop.x, crop.width, dst.width());
    let ay = Axis::new(crop.y, crop.height, dst.height());
    let data = src.data();
    let src_w = src.width() as usize;
    let last_row = src.height() as usize - 1;
    match turn {
        Turn::None => run(ax, ay, dst, |u, v| data[v * src_w + u]),
        Turn::Clockwise => run(ax, ay, dst, |u, v| data[(last_row - u) * src_w + v]),
    }
}

/// Fills `dst` from a source read through `fetch(column, row)`.
fn run(ax: Axis, ay: Axis, dst: &mut Plane<f32>, fetch: impl Fn(usize, usize) -> f32 + Sync) {
    let dst_w = dst.width() as usize;
    dst.data_mut()
        .par_chunks_mut(dst_w)
        .enumerate()
        .for_each(|(j, out_row)| {
            out_row.fill(0.0);
            ay.taps(j, |v, wy| {
                for (i, out) in out_row.iter_mut().enumerate() {
                    let mut acc = 0.0;
                    ax.taps(i, |u, wx| acc += wx * fetch(u, v));
                    *out += wy * acc;
                }
            });
        });
}

/// Averages groups of `factor` adjacent columns of `src` into `dst`.
///
/// # Panics
///
/// Panics if `dst` is not `src.width() / factor` by `src.height()`.
pub(crate) fn bin_columns(src: &Plane<f32>, factor: u32, dst: &mut Plane<f32>) {
    assert!(
        factor > 0 && dst.width() == src.width() / factor && dst.height() == src.height(),
        "binning {} x {} by {factor} cannot fill {} x {}",
        src.width(),
        src.height(),
        dst.width(),
        dst.height()
    );
    if dst.width() == 0 {
        return;
    }
    let factor = factor as usize;
    let scale = 1.0 / factor as f32;
    let src_w = src.width() as usize;
    let data = src.data();
    let dst_w = dst.width() as usize;
    dst.data_mut()
        .par_chunks_mut(dst_w)
        .zip((0..src.height() as usize).into_par_iter())
        .for_each(|(out_row, y)| {
            let src_row = &data[y * src_w..(y + 1) * src_w];
            for (out, group) in out_row.iter_mut().zip(src_row.chunks_exact(factor)) {
                *out = group.iter().sum::<f32>() * scale;
            }
        });
}
