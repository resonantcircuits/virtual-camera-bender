//! Readout: charge transferred out of the array into a time-ordered sample stream.
//!
//! The CCD shifts its rows vertically into the horizontal register one at a time, and
//! the register shifts each row out one sample per pixel period. Every line starts
//! with `ob_cols` optical-black samples from masked photosites, followed by the
//! active samples, then `hblank_px` periods of horizontal blanking; every frame ends
//! with `vblank_lines` lines of vertical blanking.
//!
//! A [`Stream`] holds the samples in readout order, in electrons. Blanking is not
//! stored: the [`Layout`] carries its length so that the analog stages can advance
//! time across it.
//!
//! The still readout reads every row. The movie readout keeps one row pair out of
//! every `line_skip` pairs (rows `0, 1`, then `2k, 2k + 1`, ...), which preserves the
//! Bayer phase. [`Direction`] says which end of the array is read first. The current
//! transfer model is lossless: charge arrives unchanged and optical-black photosites
//! hold no charge.

use std::fmt::{self, Debug, Formatter};

use crate::noise;
use crate::sensor::Charge;
use crate::{Camera, Error, Preset, Timing};

/// Salt of the readout's noise sources.
const SALT: u32 = noise::salt("readout");

/// Which end of the sensor the vertical transfer reads first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum Direction {
    /// Sensor row 0 is read first.
    #[default]
    TopDown,
    /// The last sensor row is read first.
    BottomUp,
}

/// Geometry and timing of a sample stream.
#[derive(Clone, PartialEq)]
pub struct Layout {
    rows: u32,
    cols: u32,
    ob_cols: u32,
    hblank_px: u32,
    vblank_lines: u32,
    pixel_period_s: f64,
    line_skip: u32,
    direction: Direction,
    row_map: Box<[u32]>,
}

impl Layout {
    /// Builds the stream layout of a readout timing read in `direction`.
    #[must_use]
    pub fn from_timing(timing: &Timing, direction: Direction) -> Self {
        let pairs = timing.rows_read / 2;
        let mut row_map = Vec::with_capacity(timing.rows_read as usize);
        for pair in 0..pairs {
            let first = pair * 2 * timing.line_skip;
            row_map.extend([first, first + 1]);
        }
        if direction == Direction::BottomUp {
            row_map.reverse();
        }
        Self {
            rows: timing.rows_read,
            cols: timing.cols,
            ob_cols: timing.ob_cols,
            hblank_px: timing.hblank_px,
            vblank_lines: timing.vblank_lines,
            pixel_period_s: timing.pixel_period_s,
            line_skip: timing.line_skip,
            direction,
            row_map: row_map.into_boxed_slice(),
        }
    }

    /// Returns the number of lines read per frame.
    #[must_use]
    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// Returns the active samples per line.
    #[must_use]
    pub fn cols(&self) -> u32 {
        self.cols
    }

    /// Returns the optical-black samples at the start of each line.
    #[must_use]
    pub fn ob_cols(&self) -> u32 {
        self.ob_cols
    }

    /// Returns the horizontal blanking per line in pixel periods.
    #[must_use]
    pub fn hblank_px(&self) -> u32 {
        self.hblank_px
    }

    /// Returns the vertical blanking per frame in line periods.
    #[must_use]
    pub fn vblank_lines(&self) -> u32 {
        self.vblank_lines
    }

    /// Returns the duration of one pixel period in seconds.
    #[must_use]
    pub fn pixel_period_s(&self) -> f64 {
        self.pixel_period_s
    }

    /// Returns the line skip factor; 1 reads every row.
    #[must_use]
    pub fn line_skip(&self) -> u32 {
        self.line_skip
    }

    /// Returns which end of the sensor is read first.
    #[must_use]
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// Returns the sensor row read at each stream row.
    #[must_use]
    pub fn row_map(&self) -> &[u32] {
        &self.row_map
    }

    /// Returns the stored samples per line: optical black then active.
    #[must_use]
    pub fn line_len(&self) -> u32 {
        self.ob_cols + self.cols
    }

