//! Processor capture: the bus words as the processor's own clock latches them.
//!
//! # Place in the chain
//!
//! Scene, sensor, readout, power, AFE, ADC, bus bend, then **[`reframe`]**, then the
//! black clamp, memory, development and codec. [`reframe`] rewrites the ADC words in
//! place, between the bus bend and the processor's black clamp.
//!
//! # Model
//!
//! A preset's `[bend.clock]` replaces the oscillator of the timing generator (TG) and
//! the AFE with one at `osc_mhz` (the LTC1799 mod), while the processor keeps its own
//! crystal at the camera's nominal pixel clock, generates vertical sync (VD) and latches
//! the bus once per period of its own clock
//! ([`Timing::dsp_pixel_period_s`](crate::Timing::dsp_pixel_period_s)). The readout,
//! AFE, ADC and bus bend upstream have run on the TG's clock
//! ([`Timing::pixel_period_s`](crate::Timing::pixel_period_s)), so the [`Codes`] hold
//! the bus as the TG drove it: word `codes[n][m]` during the `m`-th period of the TG's
//! line `n`. [`reframe`] replaces them by the words the processor latches.
//!
//! Below, `u` counts periods of the processor's clock from VD and `P` periods of the
//! TG's clock from VD. A line lasts `L = ob_cols + cols + hblank_px` periods on either
//! clock, the processor's horizontal blanking being the nominal count, and its first
//! `S = ob_cols + cols` periods are stored samples.
//!
//! **Latch instants.** The processor latches stored sample `j` of its line `k` at
//!
//! > `u = u_k + j + latch_phase`,
//!
//! where its first line starts at VD, `u_0 = 0`, and each later one at
//! `u_k = u_(k-1) + L + d_k`; `d_k` is the re-phasing of HD re-synchronisation (below),
//! 0 while the processor runs free. So `u_k = k L + o_k` with the persistent offset
//! `o_k = d_1 + ... + d_k`.
//!
//! **The TG's position.** VD restarts the TG with the processor's frame, so `P(0) = 0`,
//! and `P(u)` is the integral from 0 to `u` of `ratio`, the TG's frequency over the
//! processor's. The ratio is evaluated once per processor line, at its start, and held
//! over the line, so `P` is continuous and piecewise linear:
//! `P = P_k + ratio_k (u - u_k)` from `P_k = P(u_k)`.
//!
//! **The word valid then.** At `P` the TG is in its line `n = ⌊P / L⌋`, period
//! `m = ⌊P⌋ - n L`. The processor latches `codes[n][m]` if `m < S` and the TG has read
//! line `n` (`n < rows_read`). Otherwise the TG is in horizontal blanking, in vertical
//! blanking or past its last line waiting for VD, and the bus carries the AFE's pedestal,
//! the blanking word: the camera's `[adc] black_level` code. A latch at or after the
//! processor's next VD (`u` at least the frame period) finds the TG restarted for the
//! next frame; those latches are lost and keep the blanking word, so a TG that runs
//! slow never delivers its last lines.
//!
//! **HD re-synchronisation.** At the start of each line the processor looks for the TG
//! line start (HD, where `P` is a multiple of `L`) nearest to its own nominal start
//! `u_(k-1) + L`, among the TG line starts after its previous line began. If that HD lies
//! within `hd_lock_px` periods, the processor resets its line counter to it: `d_k` is
//! the distance, positive when the HD comes late, and `P_k` is the TG's line start. The
//! offset persists. This is a counter reset, not a loop filter: nothing pulls the phase
//! back gradually, and a phase that slips by more than the window per line is caught
//! only when it wraps round into the window. A window of 0 disables it; VD resets the
//! phase.
//!
//! **The oscillator.** Its frequency is
//!
//! > `f = osc_hz (1 + 1e-6 (drift_ppm w(t) + ppm_per_v (v_rail - rail_v)))`,
//!
//! at least [`MIN_RATIO`] of the nominal, and `ratio = f / f_nominal`
//! ([`Params::oscillator_hz`]). The drift `w(t)` is the mean of two incommensurate sines,
//!
//! > `w(t) = (sin 2π(c_1 + f_1 t) + sin 2π(c_2 + f_2 t)) / 2`,
//!
//! with `f_1 = drift_hz` and `f_2` the golden ratio times it ([`DRIFT_RATIO`]), so
//! `|w| <= 1`, `drift_ppm` is the peak wander, and the sum wanders without repeating over
//! any span a user watches. The phases start at offsets seeded from the preset seed and
//! run on in real time from frame to frame ([`State`]). The supply term reads the
//! camera's nominal `[power] rail_v`: the capture does not yet take the
//! [`power`](crate::power) stage's rail, so `ppm_per_v` has no effect.
//!
//! **Bayer parity.** Nothing corrects the mosaic. Where the TG's period `m` and the
//! processor's sample `j` differ in parity, or the TG's line `n` and the processor's
//! line `k`, a word lands in another colour channel, and the image processor develops
//! it as colour-phase static. That is the physics of the bend, not a separate effect.
//!
//! # What it looks like
//!
//! A free-running oscillator off by `ratio - 1` shifts each line by `(ratio - 1) L`
//! periods against the last: content stretches or compresses along the line, the slip
//! accumulates into skew, and every `1 / |ratio - 1|` lines it wraps a whole line, a
//! diagonal tear with a band of blanking, so the picture rolls down the frame by
//! `(ratio - 1)` of the lines read. VD restarts the TG, so a steady oscillator draws the
//! same tears in every frame; the drift moves them from frame to frame. A window wider
//! than the slip per line holds every line to a TG line start, removing the skew; a
//! narrower one catches the phase only as it wraps past, a snap back after each stretch
//! of skew. Far below nominal with a wide window, the processor waits for each slow TG
//! line, so it shows the start of each line stretched and loses the bottom of the frame
//! to VD.
//!
//! # Resolution
//!
//! The line's structure sets the look: how much of a slipped line falls into blanking
//! and how many lines a slip takes to wrap. A match-input line keeps the native
//! active-line time while one line covers several native lines of the frame, so it is
//! mostly horizontal blanking (82 % at 640 by 480, against 35 % on the A520's still
//! line); run on that line, the bend would look different at every size. The capture
//! therefore runs on the camera's native line and clock and samples the result: the
//! virtual line `k` latches as the native line at its centre, `(k + 1/2) R / r` (`R` and
//! `r` the native and virtual rows read), and the virtual sample `j` at the native
//! position `(j + latch_phase) B / b` in optical black and `B + (j - b + latch_phase) W /
//! w` among the active samples (`B`, `b` the optical-black counts, `W`, `w` the active
//! ones). The TG's native position maps back the same way, the line's fractional part
//! kept. At native resolution each map is the identity and the model is the one above;
//! at match-input the bend shows its native look, downscaled. `hd_lock_px` counts native
//! periods, and the drift and supply act in real time, so none needs scaling.
//!
//! # Approximations
//!
//! - The bus bend's circuit ([`bus`](crate::bus)) latches its lines at `latch_phase` of
//!   the TG's period, the source-period phase, not the processor's latch instant. The
//!   capture then takes whole bus words, so where the processor would catch a line
//!   mid-transition between two TG words, it reads the word the bus bend latched.
//! - The ratio holds over each processor line; drift and supply move over many lines.
//! - An HD inside the window before the processor's nominal line end starts the next
//!   line early; the earlier line's latches that fall after it still read the bus.
//! - The blanking word is the nominal black code, without the AFE's noise.
//!
//! # State
//!
//! The drift's two phases persist from frame to frame and through
//! [`Simulator::set_preset`](crate::Simulator::set_preset), so a new frequency or
//! drift rate continues the wander. [`State`] also holds the frame's working buffers:
//! a copy of the TG's words and the start of every native processor line.
//!
//! A steady oscillator at exactly the nominal frequency latches every word where the TG
//! put it, at every resolution, so [`reframe`] leaves the words alone then.

