//! The supply rail: the 3.3 V rail of the AFE, the bend's logic and the processor.
//!
//! # Place in the chain
//!
//! Scene, sensor, readout, then **[`track`]**, then the AFE, ADC, bus bend, capture,
//! black clamp, memory, development and codec. [`track`] fills a [`Rail`], the rail
//! voltage at the start and end of every readout line, before the AFE converts the
//! frame; [`afe::process`](crate::afe::process), [`bus::bend`] and
//! [`isp::clamp_black`](crate::isp::clamp_black) receive it, because the rail feeds
//! the AFE, the bend's 74HC logic and the processor's input stage.
//!
//! # Model
//!
//! Two AA cells feed DC-DC converters; one 3.3 V rail powers the AFE, the bus
//! drivers, the bend's chips and the processor's I/O (`docs/PLAN.md`, section 3.9,
//! "Power"). The camera's `[power]` section describes it as a source of EMF `E`
//! behind the regulator's and battery's output impedance `R = source_ohm` (the
//! preset's `[bend.rail] source_ohm` if set: a weak battery, or a converter at its
//! current limit), with the decoupling capacitance `C = decoupling_uf` across the
//! rail. The camera's `rail_v` is the rail at its own quiescent load `quiescent_ma`,
//! so `E = rail_v + R quiescent_ma` and only the current above the quiescent sags the
//! rail. With `V` the rail and `ΔI(t)` the added load,
//!
//! > `R C dV/dt = rail_v + e(t) - R ΔI(t) - V`, so `τ = R C`,
//!
//! where the added load is
//!
//! - `load_ma`, steady: LEDs, an added oscillator, a potentiometer to ground;
//! - `bus_ma(line)`, the mean current the bus bend drew from the rail on that readout
//!   line of the previous frame ([`bus::State::line_current_ma`]; 0 for a line the
//!   vector lacks, as before the bend's first frame);
//! - load spikes: arrivals of a Poisson process of rate `spike_hz` in camera time,
//!   drawn from a [`Pcg32`] seeded with the stage seed and the frame index, each
//!   switching `spike_ma` on for `spike_ms`. One transient load (a flash charger, a
//!   lens motor) either runs or does not, so an arrival during a spike extends it
//!   instead of adding to it, and a spike still running at the end of a frame runs on
//!   into the next;
//!
//! and `e(t) = ripple_mv sin(2π ripple_hz t + φ)` is the ripple of the converter or a
//! weak battery on the EMF, its phase running on from frame to frame.
//!
//! While the load holds, the deviation `d = V - rail_v` follows in closed form
//!
//! > `d(t) = -R ΔI + p(t) + (d(t0) + R ΔI - p(t0)) e^(-(t - t0) / τ)`,
//!
//! with `p(t) = ripple_mv / sqrt(1 + (ωτ)²) sin(ωt + φ - atan(ωτ))`, `ω = 2π
//! ripple_hz`, the ripple through the RC filter. Each line period and the vertical
//! blanking are split where a spike starts or ends and advanced exactly, so the rail is
//! exact at every stored instant, however long the line. Without spikes, ripple or
//! added load `d` stays exactly 0: the rail holds `rail_v` to the bit. The loads draw
//! their current down to 0 V and starve below it, so a load the source cannot carry
//! leaves the rail at 0 V instead of reversing it.
//!
//! # Indexing
//!
//! A [`Rail`] has one entry per readout line, in readout order (line 0 is read
//! first, whatever the readout direction), as [`Layout`] numbers them. Each entry
//! holds the voltage at the start of the line's first stored sample and at the end
//! of its horizontal blanking, the start of the next line period; within the line the
//! consumers take the voltage as linear between the two ([`Rail::v_at`]). Vertical
//! blanking has no entries. Ripple faster than about half the line rate is therefore
//! sampled, not resolved, by the consumers.
//!
//! # Consumers
//!
//! The AFE's reference and pedestal follow the rail ratiometrically, so codes above
//! black inflate as it sags; below `afe_uvlo_v` the AFE locks out and the bus holds
//! its last word ([`afe`](crate::afe)). The bus drivers, the bend's 74HC levels and
//! thresholds and the processor's input threshold scale with the rail, below
//! `logic_min_v` the 74HC chips go high-impedance, and the bend reports the current
//! it draws ([`bus`]). Below `dsp_brownout_v` at the start of a line the processor
//! captures nothing for the rest of the frame
//! ([`isp::clamp_black`](crate::isp::clamp_black)).
//!
//! # Time and resolution
//!
//! `R`, `C`, the spike rate and duration and the ripple frequency are physical and
//! used as given in camera time: match-input timing keeps the frame period native,
//! so a dip covers the same fraction of the frame at every resolution.
//!
//! # State
//!
//! [`State`] keeps the decoupling capacitor's voltage, the ripple's phase and what is
//! left of a spike in progress at the end of each frame's vertical blanking; the next
//! frame starts from them. The default is the power-on state: the capacitor settled at
//! `rail_v`, no spike running, the ripple at phase 0. Without `[bend.rail]` the rail
//! sits at the nominal voltage and [`track`] returns the state to that default.

