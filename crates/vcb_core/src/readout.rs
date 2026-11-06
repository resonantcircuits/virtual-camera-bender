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
//! Bayer phase. [`Direction`] says which end of the array is read first.
//!
//! # Physical model
//!
//! **Optical black.** Masked photosites collect only dark current during the
//! exposure, so every optical-black sample carries [`Params::dark_e`]. Its noise is
//! added by the analog front end.
//!
//! **Charge-transfer efficiency** (`docs/research/a520-technical.md`, section 4). A
//! vertical transfer moves a fraction `cte` of a packet and leaves `1 - cte` behind.
//! A packet that crosses `N` transfers keeps `cte^N` in its own line; the deferred
//! remainder follows a negative binomial distribution whose mean delay is
//! `N (1 - cte) / cte` lines. The model keeps one reservoir of deferred charge per
//! column and walks the lines in readout order: each line deposits
//! `packet * (1 - cte^N)` into the reservoir, which releases the fraction
//! `r = 1 / (1 + N (1 - cte) / cte)` of its content into every later line. Deferred
//! charge therefore arrives from the next line on and decays geometrically, with a
//! mean of `N (1 - cte) / cte` lines beyond the first. This geometric tail matches the
//! binomial trail to first order in `N (1 - cte)`, where nearly all deferred charge
//! lands in the next line; for `N (1 - cte)` above one it keeps the mean but is less
//! peaked than the true distribution. The release fraction of the line being read
//! applies to the whole reservoir, which is exact for neighbouring rows because their
//! transfer counts differ by one.
//!
//! `N` counts the transfers of the full-size sensor: a row at a fraction `f` of the
//! virtual array has crossed `f` of the native rows, so the loss, and the trail's
//! extent as a fraction of the frame, do not depend on the virtual sensor's size.
//! Charge still deferred after the last line drains during vertical blanking and is
//! lost; [`Stream::residual`] reports it. The horizontal transfer is lossless.
//!
//! **Vertical clock faults** (`docs/research/circuit-bend-reference.md`, sensor timing
//! paths and the vertical-melt and buffer-stall entries of the modification table). A
//! fault starts in a line period with probability `v_pulse_drop_hz * line_period_s`,
//! drawn from a hash of the line, the stage seed and the frame index, while no other
//! fault runs. [`STALL_SHARE`] of the faults are stalls: the row pointer holds, so each
//! line repeats the previous line's row (a stall in the first line reads an empty
//! register). The others are double pulses: the row pointer advances by two rows per
//! line, so rows are skipped. Durations are geometric with a mean of `v_stall_lines`
//! native lines, scaled to the virtual sensor's rows. The image slips by the number
//! of repeated or skipped rows for the rest of the frame; an odd slip flips the Bayer
//! row phase, which the image processor develops as a colour swap. Rows pushed past
//! the last line are not read, and lines read past the last row hold no charge.
//!
//! **Horizontal register glitches** (`docs/research/circuit-bend-reference.md`,
//! horizontal clock delay). A band of lines starts with probability
//! `h_glitch_hz * line_period_s` per line. Its thickness follows the reference's
//! bimodal rule ([`THIN_SHARE`] of the bands are [`THIN_BAND_LINES`] thick, the rest
//! [`THICK_BAND_LINES`], in native lines scaled to the virtual sensor), and its
//! active samples come out shifted by a whole number of samples, uniform within
//! `h_shift_frac` of the line either way, with wraparound. An odd shift swaps the
//! Bayer column phase.
//!
//! Faults of both kinds run through vertical blanking and across frame boundaries;
//! [`State`] carries the unfinished ones to the next frame. Fault rates span lines
//! and frames and are used as given, since match-input timing keeps the frame period
//! native; durations and band thicknesses are native lines scaled to the virtual
//! sensor, so a fault lasts the same time at every resolution.

use std::fmt::{self, Debug, Formatter};

use crate::noise;
use crate::preset::Mode;
use crate::sensor::{self, Charge};
use crate::{Camera, Error, Preset, Timing};

/// Salt of the readout's noise sources.
const SALT: u32 = noise::salt("readout");

/// Share of vertical clock faults that are stalls; the rest are double pulses.
///
/// A starved or loaded clock line misses pulses more often than it adds them.
/// PROVISIONAL: the split is a modelling choice, not a measurement.
pub const STALL_SHARE: f32 = 0.7;

/// Share of horizontal glitch bands that are thin.
///
/// From the bimodal band rule in `docs/research/circuit-bend-reference.md`.
pub const THIN_SHARE: f32 = 0.6;