use std::f64::consts::TAU;
use std::fmt::{self, Debug, Formatter};

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use crate::adc::Codes;
use crate::noise;
use crate::preset::ReclockBend;
use crate::readout::Layout;
use crate::{Camera, Error, Preset, Timing};

/// Salt of the capture's seeded values: the drift's starting phases.
const SALT: u32 = noise::salt("capture");

/// Frequency of the drift's second sine over its first: the golden ratio.
///
/// The irrational number worst approximated by fractions, so the two sines never come
/// close to a short common period and their mean does not visibly repeat.
pub const DRIFT_RATIO: f64 = std::f64::consts::GOLDEN_RATIO;

/// Slowest the timing generator runs, as a fraction of the nominal clock.
///
/// A drift or supply excursion past `-1e6` ppm would stop or reverse the oscillator,
/// which no oscillator does. At a thousandth of the nominal one TG line lasts a
/// thousand processor lines, longer than any frame, so the floor changes no picture and
/// keeps the TG's position increasing.
pub const MIN_RATIO: f64 = 1e-3;

/// The resolved capture, present while the preset has a `[bend.clock]` section.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Frequency of the replacement oscillator in hertz.
    pub osc_hz: f64,
    /// The camera's nominal pixel clock of the readout mode in hertz.
    pub nominal_hz: f64,
    /// Duration of the processor's capture period in seconds.
    pub dsp_pixel_period_s: f64,
    /// The preset's `[bend.clock]` section as written.
    pub bend: ReclockBend,
    /// The camera's nominal supply rail in volts, from which `ppm_per_v` counts.
    pub rail_v: f64,
    /// The word on the bus while the timing generator is in blanking: the AFE's pedestal.
    pub blank_code: u16,
    /// Starting phases of the drift's two sines in cycles, seeded.
    drift_offset: [f64; 2],
    /// The camera's own readout line, on which the capture runs.
    native: Grid,
    /// The virtual readout line of the codes.
    sensor: Grid,
    /// How the virtual readout samples the native one.
    map: Map,
    /// Duration of one period of the camera's own clock, the capture's unit, in seconds.
    native_pixel_period_s: f64,
    /// Duration of the processor's frame, VD to VD, in seconds.
    frame_period_s: f64,
    /// The processor's frame in native periods: latches at or beyond it are lost.
    frame_periods: f64,
    /// Duration of the timing generator's pixel period, the codes' period, in seconds.
    pixel_period_s: f64,
}