use std::f64::consts::TAU;
use std::fmt::{self, Debug, Formatter};

use crate::noise::{self, Pcg32};
use crate::preset::RailBend;
use crate::readout::Layout;
use crate::{Camera, Error, Preset, Timing, bus};

/// Salt of the load-spike process.
const SALT: u32 = noise::salt("power");

/// Scale that maps a 32-bit integer onto `[0, 1)`.
const ONE_OVER_2_32: f64 = 1.0 / 4_294_967_296.0;

/// The rail voltage at the start and end of every readout line, in volts.
///
/// Allocated once from the readout [`Layout`] and filled at the camera's nominal
/// voltage; [`track`] rewrites it every frame while `[bend.rail]` is present. See the
/// [module documentation](self) for how it is indexed.
///
/// # Examples
///
/// ```
/// use vcb_core::power::Rail;
/// use vcb_core::readout::{Direction, Layout};
/// use vcb_core::{Camera, Preset, Timing};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
/// let camera: Camera = toml::from_str(&std::fs::read_to_string(path)?)?;
/// let timing = Timing::derive(&camera, &Preset::new("a520"), 2272, 1704)?;
/// let mut rail = Rail::new(&Layout::from_timing(&timing, Direction::TopDown), 3.3);
/// rail.set(10, 3.3, 3.1);
/// assert!((rail.v_at(10, 0.5) - 3.2).abs() < 1e-12);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, PartialEq)]
pub struct Rail {
    /// Start and end voltage of every readout line.
    lines_v: Box<[[f64; 2]]>,
    /// The camera's nominal rail voltage.
    nominal_v: f64,
}

impl Rail {
    /// Creates a rail at `nominal_v` volts on every line of `layout`.
    #[must_use]
    pub fn new(layout: &Layout, nominal_v: f64) -> Self {
        Self {
            lines_v: vec![[nominal_v; 2]; layout.rows() as usize].into_boxed_slice(),
            nominal_v,
        }
    }

    /// Returns the camera's nominal rail voltage in volts.
    #[must_use]
    pub fn nominal_v(&self) -> f64 {
        self.nominal_v
    }

    /// Returns the number of readout lines.
    #[must_use]
    pub fn lines(&self) -> u32 {
        self.lines_v.len() as u32
    }

    /// Returns the voltage at the start of readout line `line`, in volts.
    ///
    /// # Panics
    ///
    /// Panics if `line` is not below [`lines`](Self::lines).
    #[must_use]
    pub fn start_v(&self, line: u32) -> f64 {
        self.lines_v[line as usize][0]
    }

    /// Returns the voltage at the end of readout line `line`'s blanking, in volts.
    ///
    /// # Panics
    ///
    /// Panics if `line` is not below [`lines`](Self::lines).
    #[must_use]
    pub fn end_v(&self, line: u32) -> f64 {
        self.lines_v[line as usize][1]
    }

    /// Returns the voltage at fraction `frac` of readout line `line`'s period, in volts.
    ///
    /// `frac` 0 is the start of the line's first stored sample and 1 the end of its
    /// horizontal blanking; between them the voltage is
    /// `start_v + frac * (end_v - start_v)`.
    ///
    /// # Panics
    ///
    /// Panics if `line` is not below [`lines`](Self::lines).
    #[must_use]
    pub fn v_at(&self, line: u32, frac: f64) -> f64 {
        let [start_v, end_v] = self.lines_v[line as usize];
        start_v + frac * (end_v - start_v)
    }

