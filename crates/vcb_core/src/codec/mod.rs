//! JPEG simulation: the camera's encoder, its scan, and the decoder that reads the file.
//!
//! The camera stores every still, and every movie frame, as a baseline JPEG. Its
//! artefacts are part of the look, and [`encode`] reproduces them on a [`Frame`] in
//! place, as an encode followed by a decode, leaving the size unchanged:
//!
//! 1. RGB to `Y'CbCr`, BT.601 full range as in libjpeg, luma level-shifted by 128.
//! 2. Chroma subsampling ([`Subsampling`]): 4:2:2 averages horizontal pairs (the
//!    A520's files, research section 7), 4:2:0 averages 2 by 2 squares.
//! 3. Each plane is padded to whole MCUs by repeating its last row and column: luma
//!    to 16 by 8 pixels for 4:2:2, 16 by 16 for 4:2:0 and 8 by 8 for 4:4:4, chroma to
//!    8 by 8 samples. Every 8 by 8 block goes through the floating-point forward DCT
//!    and is quantised to integers with the camera's luma or chroma table for the
//!    mode's quality setting (`round(F / Q)`, the measured Canon tables of research
//!    section 7).
//! 4. The decoder dequantises (`q Q`), transforms back and upsamples chroma
//!    ([`ChromaUp`]): by repetition, or by libjpeg's "fancy" triangle filter, which
//!    weighs the nearer chroma sample 3/4 and the farther 1/4 along each subsampled
//!    axis.
//! 5. Back to RGB, rounded and clipped to 8 bits.
//!
//! With every quantiser step at 1 and no subsampling the round trip changes no value
//! by more than about 2.
//!
//! # The scan
//!
//! Between steps 3 and 4 the encoder entropy-codes the quantised blocks into the scan,
//! the file's bytes after its headers, and the decoder reads them back. The scan is
//! baseline sequential Huffman coding (T.81 Annex F) through the standard's example
//! tables (Annex K.3; the A520's own tables were not read from its files, but the
//! sample on file and most compacts of its time use these). The components are
//! interleaved by MCU, in raster order: for 4:2:2 two luma blocks then one Cb and one
//! Cr block, for 4:2:0 four luma blocks then Cb and Cr, for 4:4:4 one block of each.
//! Each block is its DC difference to the component's previous block, then its AC
//! coefficients in zigzag order as zero runs and sizes, with end-of-block and
//! sixteen-zero symbols; every `0xFF` byte is followed by a stuffed `0x00` and the
//! last byte is padded with one bits. With a non-zero [`Params::restart_interval`] an
//! `RSTm` marker (`0xFF`, `0xD0 + m`, `m` counting 0 to 7 and round again) follows
//! every `restart_interval` MCUs but the last, and the DC predictions restart at 0.
//!
//! [`encode_scan`] runs steps 1 to 3 and writes the scan into a reused buffer;
//! [`decode_scan`] reads a scan and runs steps 4 and 5. The coding is lossless:
//! [`encode_scan`] followed by [`decode_scan`] gives exactly the frame of [`encode`].
//! Between the two the scan sits in the camera's memory, where faults corrupt it.
//!
//! # Decoding a damaged scan
//!
//! [`decode_scan`] reads a damaged scan the way libjpeg (6b and turbo) does:
//!
//! - A corrupted bit mis-decodes a code, and the decoder goes on out of step: wrong
//!   sizes and runs, blocks that end early or late, MCUs shifted sideways, and DC
//!   differences that accumulate wrongly, tinting and shading everything after the
//!   fault. A bit pattern that is no code reads as symbol 0, a zero DC difference or
//!   an end of block, and consumes the 17 bits libjpeg examined.
//! - A run past a block's last coefficient puts its value in the last coefficient,
//!   the highest frequency, and ends the block.
//! - `0xFF` followed by anything but `0x00` (after any further `0xFF` fill bytes) is a
//!   marker, and the end of the data reads as one. The decoder uses the bits before
//!   it, then zero bits to finish the MCU in progress; every later MCU is all zero
//!   coefficients, flat grey (128 in each channel), up to the next restart.
//! - At each restart, every `restart_interval` MCUs, the decoder drops the bits it
//!   holds, finds the next marker if it is not already at one, resets the DC
//!   predictions, and decodes the next interval from the bytes after it. If the marker
//!   is not the `RSTm` it expects, it follows libjpeg's `jpeg_resync_to_restart`: an
//!   `RST` one or two numbers ahead, or any other JPEG marker, is left unread, so the
//!   intervals until its number comes due decode as grey (the first MCU from zero
//!   bits); an `RST` one or two behind, or a code below `0xC0` that is no marker, is
//!   skipped and the next marker examined; an `RST` further away is taken for the
//!   expected one. A bit error inside one interval therefore leaves the later
//!   intervals intact, unless it forms a JPEG marker other than `RST`, which stays
//!   unread and greys the rest of the scan.
//! - With `restart_interval` 0 nothing resynchronises: an `RSTm` byte pair is a marker
//!   like any other and greys the rest of the scan.
//!
//! Not modelled: progressive, hierarchical and lossless modes, arithmetic coding,
//! Huffman tables optimised per image, and the file's headers (SOI, DQT, SOF0, DHT,
//! DRI, SOS, EOI), which the model assumes intact around the scan. Nor is libjpeg's
//! range limit of each component after the inverse DCT, which clips `Y'`, Cb and Cr
//! before upsampling and colour conversion and wraps samples far out of range around;
//! the model clips only the RGB bytes of step 5. The two differ only where a component
//! leaves its range, as it does in a badly damaged scan.