/// How the virtual readout samples the native one; identities at native resolution.
#[derive(Debug, Clone, PartialEq)]
struct Map {
    /// Native position of each virtual sample's latch, in periods from the line start.
    latch_x: Box<[f64]>,
    /// Native lines per virtual line read.
    row_scale: f64,
    /// Virtual optical-black samples per native one.
    ob_per_native: f64,
    /// Virtual active samples per native one.
    cols_per_native: f64,
}

impl Map {
    /// Maps `sensor` onto `native`, latching at `latch_phase` of each virtual period.
    fn new(native: Grid, sensor: Grid, latch_phase: f64) -> Self {
        let ob_scale = f64::from(native.ob_cols) / f64::from(sensor.ob_cols.max(1));
        let col_scale = f64::from(native.cols) / f64::from(sensor.cols.max(1));
        let latch_x = (0..sensor.stored())
            .map(|j| {
                if j < sensor.ob_cols {
                    (f64::from(j) + latch_phase) * ob_scale
                } else {
                    f64::from(native.ob_cols)
                        + (f64::from(j - sensor.ob_cols) + latch_phase) * col_scale
                }
            })
            .collect();
        Self {
            latch_x,
            row_scale: f64::from(native.rows) / f64::from(sensor.rows.max(1)),
            ob_per_native: f64::from(sensor.ob_cols) / f64::from(native.ob_cols.max(1)),
            cols_per_native: f64::from(sensor.cols) / f64::from(native.cols.max(1)),
        }
    }
}

/// The counts of one readout line and frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Grid {
    /// Lines read per frame.
    rows: u32,
    /// Optical-black samples at the start of each line.
    ob_cols: u32,
    /// Active samples per line.
    cols: u32,
    /// Periods per line: stored samples and horizontal blanking.
    line_len: u32,
}