    /// Sets the voltage at the start and end of readout line `line`, in volts.
    ///
    /// # Panics
    ///
    /// Panics if `line` is not below [`lines`](Self::lines).
    pub fn set(&mut self, line: u32, start_v: f64, end_v: f64) {
        self.lines_v[line as usize] = [start_v, end_v];
    }

    /// Returns the start and end voltage of every line, in readout order.
    #[must_use]
    pub fn lines_v(&self) -> &[[f64; 2]] {
        &self.lines_v
    }

    /// Returns the start and end voltage of every line mutably.
    pub fn lines_v_mut(&mut self) -> &mut [[f64; 2]] {
        &mut self.lines_v
    }

    /// Returns every line to the nominal voltage.
    pub fn reset(&mut self) {
        self.lines_v.fill([self.nominal_v; 2]);
    }

    /// Returns whether every line holds the nominal voltage.
    #[must_use]
    pub fn is_nominal(&self) -> bool {
        self.lines_v
            .iter()
            .flatten()
            .all(|&v| v.to_bits() == self.nominal_v.to_bits())
    }
}

impl Debug for Rail {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rail")
            .field("lines", &self.lines_v.len())
            .field("nominal_v", &self.nominal_v)
            .finish_non_exhaustive()
    }
}

/// The resolved supply rail, present while the preset has a `[bend.rail]` section.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Nominal rail voltage in volts, the camera's `[power] rail_v`, at the quiescent load.
    pub rail_v: f64,
    /// Source impedance of the rail in ohms: the bend's `source_ohm`, else the camera's.
    pub source_ohm: f64,
    /// Decoupling capacitance across the rail in microfarads.
    pub decoupling_uf: f64,
    /// Duration of one readout line, blanking included, in seconds.
    pub line_period_s: f64,
    /// Duration of one frame, vertical blanking included, in seconds.
    pub frame_period_s: f64,
    /// Seed of the load spikes.
    pub seed: u64,
    /// The preset's `[bend.rail]` section as written.
    pub bend: RailBend,
}

impl Params {
    /// Resolves the rail from the camera's `[power]` and the preset's `[bend.rail]`; `None` without the bend.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value of the section is out of range.
    pub fn resolve(
        camera: &Camera,
        preset: &Preset,
        timing: &Timing,
    ) -> Result<Option<Self>, Error> {
        let Some(bend) = preset.bend.rail else {
            return Ok(None);
        };
        bend.validate()?;
        Ok(Some(Self {
            rail_v: camera.power.rail_v,
            source_ohm: bend.source_ohm.unwrap_or(camera.power.source_ohm),
            decoupling_uf: camera.power.decoupling_uf,
            line_period_s: timing.line_period_s,
            frame_period_s: timing.frame_period_s,
            seed: noise::stage_seed(preset.seed, SALT),
            bend,
        }))
    }

    /// Returns the rail's time constant `R C` in seconds.
    #[must_use]
    pub fn tau_s(&self) -> f64 {
        self.source_ohm * self.decoupling_uf * 1e-6
    }
}

/// The rail's memory from frame to frame; the default is the power-on state.
///
/// Holds the decoupling capacitor's voltage, the ripple's phase and the rest of a
/// load spike in progress, each as at the start of the next frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// Capacitor voltage minus the nominal rail, in volts.
    offset_v: f64,
    /// Ripple phase in cycles, in `[0, 1)`.
    ripple_phase: f64,
    /// Rest of the load spike in progress, in seconds; 0 when none runs.
    spike_left_s: f64,
}

impl State {
    /// Returns the decoupling capacitor's voltage minus the nominal rail, in volts.
    ///
    /// As at the start of the next frame: negative while the rail sags.
    #[must_use]
    pub fn offset_v(&self) -> f64 {
        self.offset_v
    }

    /// Returns the ripple's phase at the start of the next frame, in cycles in `[0, 1)`.
    #[must_use]
    pub fn ripple_phase(&self) -> f64 {
        self.ripple_phase
    }

    /// Returns how long the load spike in progress at the start of the next frame still runs, in seconds.
    ///
    /// 0 when no spike runs across the frame boundary.
    #[must_use]
    pub fn spike_left_s(&self) -> f64 {
        self.spike_left_s
    }
}