/// Range of thin glitch bands in native lines, inclusive.
///
/// From the bimodal band rule in `docs/research/circuit-bend-reference.md`.
pub const THIN_BAND_LINES: (u32, u32) = (2, 4);

/// Range of thick glitch bands in native lines, inclusive.
///
/// From the bimodal band rule in `docs/research/circuit-bend-reference.md`.
pub const THICK_BAND_LINES: (u32, u32) = (8, 22);

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

    /// Returns the sensor row each stream row reads when the clocks run cleanly.
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
    ///
    /// The timing generator's own frame on the layout's clock. Without `[bend.clock]`
    /// it is the processor's frame; under a reclocked oscillator VD ends the frame at
    /// [`Timing::frame_period_s`](crate::Timing::frame_period_s) instead, cutting a slow
    /// generator short and stretching a fast one's vertical blanking.
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

    /// Returns whether two layouts store the same lines in the same order, whatever their clock.
    ///
    /// Everything but the pixel period must agree: lines, columns, optical black,
    /// blanking periods, line skip, direction and the map from lines to rows.
    pub(crate) fn same_shape(&self, other: &Self) -> bool {
        let retimed = Self {
            pixel_period_s: self.pixel_period_s,
            ..other.clone()
        };
        retimed == *self
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
    residual: Box<[f32]>,
    layout: Layout,
}

impl Stream {
    /// Creates a stream of zero samples with the given layout.
    #[must_use]
    pub fn new(layout: Layout) -> Self {
        Self {
            samples: vec![0.0; layout.sample_count()].into_boxed_slice(),
            residual: vec![0.0; layout.cols as usize].into_boxed_slice(),
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

    /// Returns the deferred charge per active column left after the last line, in electrons.
    ///
    /// Transfer losses trail behind their packets; what has not reached the register
    /// when the last line is read drains during vertical blanking and never reaches
    /// the output. Together with the active samples it accounts for all charge read.
    #[must_use]
    pub fn residual(&self) -> &[f32] {
        &self.residual
    }

    /// Returns the geometry and timing of the stream.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Gives the stream the timing of `layout`, keeping its samples.
    ///
    /// # Panics
    ///
    /// Panics if `layout` differs from the stream's own in anything but its pixel period.
    pub(crate) fn retime(&mut self, layout: Layout) {
        assert!(
            self.layout.same_shape(&layout),
            "cannot retime a stream of layout {:?} to {layout:?}: only the clock may change",
            self.layout
        );
        self.layout = layout;
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
    /// Dark charge of an optical-black photosite: dark current times exposure, in electrons.
    ///
    /// The masked photosites share the array's temperature, so the dark current is the
    /// sensor's at the `[bend.sensor]` temperature (see [`sensor`]).
    pub dark_e: f32,
    /// Charge transfer efficiency of one vertical transfer.
    pub cte: f64,
    /// Rows of the virtual sensor, read or skipped.
    pub sensor_height: u32,
    /// Rows of the full-size sensor; transfer counts are expressed in its rows.
    pub native_height: u32,
    /// Virtual lines per native line of the readout mode, for durations in native lines.
    pub line_scale: f64,
    /// Mean rate of vertical clock faults per second of readout.
    pub v_pulse_drop_hz: f64,
    /// Mean duration of a vertical clock fault in native lines.
    pub v_stall_lines: f64,
    /// Mean rate of horizontal register glitch bands per second of readout.
    pub h_glitch_hz: f64,
    /// Largest register shift of a glitch band as a fraction of the active line.
    pub h_shift_frac: f64,
    /// Seed of the readout's noise sources.
    pub seed: u64,
}

impl Params {
    /// Resolves the readout parameters from the camera, preset and derived timing.
    ///
    /// The preset's `[bend.clocks] cte` replaces the sensor's own when set. The
    /// optical-black photosites integrate dark current over the same exposure as the
    /// image's photosites: the preset's `[sensor] exposure_s` or else the mode's camera
    /// exposure, which the movie mode's electronic shutter (it clears every photosite,
    /// masked ones included) limits to one frame period; the dark current is the
    /// sensor's at the preset's `[bend.sensor] temperature_c`.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let direction = camera.clocking(timing.mode).direction;
        let exposure_s = match timing.mode {
            Mode::Still => preset
                .sensor
                .and_then(|o| o.exposure_s)
                .unwrap_or(camera.still.exposure_s),
            Mode::Movie => preset
                .sensor
                .and_then(|o| o.exposure_s)
                .unwrap_or(camera.movie.exposure_s)
                .min(1.0 / camera.movie.fps),
        };
        let clocks = preset.bend.clocks.clone().unwrap_or_default();
        let native_rows = camera.sensor.native_height / (2 * timing.line_skip.max(1)) * 2;
        Ok(Self {
            layout: Layout::from_timing(timing, direction),
            dark_e: (camera.sensor.dark_e_per_s * sensor::dark_factor(camera, preset) * exposure_s)
                as f32,
            cte: clocks.cte.unwrap_or(camera.sensor.cte),
            sensor_height: timing.sensor_height,
            native_height: camera.sensor.native_height,
            line_scale: f64::from(timing.rows_read) / f64::from(native_rows.max(1)),
            v_pulse_drop_hz: clocks.v_pulse_drop_hz,
            v_stall_lines: clocks.v_stall_lines,
            h_glitch_hz: clocks.h_glitch_hz,
            h_shift_frac: clocks.h_shift_frac,
            seed: noise::stage_seed(preset.seed, SALT),
        })
    }

    /// Returns the vertical transfers, in native rows, from sensor `row` into the register.
    fn transfers(&self, row: u32) -> f64 {
        let crossed = match self.layout.direction {
            Direction::TopDown => row + 1,
            Direction::BottomUp => self.sensor_height.saturating_sub(row),
        };
        f64::from(crossed) * f64::from(self.native_height) / f64::from(self.sensor_height.max(1))
    }
}

/// Persistent state of the transfer clocks between frames.
///
/// Holds the clock faults still running at the end of a frame, so a fault that
/// straddles the frame boundary continues into the next frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    vertical: Option<Fault>,
    horizontal: Option<Band>,
}

/// A running vertical clock fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fault {
    kind: FaultKind,
    lines_left: u32,
}