impl Grid {
    /// Returns the stored samples per line: optical black then active.
    fn stored(self) -> u32 {
        self.ob_cols + self.cols
    }
}

impl Params {
    /// Resolves the capture from the preset's `[bend.clock]` section; `None` without one.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value of the section is out of range.
    pub fn resolve(
        camera: &Camera,
        preset: &Preset,
        timing: &Timing,
    ) -> Result<Option<Self>, Error> {
        let Some(bend) = preset.bend.clock else {
            return Ok(None);
        };
        bend.validate()?;
        let clock = camera.clocking(timing.mode);
        let sensor = &camera.sensor;
        let native_rows = sensor.native_height / (2 * clock.line_skip.max(1)) * 2;
        let native = Grid {
            rows: native_rows,
            ob_cols: sensor.ob_cols,
            cols: sensor.native_width,
            line_len: sensor.ob_cols + sensor.native_width + clock.hblank_px,
        };
        let virtual_grid = Grid {
            rows: timing.rows_read,
            ob_cols: timing.ob_cols,
            cols: timing.cols,
            line_len: timing.line_len(),
        };
        let seed = noise::stage_seed(preset.seed, SALT);
        Ok(Some(Self {
            osc_hz: bend.osc_mhz * 1e6,
            nominal_hz: clock.pixel_clock_hz,
            dsp_pixel_period_s: timing.dsp_pixel_period_s,
            bend,
            rail_v: camera.power.rail_v,
            blank_code: camera.adc.black_level,
            drift_offset: [
                f64::from(noise::unit(0, 0, SALT, seed)),
                f64::from(noise::unit(1, 0, SALT, seed)),
            ],
            native,
            sensor: virtual_grid,
            map: Map::new(native, virtual_grid, camera.adc.latch_phase),
            native_pixel_period_s: timing.native_pixel_period_s,
            frame_period_s: timing.frame_period_s,
            frame_periods: timing.frame_period_s / timing.native_pixel_period_s,
            pixel_period_s: timing.pixel_period_s,
        }))
    }

    /// Returns the replacement oscillator's frequency over the nominal pixel clock.
    #[must_use]
    pub fn clock_ratio(&self) -> f64 {
        self.osc_hz / self.nominal_hz
    }

    /// Returns the oscillator's frequency in hertz at a supply voltage and a drift.
    ///
    /// `osc_hz (1 + 1e-6 (drift_ppm drift + ppm_per_v (rail_v - nominal)))`, with
    /// `drift` the normalised wander in `[-1, 1]` and the nominal rail
    /// [`Params::rail_v`]; never below [`MIN_RATIO`] of the nominal pixel clock.
    #[must_use]
    pub fn oscillator_hz(&self, rail_v: f64, drift: f64) -> f64 {
        let ppm = self.bend.drift_ppm * drift + self.bend.ppm_per_v * (rail_v - self.rail_v);
        (self.osc_hz * (1.0 + 1e-6 * ppm)).max(MIN_RATIO * self.nominal_hz)
    }

    /// Returns whether the oscillator runs at exactly the nominal frequency all the time.
    ///
    /// No drift, and the rail at its nominal voltage, as it is until the capture
    /// reads the power stage's rail.
    fn steady(&self) -> bool {
        self.osc_hz.to_bits() == self.nominal_hz.to_bits() && self.bend.drift_ppm == 0.0
    }

    /// Returns the drift `w(t)` in `[-1, 1]`, `t_s` seconds after VD, from the phases at VD.
    fn drift(&self, cycles: [f64; 2], t_s: f64) -> f64 {
        let f1_hz = self.bend.drift_hz;
        let first = cycles[0] + self.drift_offset[0] + f1_hz * t_s;
        let second = cycles[1] + self.drift_offset[1] + f1_hz * DRIFT_RATIO * t_s;
        f64::midpoint((TAU * first).sin(), (TAU * second).sin())
    }

    /// Returns the TG's frequency over the processor's, `u` native periods after VD.
    fn ratio_at(&self, cycles: [f64; 2], u: f64) -> f64 {
        let drift = self.drift(cycles, u * self.native_pixel_period_s);
        self.oscillator_hz(self.rail_v, drift) / self.nominal_hz
    }

