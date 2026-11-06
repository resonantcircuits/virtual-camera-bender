//! JPEG simulation: the compression artefacts of the camera's encoder.
//!
//! The camera stores every still, and every movie frame, as a baseline JPEG. Its
//! artefacts are part of the look, and [`encode`] reproduces them on a [`Frame`] in
//! place, as an encode followed by a decode, leaving the size unchanged:
//!
//! 1. RGB to `Y'CbCr`, BT.601 full range as in libjpeg, luma level-shifted by 128.
//! 2. Chroma subsampling ([`Subsampling`]): 4:2:2 averages horizontal pairs (the
//!    A520's files, research section 7), 4:2:0 averages 2 by 2 squares.
//! 3. Each plane is padded to whole 8 by 8 blocks by repeating its last row and
//!    column; every block goes through the floating-point forward DCT, is quantised
//!    with the camera's luma or chroma table for the mode's quality setting
//!    (`round(F / Q)`, the measured Canon tables of research section 7), dequantised
//!    and transformed back.
//! 4. The decoder upsamples chroma ([`ChromaUp`]): by repetition, or by libjpeg's
//!    "fancy" triangle filter, which weighs the nearer chroma sample 3/4 and the
//!    farther 1/4 along each subsampled axis.
//! 5. Back to RGB, rounded and clipped to 8 bits.
//!
//! Entropy coding is lossless and is not modelled. With every quantiser step at 1
//! and no subsampling the round trip changes no value by more than about 2.

mod dct;

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::{ParallelSlice, ParallelSliceMut};

use crate::isp::ycc;
use crate::plane::Plane;
use crate::preset::Mode;
use crate::{Camera, Error, Frame, Preset, Timing};

use dct::{Block, Dct};

/// JPEG's level shift: samples are centred on zero before the DCT.
const LEVEL_SHIFT: f32 = 128.0;

/// Chroma subsampling of the encoder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Subsampling {
    /// Full-resolution chroma.
    #[cfg_attr(feature = "serde", serde(rename = "4:4:4"))]
    S444,
    /// Chroma at half resolution horizontally, full resolution vertically.
    #[default]
    #[cfg_attr(feature = "serde", serde(rename = "4:2:2"))]
    S422,
    /// Chroma at half resolution in both directions.
    #[cfg_attr(feature = "serde", serde(rename = "4:2:0"))]
    S420,
}

/// Chroma upsampling of the decoder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum ChromaUp {
    /// Each chroma sample covers its block of pixels, giving chunky colour edges.
    Nearest,
    /// Chroma interpolated between sample centres.
    #[default]
    Bilinear,
}

/// The camera's JPEG quality setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Quality {
    /// The finest setting.
    #[default]
    Superfine,
    /// The middle setting.
    Fine,
    /// The coarsest setting.
    Normal,
}

/// One luma and one chroma quantisation table, rows of 8 in natural order.
///
/// Entry `[v][u]` is the quantiser step of vertical frequency `v` and horizontal
/// frequency `u`; `[0][0]` is the DC step.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct QuantTables {
    /// Quantiser steps of the luma channel.
    pub luma: [[u16; 8]; 8],
    /// Quantiser steps of both chroma channels.
    pub chroma: [[u16; 8]; 8],
}

/// Parameters of the JPEG encoder and decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Params {
    /// Quality setting of the readout mode.
    pub quality: Quality,
    /// Quantisation tables of that setting.
    pub tables: QuantTables,
    /// Chroma subsampling.
    pub subsampling: Subsampling,
    /// Chroma upsampling of the decoder.
    pub chroma_upsample: ChromaUp,
}

impl Params {
    /// Resolves the codec parameters from the camera, preset and derived timing.
    ///
    /// The still and movie sections each select a quality setting; its tables come
    /// from the camera's `[jpeg.tables]`.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let _ = preset;
        let quality = match timing.mode {
            Mode::Still => camera.still.jpeg,
            Mode::Movie => camera.movie.jpeg,
        };
        Ok(Self {
            quality,
            tables: camera.jpeg.tables.get(quality).clone(),
            subsampling: camera.jpeg.subsampling,
            chroma_upsample: camera.jpeg.chroma_upsample,
        })
    }
}

impl Subsampling {
    /// Returns the chroma subsampling factors `(horizontal, vertical)`.
    #[must_use]
    pub fn factors(self) -> (u32, u32) {
        match self {
            Self::S444 => (1, 1),
            Self::S422 => (2, 1),
            Self::S420 => (2, 2),
        }
    }
}