    /// Returns the stored samples per frame.
    #[must_use]
    pub fn sample_count(&self) -> usize {
        self.rows as usize * self.line_len() as usize
    }

    /// Returns the duration of one line, including blanking, in seconds.
    #[must_use]
    pub fn line_period_s(&self) -> f64 {
        f64::from(self.line_len() + self.hblank_px) * self.pixel_period_s
    }

    /// Returns the duration of one frame, including vertical blanking, in seconds.
    #[must_use]
    pub fn frame_period_s(&self) -> f64 {
        f64::from(self.rows + self.vblank_lines) * self.line_period_s()
    }

    /// Returns the image row, top to bottom among the rows read, of stream row `line`.
    ///
    /// # Panics
    ///
    /// Panics if `line` is not below [`rows`](Self::rows).
    #[must_use]
    pub fn image_row(&self, line: u32) -> u32 {
        assert!(
            line < self.rows,
            "stream row {line} is outside a readout of {} rows",
            self.rows
        );
        match self.direction {
            Direction::TopDown => line,
            Direction::BottomUp => self.rows - 1 - line,
        }
    }
}

impl Debug for Layout {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Layout")
            .field("rows", &self.rows)
            .field("cols", &self.cols)
            .field("ob_cols", &self.ob_cols)
            .field("hblank_px", &self.hblank_px)
            .field("vblank_lines", &self.vblank_lines)
            .field("pixel_period_s", &self.pixel_period_s)
            .field("line_skip", &self.line_skip)
            .field("direction", &self.direction)
            .finish_non_exhaustive()
    }
}

/// Charge samples in readout order, in electrons.
#[derive(Clone, PartialEq)]
pub struct Stream {
    samples: Box<[f32]>,
    layout: Layout,
}

impl Stream {
    /// Creates a stream of zero samples with the given layout.
    #[must_use]
    pub fn new(layout: Layout) -> Self {
        Self {
            samples: vec![0.0; layout.sample_count()].into_boxed_slice(),
            layout,
        }
    }

    /// Returns all samples, line by line.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Returns all samples mutably.
    pub fn samples_mut(&mut self) -> &mut [f32] {
        &mut self.samples
    }

    /// Returns the geometry and timing of the stream.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
}

impl Debug for Stream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream")
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

/// Parameters of the vertical and horizontal transfer.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Geometry and timing of the produced stream.
    pub layout: Layout,
    /// Seed of the readout's noise sources.
    pub seed: u64,
}

impl Params {
    /// Resolves the readout parameters from the camera, preset and derived timing.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let direction = camera.clocking(timing.mode).direction;
        Ok(Self {
            layout: Layout::from_timing(timing, direction),
            seed: noise::stage_seed(preset.seed, SALT),
        })
    }
}

/// Persistent state of the transfer clocks between frames.
///
/// The lossless transfer leaves nothing behind in the registers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct State;

/// Reads `charge` out of the array into the stream `out` for frame `index`.
///
/// Each line receives `ob_cols` optical-black samples of zero electrons, then the
/// active photosites of the sensor row given by the row map.
///
/// # Panics
///
/// Panics if `out` does not have the layout in `params`, or if the layout reads
/// rows or columns outside `charge`.
pub fn read(params: &Params, state: &mut State, charge: &Charge, index: u64, out: &mut Stream) {
    let _ = (state, index);
    let layout = &params.layout;
    assert!(
        out.layout == *layout,
        "stream layout {:?} does not match the readout layout {layout:?}",
        out.layout
    );
    let plane = charge.plane();
    assert!(
        layout.cols <= plane.width(),
        "readout of {} columns exceeds a {}-column sensor",
        layout.cols,
        plane.width()
    );
    let ob = layout.ob_cols as usize;
    let cols = layout.cols as usize;
    for (line, &row) in out
        .samples
        .chunks_exact_mut(layout.line_len() as usize)
        .zip(layout.row_map.iter())
    {
        let (black, active) = line.split_at_mut(ob);
        black.fill(0.0);
        active.copy_from_slice(&plane.row(row)[..cols]);
    }
}