    /// Finds where every native processor line of the frame starts, into `starts`.
    fn plan(&self, cycles: [f64; 2], starts: &mut Vec<LineStart>) {
        let line = f64::from(self.native.line_len);
        starts.clear();
        let mut start = LineStart {
            u: 0.0,
            p: 0.0,
            ratio: self.ratio_at(cycles, 0.0),
        };
        starts.push(start);
        for _ in 1..self.native.rows {
            let (u, p) = self.resync(start, line);
            start = LineStart {
                u,
                p,
                ratio: self.ratio_at(cycles, u),
            };
            starts.push(start);
        }
    }

    /// Returns the start of the processor line after `prev` and the TG's position there.
    ///
    /// The nominal start is one line after `prev`; with a capture window the line
    /// starts at the nearest later TG line start instead if it lies within the window.
    fn resync(&self, prev: LineStart, line: f64) -> (f64, f64) {
        let u_nominal = prev.u + line;
        let p_nominal = prev.p + prev.ratio * line;
        let window = self.bend.hd_lock_px;
        if window <= 0.0 {
            return (u_nominal, p_nominal);
        }
        let early_p = (p_nominal / line).floor() * line;
        let late_p = early_p + line;
        let late = (late_p, (late_p - p_nominal) / prev.ratio);
        // The early candidate counts only if it came after the previous line began.
        let (hd_p, shift) = if early_p > prev.p {
            let early = (early_p, (early_p - p_nominal) / prev.ratio);
            if -early.1 <= late.1 { early } else { late }
        } else {
            late
        };
        if shift.abs() <= window {
            (u_nominal + shift, hd_p)
        } else {
            (u_nominal, p_nominal)
        }
    }

    /// Fills virtual line `line` with the words its latches find in `source`.
    fn fill(&self, line: usize, starts: &[LineStart], source: &[u16], out: &mut [u16]) {
        // The native line at the centre of this one, and how far into it that centre lies.
        let centre = (line as f64 + 0.5) * self.map.row_scale;
        let native_line = (centre as usize).min(starts.len() - 1);
        let into = centre - native_line as f64;
        let start = starts[native_line];
        let line_len = f64::from(self.native.line_len);
        let native_stored = f64::from(self.native.stored());
        let before_vd = self.frame_periods - start.u;
        // The TG line under the latches, as the TG positions where it starts and ends
        // and the offset of its words in `source`; found again only when a latch leaves it.
        let mut tg_line = (f64::INFINITY, f64::NEG_INFINITY, None);
        for (word, &x) in out.iter_mut().zip(&self.map.latch_x) {
            if x >= before_vd {
                *word = self.blank_code;
                continue;
            }
            let p = start.p + start.ratio * x;
            if !(tg_line.0..tg_line.1).contains(&p) {
                let n = (p / line_len).floor();
                tg_line = (
                    n * line_len,
                    (n + 1.0) * line_len,
                    self.source_line(n, into),
                );
            }
            let m = p - tg_line.0;
            *word = match tg_line.2 {
                Some(offset) if m < native_stored => source[offset + self.source_col(m)],
                _ => self.blank_code,
            };
        }
    }

    /// Returns where the words of the TG's native line `n` start in the source; `None` if unread.
    ///
    /// `into` is the fraction of a native line by which the virtual line's centre lies
    /// past its native line's start; the TG's line keeps it on the way back.
    fn source_line(&self, n: f64, into: f64) -> Option<usize> {
        (n < f64::from(self.native.rows)).then(|| {
            let row = ((n + into) / self.map.row_scale) as usize;
            row.min(self.sensor.rows as usize - 1) * self.sensor.stored() as usize
        })
    }

    /// Returns the virtual sample of the TG's stored native period `m`.
    fn source_col(&self, m: f64) -> usize {
        let native_ob = f64::from(self.native.ob_cols);
        if m < native_ob {
            ((m * self.map.ob_per_native) as usize)
                .min(self.sensor.ob_cols.saturating_sub(1) as usize)
        } else {
            let active = ((m - native_ob) * self.map.cols_per_native) as usize;
            (self.sensor.ob_cols as usize + active).min(self.sensor.stored() as usize - 1)
        }
    }