mod dct;
mod huffman;
mod scan;

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::{ParallelSlice, ParallelSliceMut};

use crate::isp::ycc;
use crate::plane::Plane;
use crate::preset::Mode;
use crate::{Camera, Error, Frame, Preset, Timing};

use dct::{Block, Dct};
use scan::{Coefficients, Grid};

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
    /// MCUs per restart interval of the scan; 0 writes no restart markers.
    ///
    /// With a non-zero interval the encoder writes an `RSTm` marker and restarts its
    /// DC predictions after every `restart_interval` MCUs, and the decoder
    /// resynchronises at each marker. A JPEG file's DRI segment holds the interval in
    /// 16 bits.
    pub restart_interval: u32,
}

impl Params {
    /// Resolves the codec parameters from the camera, preset and derived timing.
    ///
    /// The still and movie sections each select a quality setting; its tables come
    /// from the camera's `[jpeg.tables]`. The scan has no restart markers.
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
            // TODO(stage 20c): read `camera.jpeg.restart_interval` once the camera
            // model has the key.
            restart_interval: 0,
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
/// Holds the padded luma plane and the padded, subsampled chroma planes, and the
/// quantised coefficients of their blocks.
#[derive(Debug, Clone)]
pub struct State {
    luma: Component,
    cb: Component,
    cr: Component,
}

impl Default for State {
    fn default() -> Self {
        Self {
            luma: Component::empty(),
            cb: Component::empty(),
            cr: Component::empty(),
        }
    }
}

impl State {
    /// Sizes every plane for `layout`, reallocating only on a change of size.
    fn fit(&mut self, layout: &Layout) {
        self.luma.fit(layout.luma_size());
        self.cb.fit(layout.chroma_size());
        self.cr.fit(layout.chroma_size());
    }
}

/// One component of the frame: its padded samples and their quantised blocks.
#[derive(Debug, Clone)]
struct Component {
    /// Samples centred on zero, padded to whole MCUs.
    samples: Plane<f32>,
    /// Quantised coefficients: each row holds one row of blocks, 64 values a block.
    blocks: Plane<i16>,
}

impl Component {
    fn empty() -> Self {
        Self {
            samples: Plane::new(0, 0),
            blocks: Plane::new(0, 0),
        }
    }

    /// Sizes the component for a padded plane of `width` by `height` samples.
    fn fit(&mut self, (width, height): (u32, u32)) {
        fit(&mut self.samples, width, height);
        fit(&mut self.blocks, 8 * width, height / 8);
    }