/// Fills `rail` with the supply voltage of every readout line of frame `index`.
///
/// Without a bend (`params` is `None`) the rail already holds the camera's nominal
/// `rail_v`, at which the simulator builds it and to which it returns it when the
/// bend is removed; `state` returns to power-on, the capacitor settled at the nominal
/// voltage. With one, the rail follows the model of the [module documentation](self)
/// through every line and the vertical blanking from `state`, which is left as at the
/// start of the next frame. `bus` is the bus bend's state after the previous frame,
/// whose per-line current loads the rail; the spikes of frame `index` come from a
/// stream seeded with the stage seed and `index`.
///
/// # Examples
///
/// ```
/// use vcb_core::power::{self, Params, Rail, State};
/// use vcb_core::preset::RailBend;
/// use vcb_core::readout::{Direction, Layout};
/// use vcb_core::{Camera, Preset, Timing, bus};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
/// let camera: Camera = toml::from_str(&std::fs::read_to_string(path)?)?;
/// let mut preset = Preset::new("a520");
/// preset.bend.rail = Some(RailBend { load_ma: 200.0, ..RailBend::default() });
/// let timing = Timing::derive(&camera, &preset, 2272, 1704)?;
/// let params = Params::resolve(&camera, &preset, &timing)?.expect("the preset bends the rail");
/// let mut rail = Rail::new(&Layout::from_timing(&timing, Direction::TopDown), 3.3);
/// power::track(Some(&params), &mut State::default(), &bus::State::default(), 0, &mut rail);
/// // 200 mA more through the source impedance: the rail settles R * 0.2 A low.
/// let settled_v = camera.power.rail_v - params.source_ohm * 0.2;
/// assert!((rail.end_v(rail.lines() - 1) - settled_v).abs() < 1e-9);
/// # Ok(())
/// # }
/// ```
pub fn track(
    params: Option<&Params>,
    state: &mut State,
    bus: &bus::State,
    index: u64,
    rail: &mut Rail,
) {
    let Some(params) = params else {
        *state = State::default();
        return;
    };
    let bend = &params.bend;
    let mut supply = Supply::new(params, state, index);
    let line_s = params.line_period_s;
    for line in 0..rail.lines() {
        let start_s = f64::from(line) * line_s;
        let bus_ma = bus
            .line_current_ma
            .get(line as usize)
            .map_or(0.0, |&ma| f64::from(ma));
        let start_v = supply.offset_v;
        supply.run(start_s, start_s + line_s, bend.load_ma + bus_ma);
        rail.set(
            line,
            params.rail_v + start_v,
            params.rail_v + supply.offset_v,
        );
    }
    let read_s = f64::from(rail.lines()) * line_s;
    let frame_s = params.frame_period_s.max(read_s);
    supply.run(read_s, frame_s, bend.load_ma);
    *state = State {
        offset_v: supply.offset_v,
        ripple_phase: (state.ripple_phase + bend.ripple_hz * frame_s).rem_euclid(1.0),
        spike_left_s: (supply.spikes.end_s - frame_s).max(0.0),
    };
}

/// The source, the decoupling and the loads over one frame, in frame time.
struct Supply {
    /// Source impedance in ohms.
    source_ohm: f64,
    /// Nominal rail voltage in volts.
    rail_v: f64,
    /// Time constant of the rail in seconds.
    tau_s: f64,
    /// The ripple through the RC filter; `None` without ripple.
    ripple: Option<Ripple>,
    /// The load spikes.
    spikes: Spikes,
    /// The rail minus its nominal voltage, in volts, at the time reached.
    offset_v: f64,
}

impl Supply {
    fn new(params: &Params, state: &State, index: u64) -> Self {
        let bend = &params.bend;
        let tau_s = params.tau_s();
        let ripple = (bend.ripple_mv > 0.0).then(|| {
            let omega_tau = TAU * bend.ripple_hz * tau_s;
            Ripple {
                amplitude_v: bend.ripple_mv * 1e-3 / omega_tau.mul_add(omega_tau, 1.0).sqrt(),
                freq_hz: bend.ripple_hz,
                start_phase: state.ripple_phase,
                lag_rad: omega_tau.atan(),
            }
        });
        Self {
            source_ohm: params.source_ohm,
            rail_v: params.rail_v,
            tau_s,
            ripple,
            spikes: Spikes::new(bend, params.seed, index, state.spike_left_s),
            offset_v: state.offset_v,
        }
    }