    /// Panics unless `layout` is the virtual readout these parameters were resolved for.
    fn check(&self, layout: &Layout) {
        let grid = Grid {
            rows: layout.rows(),
            ob_cols: layout.ob_cols(),
            cols: layout.cols(),
            line_len: layout.line_len() + layout.hblank_px(),
        };
        assert!(
            grid == self.sensor,
            "codes of layout {layout:?} do not match the capture's readout {:?}",
            self.sensor
        );
        assert!(
            (layout.pixel_period_s() / self.pixel_period_s - 1.0).abs() < 1e-9,
            "codes of pixel period {} s do not match the capture's sensor clock of {} s",
            layout.pixel_period_s(),
            self.pixel_period_s
        );
    }
}

/// Where a processor line starts: its time and the TG's position, and the ratio over it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LineStart {
    /// Native processor periods from VD to the line's start.
    u: f64,
    /// Native TG periods from VD to the line's start.
    p: f64,
    /// The TG's frequency over the processor's, held over the line.
    ratio: f64,
}

/// The capture's memory from frame to frame; the default is the power-on state.
///
/// The hardware's memory is the drift oscillator's phase: the phases of its two sines,
/// which run on in real time across frames. The state also holds working buffers that
/// carry nothing between frames and take no part in equality.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct State {
    /// Phases of the drift's two sines at the next VD, in cycles in `[0, 1)`.
    drift_cycles: [f64; 2],
    /// The words as the TG drove them, copied before the frame is rewritten.
    source: Vec<u16>,
    /// The start of every native processor line of the frame.
    starts: Vec<LineStart>,
}

impl State {
    /// Returns the phases of the drift's two sines at the next VD, in cycles in `[0, 1)`.
    ///
    /// Counted from power-on, before the seeded starting phases are added; they advance
    /// by the drift's frequencies times the processor's frame period every frame.
    #[must_use]
    pub fn drift_cycles(&self) -> [f64; 2] {
        self.drift_cycles
    }

    /// Advances the drift's phases by one processor frame.
    fn advance(&mut self, params: &Params) {
        let f1_hz = params.bend.drift_hz;
        let periods = [
            f1_hz * params.frame_period_s,
            f1_hz * DRIFT_RATIO * params.frame_period_s,
        ];
        for (cycles, step) in self.drift_cycles.iter_mut().zip(periods) {
            *cycles = (*cycles + step).rem_euclid(1.0);
        }
    }
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.drift_cycles == other.drift_cycles
    }
}

impl Debug for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("State")
            .field("drift_cycles", &self.drift_cycles)
            .finish_non_exhaustive()
    }
}

/// Rewrites `codes` in place with the words the processor captures in frame `index`.
///
/// Without a bend (`params` is `None`) it does nothing. With one, each processor latch
/// takes the word the timing generator drove at that instant, or the blanking word, as
/// the [module documentation](self) describes, and the drift's phases advance by one
/// frame. The capture draws no noise; `index` only labels the frame's trace event.
///
/// # Panics
///
/// Panics if `codes` does not have the readout the parameters were resolved for.
pub fn reframe(params: Option<&Params>, state: &mut State, index: u64, codes: &mut Codes) {
    let Some(params) = params else {
        return;
    };
    params.check(codes.layout());
    if !params.steady() {
        state.source.clear();
        state.source.extend_from_slice(codes.words());
        params.plan(state.drift_cycles, &mut state.starts);
        let (starts, source) = (&state.starts, &state.source);
        codes
            .words_mut()
            .par_chunks_exact_mut(params.sensor.stored() as usize)
            .enumerate()
            .for_each(|(line, words)| params.fill(line, starts, source, words));
    }
    tracing::trace!(
        name: "capture.reframe.done",
        index,
        ratio_at_vd = params.ratio_at(state.drift_cycles, 0.0),
        "frame captured"
    );
    state.advance(params);
}