    /// Returns the blocks in raster order.
    fn blocks(&self) -> &[Coefficients] {
        self.blocks.data().as_chunks().0
    }

    /// Returns the blocks in raster order, mutably.
    fn blocks_mut(&mut self) -> &mut [Coefficients] {
        self.blocks.data_mut().as_chunks_mut().0
    }
}

/// The MCU grid of a frame and the sizes of its planes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    /// Chroma subsampling factors `(horizontal, vertical)`: luma blocks per MCU.
    factors: (u32, u32),
    /// Size of the chroma planes before padding.
    chroma: (u32, u32),
    /// MCUs across and down.
    mcus: (u32, u32),
}

impl Layout {
    /// Returns the layout of a `width` by `height` frame, or `None` if it is empty.
    fn new(width: u32, height: u32, subsampling: Subsampling) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let (sx, sy) = subsampling.factors();
        Some(Self {
            factors: (sx, sy),
            chroma: (width.div_ceil(sx), height.div_ceil(sy)),
            mcus: (width.div_ceil(8 * sx), height.div_ceil(8 * sy)),
        })
    }

    /// Returns the padded size of the luma plane: whole MCUs.
    fn luma_size(&self) -> (u32, u32) {
        (
            8 * self.factors.0 * self.mcus.0,
            8 * self.factors.1 * self.mcus.1,
        )
    }

    /// Returns the padded size of a chroma plane: one block per MCU.
    fn chroma_size(&self) -> (u32, u32) {
        (8 * self.mcus.0, 8 * self.mcus.1)
    }

    /// Returns the MCU grid of the scan.
    fn grid(&self) -> Grid {
        Grid {
            columns: self.mcus.0 as usize,
            rows: self.mcus.1 as usize,
            factors: (self.factors.0 as usize, self.factors.1 as usize),
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
///     restart_interval: 0,
/// };
/// let mut frame = Frame::from_rgb(2, 1, vec![200, 40, 90, 10, 220, 128]);
/// let before = frame.clone();
/// codec::encode(&params, &mut State::default(), &mut frame);
/// assert!(frame.rgb().iter().zip(before.rgb()).all(|(a, b)| a.abs_diff(*b) <= 2));
/// ```
pub fn encode(params: &Params, state: &mut State, frame: &mut Frame) {
    let Some(layout) = Layout::new(frame.width(), frame.height(), params.subsampling) else {
        return;
    };
    let dct = Dct::new();
    analyse(params, &layout, &dct, state, frame);
    synthesise(params, &layout, &dct, state, frame);
}

/// Encodes `frame` and writes its entropy-coded scan into `scan`.
///
/// `scan` is cleared first and receives the scan's bytes only, with any restart
/// markers, without the file's headers; its capacity is reused. An empty frame
/// writes an empty scan.
///
/// # Examples
///
/// ```
/// use vcb_core::Frame;
/// use vcb_core::codec::{self, ChromaUp, Params, QuantTables, Quality, State, Subsampling};
///
/// let params = Params {
///     quality: Quality::Normal,
///     tables: QuantTables { luma: [[4; 8]; 8], chroma: [[8; 8]; 8] },
///     subsampling: Subsampling::S422,
///     chroma_upsample: ChromaUp::Bilinear,
///     restart_interval: 2,
/// };
/// let rgb = (0..48 * 16 * 3).map(|i| (i * 37 % 251) as u8).collect();
/// let frame = Frame::from_rgb(48, 16, rgb);
/// let mut state = State::default();
/// let mut scan = Vec::new();
/// codec::encode_scan(&params, &mut state, &frame, &mut scan);
///
/// // Six MCUs of 16 by 8 pixels in intervals of two: RST0 and RST1 between them.
/// let markers: Vec<u8> = scan.windows(2).filter(|w| w[0] == 0xFF && w[1] != 0).map(|w| w[1]).collect();
/// assert_eq!(markers, [0xD0, 0xD1]);
///
/// let mut decoded = Frame::new(48, 16);
/// codec::decode_scan(&params, &mut state, &scan, &mut decoded);
/// let mut direct = frame.clone();
/// codec::encode(&params, &mut state, &mut direct);
/// assert_eq!(decoded, direct);
/// ```
pub fn encode_scan(params: &Params, state: &mut State, frame: &Frame, scan: &mut Vec<u8>) {
    scan.clear();
    let Some(layout) = Layout::new(frame.width(), frame.height(), params.subsampling) else {
        return;
    };
    analyse(params, &layout, &Dct::new(), state, frame);
    scan::encode(
        layout.grid(),
        [state.luma.blocks(), state.cb.blocks(), state.cr.blocks()],
        params.restart_interval,
        scan,
    );
}

/// Decodes `scan` into `frame` as libjpeg would, damaged or not.
///
/// The frame keeps its size, which must be the size `scan` was encoded at; every
/// byte is overwritten. The scan is read by the rules of the module documentation:
/// after damage the decoder runs out of step, greys the MCUs after a marker or the
/// end of the data, and recovers at restart markers.
///
/// # Examples
///
/// ```
/// use vcb_core::Frame;
/// use vcb_core::codec::{self, ChromaUp, Params, QuantTables, Quality, State, Subsampling};
///
/// let params = Params {
///     quality: Quality::Normal,
///     tables: QuantTables { luma: [[4; 8]; 8], chroma: [[8; 8]; 8] },
///     subsampling: Subsampling::S444,
///     chroma_upsample: ChromaUp::Nearest,
///     restart_interval: 0,
/// };
/// let frame = Frame::from_rgb(64, 8, vec![200; 64 * 8 * 3]);
/// let mut state = State::default();
/// let mut scan = Vec::new();
/// codec::encode_scan(&params, &mut state, &frame, &mut scan);
///
/// // A marker where the data should be: the decoder greys everything after it.
/// scan[0] = 0xFF;
/// scan[1] = 0xD9;
/// let mut decoded = Frame::new(64, 8);
/// codec::decode_scan(&params, &mut state, &scan, &mut decoded);
/// assert_eq!(decoded.pixel(63, 7), [128, 128, 128]);
/// ```
pub fn decode_scan(params: &Params, state: &mut State, scan: &[u8], frame: &mut Frame) {
    let Some(layout) = Layout::new(frame.width(), frame.height(), params.subsampling) else {
        return;
    };
    state.fit(&layout);
    scan::decode(
        layout.grid(),
        scan,
        params.restart_interval,
        [
            state.luma.blocks_mut(),
            state.cb.blocks_mut(),
            state.cr.blocks_mut(),
        ],
    );
    synthesise(params, &layout, &Dct::new(), state, frame);
}

/// Splits `frame` into padded planes and quantises every block: steps 1 to 3.
fn analyse(params: &Params, layout: &Layout, dct: &Dct, state: &mut State, frame: &Frame) {
    state.fit(layout);
    split(frame, layout.factors, state);
    pad(&mut state.luma.samples, frame.width(), frame.height());
    pad(&mut state.cb.samples, layout.chroma.0, layout.chroma.1);
    pad(&mut state.cr.samples, layout.chroma.0, layout.chroma.1);
    let (luma, chroma) = steps(&params.tables);
    quantise(&state.luma.samples, dct, &luma, &mut state.luma.blocks);
    quantise(&state.cb.samples, dct, &chroma, &mut state.cb.blocks);
    quantise(&state.cr.samples, dct, &chroma, &mut state.cr.blocks);
}

/// Dequantises every block and writes the decoded frame: steps 4 and 5.
fn synthesise(params: &Params, layout: &Layout, dct: &Dct, state: &mut State, frame: &mut Frame) {
    let (luma, chroma) = steps(&params.tables);
    dequantise(&state.luma.blocks, dct, &luma, &mut state.luma.samples);
    dequantise(&state.cb.blocks, dct, &chroma, &mut state.cb.samples);
    dequantise(&state.cr.blocks, dct, &chroma, &mut state.cr.samples);
    merge(
        state,
        layout.factors,
        layout.chroma,
        params.chroma_upsample,
        frame,
    );
}

/// Returns the luma and chroma quantiser steps as floats.
fn steps(tables: &QuantTables) -> (Block, Block) {
    let float = |table: &[[u16; 8]; 8]| table.map(|row| row.map(f32::from));
    (float(&tables.luma), float(&tables.chroma))
}

/// Reallocates `plane` if it is not `width` by `height`.
fn fit<T: Default + Clone>(plane: &mut Plane<T>, width: u32, height: u32) {
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
    let luma_stride = state.luma.samples.width() as usize;
    state
        .luma
        .samples
        .data_mut()
        .par_chunks_mut(luma_stride)
        .take(height)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, value) in row[..width].iter_mut().enumerate() {
                *value = ycc::from_rgb(pixel(x, y))[0] - LEVEL_SHIFT;
            }
        });

    let chroma_stride = state.cb.samples.width() as usize;
    let (chroma_w, chroma_h) = (width.div_ceil(sx), height.div_ceil(sy));
    let scale = 1.0 / (sx * sy) as f32;
    state
        .cb
        .samples
        .data_mut()
        .par_chunks_mut(chroma_stride)
        .zip(state.cr.samples.data_mut().par_chunks_mut(chroma_stride))
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