/// Working memory of the codec, reused across frames.
///
/// Holds the padded luma plane and the padded, subsampled chroma planes.
#[derive(Debug, Clone)]
pub struct State {
    luma: Plane<f32>,
    cb: Plane<f32>,
    cr: Plane<f32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            luma: Plane::new(0, 0),
            cb: Plane::new(0, 0),
            cr: Plane::new(0, 0),
        }
    }
}

/// Applies the encoder's and decoder's artefacts to `frame` in place.
///
/// # Examples
///
/// ```
/// use vcb_core::Frame;
/// use vcb_core::codec::{self, ChromaUp, Params, QuantTables, Quality, State, Subsampling};
///
/// let params = Params {
///     quality: Quality::Superfine,
///     tables: QuantTables { luma: [[1; 8]; 8], chroma: [[1; 8]; 8] },
///     subsampling: Subsampling::S444,
///     chroma_upsample: ChromaUp::Bilinear,
/// };
/// let mut frame = Frame::from_rgb(2, 1, vec![200, 40, 90, 10, 220, 128]);
/// let before = frame.clone();
/// codec::encode(&params, &mut State::default(), &mut frame);
/// assert!(frame.rgb().iter().zip(before.rgb()).all(|(a, b)| a.abs_diff(*b) <= 2));
/// ```
pub fn encode(params: &Params, state: &mut State, frame: &mut Frame) {
    let (width, height) = (frame.width(), frame.height());
    if width == 0 || height == 0 {
        return;
    }
    let (sx, sy) = params.subsampling.factors();
    let (chroma_w, chroma_h) = (width.div_ceil(sx), height.div_ceil(sy));
    fit(
        &mut state.luma,
        width.next_multiple_of(8),
        height.next_multiple_of(8),
    );
    fit(
        &mut state.cb,
        chroma_w.next_multiple_of(8),
        chroma_h.next_multiple_of(8),
    );
    fit(
        &mut state.cr,
        chroma_w.next_multiple_of(8),
        chroma_h.next_multiple_of(8),
    );

    split(frame, (sx, sy), state);
    pad(&mut state.luma, width, height);
    pad(&mut state.cb, chroma_w, chroma_h);
    pad(&mut state.cr, chroma_w, chroma_h);
    let dct = Dct::new();
    let steps = |table: &[[u16; 8]; 8]| table.map(|row| row.map(f32::from));
    quantise(&mut state.luma, &dct, &steps(&params.tables.luma));
    quantise(&mut state.cb, &dct, &steps(&params.tables.chroma));
    quantise(&mut state.cr, &dct, &steps(&params.tables.chroma));
    merge(
        state,
        (sx, sy),
        (chroma_w, chroma_h),
        params.chroma_upsample,
        frame,
    );
}

/// Reallocates `plane` if it is not `width` by `height`.
fn fit(plane: &mut Plane<f32>, width: u32, height: u32) {
    if plane.width() != width || plane.height() != height {
        *plane = Plane::new(width, height);
    }
}

/// Converts the frame into level-shifted luma and subsampled chroma planes.
fn split(frame: &Frame, (sx, sy): (u32, u32), state: &mut State) {
    let (width, height) = (frame.width() as usize, frame.height() as usize);
    let (sx, sy) = (sx as usize, sy as usize);
    let rgb = frame.rgb();
    let pixel = |x: usize, y: usize| {
        let i = (y * width + x) * 3;
        [rgb[i], rgb[i + 1], rgb[i + 2]].map(f32::from)
    };
    let luma_stride = state.luma.width() as usize;
    state
        .luma
        .data_mut()
        .par_chunks_mut(luma_stride)
        .take(height)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, value) in row[..width].iter_mut().enumerate() {
                *value = ycc::from_rgb(pixel(x, y))[0] - LEVEL_SHIFT;
            }
        });

    let chroma_stride = state.cb.width() as usize;
    let (chroma_w, chroma_h) = (width.div_ceil(sx), height.div_ceil(sy));
    let scale = 1.0 / (sx * sy) as f32;
    state
        .cb
        .data_mut()
        .par_chunks_mut(chroma_stride)
        .zip(state.cr.data_mut().par_chunks_mut(chroma_stride))
        .take(chroma_h)
        .enumerate()
        .for_each(|(j, (cb_row, cr_row))| {
            for i in 0..chroma_w {
                let (mut cb, mut cr) = (0.0, 0.0);
                for dy in 0..sy {
                    let y = (j * sy + dy).min(height - 1);
                    for dx in 0..sx {
                        let x = (i * sx + dx).min(width - 1);
                        let [_, b, r] = ycc::from_rgb(pixel(x, y));
                        cb += b;
                        cr += r;
                    }
                }
                cb_row[i] = cb * scale;
                cr_row[i] = cr * scale;
            }
        });
}