/// The two kinds of vertical clock fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaultKind {
    /// No pulse: the register receives no new row.
    Stall,
    /// Two pulses per line: the row pointer skips a row.
    DoublePulse,
}

/// A running horizontal glitch band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Band {
    shift: i64,
    lines_left: u32,
}

/// The independent random draws of one line period, used as the hash row.
#[derive(Debug, Clone, Copy)]
enum Draw {
    VStart,
    VKind,
    VLength,
    HStart,
    HClass,
    HLength,
    HShift,
}

/// Per-frame event clock: the fault probabilities per line and the frame's seed.
struct Clock<'a> {
    params: &'a Params,
    v_per_line: f64,
    h_per_line: f64,
    seed: u64,
}

impl Clock<'_> {
    fn unit(&self, line: u32, draw: Draw) -> f32 {
        noise::unit(line, draw as u32, SALT, self.seed)
    }

    /// Runs the vertical clock for one line period; returns the rows it shifts in.
    fn vertical(&self, state: &mut State, line: u32) -> u32 {
        if state.vertical.is_none()
            && self.v_per_line > 0.0
            && f64::from(self.unit(line, Draw::VStart)) < self.v_per_line
        {
            let kind = if self.unit(line, Draw::VKind) < STALL_SHARE {
                FaultKind::Stall
            } else {
                FaultKind::DoublePulse
            };
            let mean = self.params.v_stall_lines * self.params.line_scale;
            let lines_left = geometric(self.unit(line, Draw::VLength), mean);
            state.vertical = Some(Fault { kind, lines_left });
        }
        let Some(fault) = state.vertical.as_mut() else {
            return 1;
        };
        fault.lines_left -= 1;
        let rows = match fault.kind {
            FaultKind::Stall => 0,
            FaultKind::DoublePulse => 2,
        };
        if fault.lines_left == 0 {
            state.vertical = None;
        }
        rows
    }

    /// Runs the horizontal clock for one line period; returns the register shift in samples.
    fn horizontal(&self, state: &mut State, line: u32) -> i64 {
        if state.horizontal.is_none()
            && self.h_per_line > 0.0
            && f64::from(self.unit(line, Draw::HStart)) < self.h_per_line
        {
            let (low, high) = if self.unit(line, Draw::HClass) < THIN_SHARE {
                THIN_BAND_LINES
            } else {
                THICK_BAND_LINES
            };
            let span = f64::from(high - low + 1);
            let native =
                f64::from(low) + (f64::from(self.unit(line, Draw::HLength)) * span).floor();
            let lines_left = (native * self.params.line_scale).round().max(1.0) as u32;
            let reach = self.params.h_shift_frac * f64::from(self.params.layout.cols);
            let toss = noise::signed(line, Draw::HShift as u32, SALT, self.seed);
            let shift = (f64::from(toss) * reach).round() as i64;
            state.horizontal = Some(Band { shift, lines_left });
        }
        let Some(band) = state.horizontal.as_mut() else {
            return 0;
        };
        band.lines_left -= 1;
        let shift = band.shift;
        if band.lines_left == 0 {
            state.horizontal = None;
        }
        shift
    }
}