/// Transforms every block of `samples` and quantises it with `steps` into `blocks`.
fn quantise(samples: &Plane<f32>, dct: &Dct, steps: &Block, blocks: &mut Plane<i16>) {
    let width = samples.width() as usize;
    if width == 0 {
        return;
    }
    samples
        .data()
        .par_chunks(8 * width)
        .zip(blocks.data_mut().par_chunks_mut(8 * width))
        .for_each(|(band, row)| {
            for (bx, out) in row.as_chunks_mut::<64>().0.iter_mut().enumerate() {
                let x = 8 * bx;
                let mut block: Block = [[0.0; 8]; 8];
                for (v, block_row) in block.iter_mut().enumerate() {
                    block_row.copy_from_slice(&band[v * width + x..v * width + x + 8]);
                }
                let coefficients = dct.forward(&block);
                for ((out_row, row), step_row) in out
                    .as_chunks_mut::<8>()
                    .0
                    .iter_mut()
                    .zip(&coefficients)
                    .zip(steps)
                {
                    for ((q, &c), &step) in out_row.iter_mut().zip(row).zip(step_row) {
                        // Baseline coefficients of 8-bit samples lie within +-1024.
                        *q = (c / step).round() as i16;
                    }
                }
            }
        });
}

/// Dequantises every block of `blocks` with `steps` and inverse-transforms it into `samples`.
fn dequantise(blocks: &Plane<i16>, dct: &Dct, steps: &Block, samples: &mut Plane<f32>) {
    let width = samples.width() as usize;
    if width == 0 {
        return;
    }
    samples
        .data_mut()
        .par_chunks_mut(8 * width)
        .zip(blocks.data().par_chunks(8 * width))
        .for_each(|(band, row)| {
            for (bx, quantised) in row.as_chunks::<64>().0.iter().enumerate() {
                let x = 8 * bx;
                let mut coefficients: Block = [[0.0; 8]; 8];
                for ((out, q_row), step_row) in coefficients
                    .iter_mut()
                    .zip(quantised.as_chunks::<8>().0)
                    .zip(steps)
                {
                    for ((c, &q), &step) in out.iter_mut().zip(q_row).zip(step_row) {
                        *c = f32::from(q) * step;
                    }
                }
                for (v, block_row) in dct.inverse(&coefficients).iter().enumerate() {
                    band[v * width + x..v * width + x + 8].copy_from_slice(block_row);
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
    let luma_stride = state.luma.samples.width() as usize;
    let chroma_stride = state.cb.samples.width() as usize;
    let (luma, cb, cr) = (
        state.luma.samples.data(),
        state.cb.samples.data(),
        state.cr.samples.data(),
    );
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

#[cfg(test)]
mod tests {
    use super::{
        Block, ChromaUp, Dct, Frame, Params, Plane, Quality, QuantTables, State, Subsampling,
        encode, fit, merge, pad, split,
    };

    /// The codec's float round trip before quantisation yielded integers: each
    /// coefficient becomes `round(F / Q) Q` in place.
    fn float_round_trip(plane: &mut Plane<f32>, dct: &Dct, steps: &Block) {
        let width = plane.width() as usize;
        for band in plane.data_mut().chunks_mut(8 * width) {
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
        }
    }

    /// The former `encode`: planes padded to whole blocks, the float round trip.
    fn float_encode(params: &Params, frame: &mut Frame) {
        let (width, height) = (frame.width(), frame.height());
        let (sx, sy) = params.subsampling.factors();
        let (chroma_w, chroma_h) = (width.div_ceil(sx), height.div_ceil(sy));
        let mut state = State::default();
        let chroma = (chroma_w.next_multiple_of(8), chroma_h.next_multiple_of(8));
        fit(
            &mut state.luma.samples,
            width.next_multiple_of(8),
            height.next_multiple_of(8),
        );
        fit(&mut state.cb.samples, chroma.0, chroma.1);
        fit(&mut state.cr.samples, chroma.0, chroma.1);
        split(frame, (sx, sy), &mut state);
        pad(&mut state.luma.samples, width, height);
        pad(&mut state.cb.samples, chroma_w, chroma_h);
        pad(&mut state.cr.samples, chroma_w, chroma_h);
        let dct = Dct::new();
        let steps = |table: &[[u16; 8]; 8]| table.map(|row| row.map(f32::from));
        float_round_trip(&mut state.luma.samples, &dct, &steps(&params.tables.luma));
        float_round_trip(&mut state.cb.samples, &dct, &steps(&params.tables.chroma));
        float_round_trip(&mut state.cr.samples, &dct, &steps(&params.tables.chroma));
        merge(
            &state,
            (sx, sy),
            (chroma_w, chroma_h),
            params.chroma_upsample,
            frame,
        );
    }

    /// Hard-edged checks under deterministic noise.
    fn test_frame(width: u32, height: u32) -> Frame {
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        let mut seed = 0x2545_f491_u32;
        for y in 0..height {
            for x in 0..width {
                for k in 0..3 {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let edge = if (x / 5 + y / 3 + k) % 2 == 0 {
                        220
                    } else {
                        30
                    };
                    rgb.push((edge + seed % 36) as u8);
                }
            }
        }
        Frame::from_rgb(width, height, rgb)
    }

    #[test]
    fn integer_quantisation_renders_the_frames_of_the_float_round_trip() {
        let ramp = |base: u16, slope: u16| {
            std::array::from_fn(|v| std::array::from_fn(|u| base + slope * (u + v) as u16))
        };
        let tables = [
            QuantTables {
                luma: [[1; 8]; 8],
                chroma: [[1; 8]; 8],
            },
            QuantTables {
                luma: ramp(1, 1),
                chroma: ramp(2, 3),
            },
            QuantTables {
                luma: ramp(9, 7),
                chroma: ramp(17, 11),
            },
        ];
        for (width, height) in [(64, 48), (37, 21), (1, 1), (17, 33), (40, 17)] {
            let input = test_frame(width, height);
            for subsampling in [Subsampling::S444, Subsampling::S422, Subsampling::S420] {
                for chroma_upsample in [ChromaUp::Nearest, ChromaUp::Bilinear] {
                    for tables in &tables {
                        let params = Params {
                            quality: Quality::Normal,
                            tables: tables.clone(),
                            subsampling,
                            chroma_upsample,
                            restart_interval: 0,
                        };
                        let mut expected = input.clone();
                        float_encode(&params, &mut expected);
                        let mut actual = input.clone();
                        encode(&params, &mut State::default(), &mut actual);
                        assert!(
                            expected == actual,
                            "{width}x{height} {subsampling:?} {chroma_upsample:?} {tables:?}"
                        );
                    }
                }
            }
        }
    }
}