    /// Advances the rail from `from_s` to `to_s` under a steady added load of `load_ma`.
    ///
    /// The interval is split where a spike starts or ends.
    fn run(&mut self, from_s: f64, to_s: f64, load_ma: f64) {
        let mut t_s = from_s;
        while t_s < to_s {
            let (spike_ma, change_s) = self.spikes.at(t_s);
            let until_s = change_s.min(to_s);
            self.relax(t_s, until_s, load_ma + spike_ma);
            t_s = until_s;
        }
    }

    /// Advances the rail exactly from `from_s` to `to_s` under a constant added load.
    fn relax(&mut self, from_s: f64, to_s: f64, load_ma: f64) {
        let settled_v = -self.source_ohm * load_ma * 1e-3;
        let decay = (-(to_s - from_s) / self.tau_s).exp();
        let (from_v, to_v) = self
            .ripple
            .as_ref()
            .map_or((0.0, 0.0), |ripple| (ripple.at(from_s), ripple.at(to_s)));
        let offset_v = settled_v + to_v + (self.offset_v - settled_v - from_v) * decay;
        // The loads starve at 0 V: the rail does not reverse.
        self.offset_v = offset_v.max(-self.rail_v);
    }
}

/// The source's ripple as it appears on the rail once the RC filter has settled.
struct Ripple {
    /// Amplitude on the rail in volts.
    amplitude_v: f64,
    /// Frequency in hertz.
    freq_hz: f64,
    /// Phase of the source's ripple at the start of the frame, in cycles.
    start_phase: f64,
    /// Lag of the rail behind the source, in radians.
    lag_rad: f64,
}

impl Ripple {
    /// Returns the ripple on the rail at `t_s` into the frame, in volts.
    fn at(&self, t_s: f64) -> f64 {
        let cycles = self.freq_hz.mul_add(t_s, self.start_phase).fract();
        self.amplitude_v * TAU.mul_add(cycles, -self.lag_rad).sin()
    }
}

/// The load spikes: a Poisson process of arrivals, each holding the load on for a while.
struct Spikes {
    /// Arrival stream of the frame.
    rng: Pcg32,
    /// Mean arrivals per second; 0 for none.
    rate_hz: f64,
    /// How long one spike lasts, in seconds.
    duration_s: f64,
    /// Current of a spike in milliamperes.
    current_ma: f64,
    /// When the spike in progress ends, in frame time; at or before now when none runs.
    end_s: f64,
    /// Time of the next arrival, in frame time.
    next_s: f64,
}

impl Spikes {
    /// Starts the frame's spikes with `left_s` of a spike carried over from the last frame.
    fn new(bend: &RailBend, seed: u64, index: u64, left_s: f64) -> Self {
        let mut spikes = Self {
            rng: Pcg32::new(seed, index),
            rate_hz: if bend.spike_ma > 0.0 {
                bend.spike_hz
            } else {
                0.0
            },
            duration_s: bend.spike_ms * 1e-3,
            current_ma: bend.spike_ma,
            end_s: left_s,
            next_s: 0.0,
        };
        spikes.next_s = spikes.interval();
        spikes
    }

    /// Returns the time to the next arrival, exponentially distributed; infinite without spikes.
    fn interval(&mut self) -> f64 {
        if self.rate_hz > 0.0 {
            let u = (f64::from(self.rng.next_u32()) + 0.5) * ONE_OVER_2_32;
            -u.ln() / self.rate_hz
        } else {
            f64::INFINITY
        }
    }

    /// Takes the arrivals up to `t_s` and returns the spike current then and when it next changes.
    fn at(&mut self, t_s: f64) -> (f64, f64) {
        while self.next_s <= t_s {
            self.end_s = self.end_s.max(self.next_s + self.duration_s);
            self.next_s += self.interval();
        }
        if self.end_s > t_s {
            (self.current_ma, self.end_s.min(self.next_s))
        } else {
            (0.0, self.next_s)
        }
    }
}
