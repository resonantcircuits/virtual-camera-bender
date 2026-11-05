//! Separable Gaussian blur of planes, with edges clamped.

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::{ParallelSlice, ParallelSliceMut};

use crate::plane::Plane;

/// Kernel half-width in standard deviations; the tails beyond hold 0.3 % of the weight.
const KERNEL_SIGMAS: f32 = 3.0;

/// Fills `kernel` with the one-sided weights of a normalised Gaussian of `sigma` pixels.
///
/// `kernel[0]` is the centre weight and `kernel[k]` the weight at offsets `-k` and `+k`;
/// the full kernel sums to one.
pub(super) fn gaussian_kernel(sigma: f32, kernel: &mut Vec<f32>) {
    let radius = (KERNEL_SIGMAS * sigma).ceil().max(1.0) as usize;
    let scale = -0.5 / (sigma * sigma);
    kernel.clear();
    kernel.extend((0..=radius).map(|k| (scale * (k * k) as f32).exp()));
    let total = kernel[0] + 2.0 * kernel[1..].iter().sum::<f32>();
    for weight in kernel.iter_mut() {
        *weight /= total;
    }
}

/// Blurs `plane` in place, using `scratch` of the same size.
pub(super) fn blur(plane: &mut Plane<f32>, scratch: &mut Plane<f32>, kernel: &[f32]) {
    horizontal(plane, scratch, kernel);
    vertical(scratch, plane, kernel);
}

/// Writes the blur of `src` into `dst`, using `scratch`; all three have the same size.
pub(super) fn blur_into(
    src: &Plane<f32>,
    scratch: &mut Plane<f32>,
    dst: &mut Plane<f32>,
    kernel: &[f32],
) {
    horizontal(src, scratch, kernel);
    vertical(scratch, dst, kernel);
}

fn horizontal(src: &Plane<f32>, dst: &mut Plane<f32>, kernel: &[f32]) {
    let width = src.width() as usize;
    if width == 0 {
        return;
    }
    let radius = kernel.len() - 1;
    let last = width - 1;
    dst.data_mut()
        .par_chunks_mut(width)
        .zip(src.data().par_chunks(width))
        .for_each(|(out, row)| {
            for (x, value) in out.iter_mut().enumerate() {
                let mut acc = kernel[0] * row[x];
                if x >= radius && x + radius <= last {
                    for (k, &weight) in kernel.iter().enumerate().skip(1) {
                        acc += weight * (row[x - k] + row[x + k]);
                    }
                } else {
                    for (k, &weight) in kernel.iter().enumerate().skip(1) {
                        acc += weight * (row[x.saturating_sub(k)] + row[(x + k).min(last)]);
                    }
                }
                *value = acc;
            }
        });
}

fn vertical(src: &Plane<f32>, dst: &mut Plane<f32>, kernel: &[f32]) {
    let width = src.width() as usize;
    if width == 0 {
        return;
    }
    let last = src.height() as usize - 1;
    dst.data_mut()
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, out)| {
            let centre = src.row(y as u32);
            for (value, &sample) in out.iter_mut().zip(centre) {
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