/// Fills the plane beyond its first `width` columns and `height` rows by repeating the edge.
fn pad(plane: &mut Plane<f32>, width: u32, height: u32) {
    let stride = plane.width() as usize;
    let (width, height) = (width as usize, height as usize);
    if width < stride {
        for row in plane.rows_mut().take(height) {
            let edge = row[width - 1];
            row[width..].fill(edge);
        }
    }
    let (head, tail) = plane.data_mut().split_at_mut(height * stride);
    let last = &head[(height - 1) * stride..];
    for row in tail.chunks_exact_mut(stride) {
        row.copy_from_slice(last);
    }
}

/// Transforms, quantises with `steps`, dequantises and inverse-transforms every block.
fn quantise(plane: &mut Plane<f32>, dct: &Dct, steps: &Block) {
    let width = plane.width() as usize;
    if width == 0 {
        return;
    }
    plane.data_mut().par_chunks_mut(8 * width).for_each(|band| {
        for bx in (0..width).step_by(8) {
            let mut block: Block = [[0.0; 8]; 8];
            for (v, row) in block.iter_mut().enumerate() {
                row.copy_from_slice(&band[v * width + bx..v * width + bx + 8]);
            }
            let mut coefficients = dct.forward(&block);
            for (row, step_row) in coefficients.iter_mut().zip(steps) {
                for (c, &step) in row.iter_mut().zip(step_row) {
                    *c = (*c / step).round() * step;
                }
            }
            for (v, row) in dct.inverse(&coefficients).iter().enumerate() {
                band[v * width + bx..v * width + bx + 8].copy_from_slice(row);
            }
        }
    });
}

/// Returns the chroma samples and the weight of the second one for output position `i`.
fn taps(i: usize, factor: u32, len: usize, up: ChromaUp) -> (usize, usize, f32) {
    let j = i / factor as usize;
    if factor == 1 || up == ChromaUp::Nearest {
        return (j, j, 0.0);
    }
    // libjpeg's triangle filter: the nearer sample weighs 3/4, the farther 1/4.
    let other = if i.is_multiple_of(2) {
        j.saturating_sub(1)
    } else {
        (j + 1).min(len - 1)
    };
    (j, other, 0.25)
}

/// Upsamples chroma, converts back to RGB and writes the frame's bytes.
fn merge(
    state: &State,
    (sx, sy): (u32, u32),
    (chroma_w, chroma_h): (u32, u32),
    up: ChromaUp,
    frame: &mut Frame,
) {
    let width = frame.width() as usize;
    let luma_stride = state.luma.width() as usize;
    let chroma_stride = state.cb.width() as usize;
    let (luma, cb, cr) = (state.luma.data(), state.cb.data(), state.cr.data());
    frame
        .rgb_mut()
        .par_chunks_mut(width * 3)
        .zip(luma.par_chunks(luma_stride))
        .enumerate()
        .for_each(|(y, (bytes, luma_row))| {
            let (r0, r1, wv) = taps(y, sy, chroma_h as usize, up);
            for (x, pixel) in bytes.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let (c0, c1, wh) = taps(x, sx, chroma_w as usize, up);
                let sample = |plane: &[f32]| {
                    let at = |r: usize, c: usize| plane[r * chroma_stride + c];
                    let top = wh.mul_add(at(r0, c1) - at(r0, c0), at(r0, c0));
                    let bottom = wh.mul_add(at(r1, c1) - at(r1, c0), at(r1, c0));
                    wv.mul_add(bottom - top, top)
                };
                let rgb = ycc::to_rgb([luma_row[x] + LEVEL_SHIFT, sample(cb), sample(cr)]);
                for (byte, value) in pixel.iter_mut().zip(rgb) {
                    *byte = value.round().clamp(0.0, 255.0) as u8;
                }
            }
        });
}