/// Draws a duration of at least one line, geometric with the given mean.
fn geometric(unit: f32, mean: f64) -> u32 {
    if mean <= 1.0 {
        return 1;
    }
    // Inverse CDF of the geometric distribution on {1, 2, ...} with end probability 1 / mean.
    let extra = f64::from(1.0 - unit).ln() / (1.0 - 1.0 / mean).ln();
    (extra.floor() as u32).saturating_add(1)
}

/// Moves one row of packets through the lossy vertical register into `out`.
///
/// Each packet keeps the fraction `keep`; the rest joins its column's reservoir in
/// `deferred`, which first releases the fraction `release` into this line.
fn transfer(packets: &[f32], keep: f32, release: f32, out: &mut [f32], deferred: &mut [f32]) {
    let lose = 1.0 - keep;
    let hold = 1.0 - release;
    for ((out, deferred), &packet) in out.iter_mut().zip(deferred.iter_mut()).zip(packets) {
        *out = packet * keep + release * *deferred;
        *deferred = *deferred * hold + packet * lose;
    }
}

/// Reads a line with no row in the register: only deferred charge arrives.
fn drain(release: f32, out: &mut [f32], deferred: &mut [f32]) {
    for (out, deferred) in out.iter_mut().zip(deferred.iter_mut()) {
        *out = release * *deferred;
        *deferred -= *out;
    }
}

/// Reads `charge` out of the array into the stream `out` for frame `index`.
///
/// Each line receives `ob_cols` optical-black samples of dark charge, then the active
/// photosites of the row in the register, after charge-transfer losses, vertical
/// clock faults and horizontal register glitches as described in the
/// [module documentation](self).
///
/// # Panics
///
/// Panics if `out` does not have the layout in `params`, or if the layout reads
/// rows or columns outside `charge`.
pub fn read(params: &Params, state: &mut State, charge: &Charge, index: u64, out: &mut Stream) {
    let layout = &params.layout;
    assert!(
        out.layout == *layout,
        "stream layout {:?} does not match the readout layout {layout:?}",
        out.layout
    );
    let plane = charge.plane();
    assert!(
        layout.cols <= plane.width() && params.sensor_height <= plane.height(),
        "readout of {} columns and {} rows exceeds a {} x {} sensor",
        layout.cols,
        params.sensor_height,
        plane.width(),
        plane.height()
    );
    let line_period_s = layout.line_period_s();
    let clock = Clock {
        params,
        v_per_line: params.v_pulse_drop_hz * line_period_s,
        h_per_line: params.h_glitch_hz * line_period_s,
        seed: params.seed ^ index,
    };
    let ob = layout.ob_cols as usize;
    let cols = layout.cols as usize;
    let lines = out.samples.chunks_exact_mut(layout.line_len() as usize);
    let residual = &mut out.residual;
    residual.fill(0.0);
    // Rows shifted into the horizontal register so far; the last one is read.
    let mut shifted: u32 = 0;
    let mut release = 1.0f32;
    for (line, samples) in (0..layout.rows).zip(lines) {
        shifted += clock.vertical(state, line);
        let (black, active) = samples.split_at_mut(ob);
        black.fill(params.dark_e);
        let row = shifted
            .checked_sub(1)
            .and_then(|pointer| layout.row_map.get(pointer as usize));
        if let Some(&row) = row {
            let transfers = params.transfers(row);
            let keep = params.cte.powf(transfers) as f32;
            release = (1.0 / (1.0 + transfers * (1.0 - params.cte) / params.cte)) as f32;
            transfer(&plane.row(row)[..cols], keep, release, active, residual);
        } else {
            drain(release, active, residual);
        }
        let shift = clock.horizontal(state, line);
        if shift != 0 {
            active.rotate_right(shift.rem_euclid(i64::from(layout.cols)) as usize);
        }
    }
    for line in layout.rows..layout.rows + layout.vblank_lines {
        clock.vertical(state, line);
        clock.horizontal(state, line);
    }
}
