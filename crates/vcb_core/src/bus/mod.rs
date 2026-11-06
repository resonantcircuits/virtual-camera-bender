//! The bend circuit on the ADC's parallel data bus.
//!
//! The documented A520 modification taps the data lines between the ADC and the image
//! processor (schematic in `diagrams/`; component data in `docs/research/a520-technical.md`
//! sections 5, 6 and 10; bend behaviour in `docs/research/circuit-bend-reference.md`).
//! [`bend`] simulates it electrically on the stream of ADC codes:
//!
//! - **Drivers.** Every bus line `Dk` is driven by the AFE's CMOS output stage: an ideal
//!   source at 0 V or `v_oh` behind `driver_r_ohm` plus the bus series resistor
//!   `bus_series_ohm`. Each line's driver resistance carries a log-normal spread of
//!   `driver_r_spread`, hashed from the camera's `unit_seed` and so fixed for the unit
//!   whatever the preset's seed, so a contest between two equal drivers resolves the
//!   same way every time and differently for each pair of lines. A driver holds the
//!   bit of the code being converted for the whole pixel period; during blanking the
//!   ADC converts the pedestal and the drivers hold the black code. At the start of a
//!   pixel period a driver that changes its bit slews at `driver_slew_v_per_us`, so a
//!   full swing takes `v_oh / driver_slew_v_per_us`, which must fit in a pixel period.
//! - **Line capacitance.** Every tapped line carries `line_pf` to ground (pin, trace and
//!   the processor's input), and so do the bend's wires S and T. A line therefore
//!   charges through its driver's resistance and is still moving for part of each
//!   pixel period after a change; a direct wire adds the capacitances it joins. Lines
//!   the bend does not touch drive only their own capacitance and are taken to have
//!   settled at the latch, so they keep the ADC's bit.
//! - **Source bank.** A DIP switch joins the pins in `source_pins` (`D2` to `D11`) into
//!   node S, each through a patch resistor of `series_ohm`. A patch resistor of 0 is a
//!   direct wire, which makes the pin and S one node.
//! - **Coupling.** A capacitor of `cap_uf` joins S to node F, and a potentiometer of
//!   `pot_kohm` loads one of them to ground ([`PotNode`]). On F the pair is a
//!   high-pass filter: edges of the source mix pass to F, its mean decays away with the
//!   time constant of the capacitor and the resistance around it. On S, as the
//!   schematic draws it, F keeps no resistive path but the gate inputs, and the
//!   capacitor shifts the source mix's level instead of filtering it.
//! - **Gate inputs.** The inverter's input and the flip-flop's clock sit on F. Each
//!   leaks `input_leak_mohm` to half the supply and clamps F through protection diodes:
//!   below ground or above the supply by more than `clamp_diode_v`, a diode conducts
//!   through `clamp_r_ohm`. The diodes charge the capacitor when F passes a rail, which
//!   restores F's level when the potentiometer loads S (research section 6).
//! - **Function.** [`Function::Bypass`] wires F to node T. [`Function::Invert`] drives T
//!   from a 74HC00 NAND gate with tied inputs, and [`Function::Divide`] from the `Q`
//!   output of a 74HC74 wired as a toggle (`D` from `/Q`, preset and clear tied high)
//!   and clocked by F. Both chips read F through a threshold with hysteresis and drive T
//!   through the 74HC output resistance, which beats the AFE's weaker drivers in a
//!   contest on a shared line (research section 6).
//! - **Target.** A selector joins T to one pin (`D4` to `D11`) through `series_ohm`, or
//!   to ground through a closed switch. `common_bus`, or a pin in both banks, makes S
//!   and T one node; with the divider this is a flip-flop that clocks itself.
//! - **Image processor latch.** At `latch_phase` of each pixel period the processor
//!   compares the pin voltage of every line it reads with `dsp_threshold_v` plus
//!   Gaussian thermal noise of `dsp_noise_mv`. A line behind a patch resistor is read at
//!   its pin, a direct short at its bus node. The voltage is the line's actual one at
//!   that instant, so a line still charging can read either way. The lines the bend
//!   touches return the latched bits; every other line keeps the ADC's bit. No bit is
//!   chosen at random: the same codes and state give the same bits, except where a pin
//!   voltage lies within the thermal noise of the threshold.
//!
//! # Gates
//!
//! The inverter's output follows the complement of its input after `hc_prop_delay_ns`;
//! an input pulse shorter than the delay is swallowed. The flip-flop toggles `Q`
//! `ff_prop_delay_ns` after a rising clock edge. It does not see an edge that comes less
//! than `ff_min_pulse_ns` after the clock went low, or less than `1 / ff_fmax_hz` after
//! the last edge it saw, and it keeps `Q` when `/Q` changed less than `ff_setup_ns`
//! before the edge. At about 3 V a 74HC74 is guaranteed only to 15 MHz (research
//! section 6), so it cannot follow a source mix that toggles at the 27 MHz pixel rate:
//! it skips edges and holds its state across them. These skipped edges, not a random
//! choice, make the divider's runs irregular.
//!
//! The gate delays, the minimum pulse, the setup time, `1 / ff_fmax_hz` and the
//! drivers' slew time are multiplied by `pixel_period_s / native_pixel_period_s`, and
//! the line capacitance with them, so at match-input resolution they and the lines'
//! time constants span the same number of pixel periods as on the camera's own clock.
//!
//! # Numerical method
//!
//! Time advances in `oversample` sub-steps per pixel period. Every capacitance (the
//! coupling capacitor and the line capacitances) is replaced by its backward-Euler
//! companion, a conductance `C / dt` in parallel with a current source `C / dt` times
//! its voltage at the previous sub-step, and each protection diode by a switch in
//! series with its forward voltage and resistance, which leaves a piecewise-linear
//! resistive network with three states: no diode, the diodes to ground, or the diodes
//! to the supply conducting. Ideal wires merge nodes, so a circuit of direct shorts
//! has two or three unknown node voltages, and one with patch resistors at most
//! `bits + 3`. [`Params::resolve`] stamps and inverts the conductance matrix of each
//! state once, and tabulates every needed voltage (node F, the capacitor voltages, each
//! latched pin) as a linear function of the driver levels, the gate output and the
//! previous capacitor voltages. A sub-step solves F in the previous sub-step's diode
//! state, switches state if F contradicts it, and is then a table lookup and one
//! multiply-add per capacitance for each voltage. A slewing driver enters each sub-step
//! with its mean level over that sub-step.
//!
//! The line time constants (a few nanoseconds) are comparable to a sub-step at the
//! native clock, where a backward-Euler step would let a line keep several percent of
//! each step at the latch. The resolved network therefore takes the backward-Euler
//! step only as a description of the circuit: on its eigenvectors each decay rate is
//! replaced by its exact exponential, so a sub-step is the network's exact response to
//! sources held over it (the circuit module's docs give the derivation). The line
//! capacitance draws its charge partly through the coupling capacitor, so the
//! capacitor voltage shifts by about `line_pf / cap_uf` of each step on S; at
//! match-input resolution, where the line capacitance is scaled up with the pixel
//! period, that share grows with it.
//!
//! Within a pixel period the drivers change only at its start, so while the gate's
//! output and the diode state hold, node F at every sub-step, the latched pins and the
//! state at the end are each a fixed linear function of the state at the start, which
//! `Params::resolve` tabulates per driver pattern. Such a pixel costs one product with
//! the start state per voltage and no voltage waits on another; a pixel in which the
//! gate switches or a diode changes runs sub-step by sub-step instead.
//!
//! A gate output changes at a sub-step boundary: from the first sub-step that ends at
//! or after its due time, and at least one sub-step after the input crossing that
//! causes it. The latch reads the voltages of the first sub-step that ends at or
//! after `latch_phase`.
//!
//! During horizontal and vertical blanking no samples are stored but the circuit keeps
//! running with the black code on the drivers. The drivers do not switch then, so once
//! the lines have settled the capacitor voltages are advanced over the interval in
//! closed form (the recursion of the slowest mode summed as a geometric series), split
//! where a pending gate transition completes and where a conducting
//! diode turns off, both exactly. After a change of the drivers, the gate or the diodes
//! the circuit steps explicitly until the line capacitances have settled to within
//! picovolts. The approximation is that the gates see no new input crossings during
//! blanking: a crossing of F caused by the black level is registered at the first
//! sub-step of the next line.
//!
//! # Supply
//!
//! One rail ([`power`](crate::power)) feeds the AFE's output drivers, the bend's
//! 74HC chips and the processor's inputs. Each pixel period runs at the rail of its
//! middle, read from the [`Rail`] (linear within a line), and each blanking interval at
//! the rail of its middle.
//!
//! - **Ratiometric circuit.** The drivers' and the gate's high level, the gate
//!   inputs' leakage bias and protection-diode thresholds, the 74HC switching
//!   thresholds and hysteresis and the processor's latch threshold all scale with the
//!   rail, `r = rail / nominal`. The network is linear in its sources, so with every
//!   source and threshold scaled together a voltage is `r` times the nominal network's
//!   for the state `z / r`. The run therefore keeps the state over `r` and evaluates
//!   the nominal tables unchanged; a new rail re-expresses the state, `z / r` becoming
//!   `z / r'`, so the capacitor voltages themselves never jump. Only the processor's
//!   thermal noise does not scale: it is referred to `r` as well. Scaling the diodes'
//!   forward voltage with the rail is the approximation (0.5 V at the nominal rail:
//!   a 10 % sag moves it 50 mV); slower 74HC timing and weaker drivers at a low rail
//!   are not modelled. Below a tenth of the nominal the circuit is held at a tenth.
//! - **Logic brownout.** Below `[power] logic_min_v` the 74HC chips lose their
//!   supply: the gate's output floats, so node T keeps only its other connections and
//!   the target line returns to its own driver, and the chips forget their logic
//!   levels and timers, starting from power-on when the rail returns. The run then
//!   uses a second network, solved once with node T undriven; the state's layout is
//!   the same, so the circuit moves between the two without a jump. Bypass has no
//!   chip in the path and ignores it.
//! - **Load.** At each latch instant the bend draws from the rail the current of its
//!   high sources: every tapped driver that is high delivers `(v_oh r - v_pin) /
//!   r_driver`, the gate's output, when high, `(v_oh r - v_T) / hc_r_out_ohm`, and the
//!   diodes to the supply on node F return what they conduct. The line's mean over
//!   its stored samples is left in [`State::line_current_ma`], from which the rail
//!   draws it during the whole line of the next frame. The drivers are taken at their
//!   new levels and the gate inputs' microampere leakage is left out.
//!
//! # State
//!
//! The capacitor voltages, the diodes' state, the gates' logic levels, pending
//! transitions and edge timers persist in [`State`] from frame to frame, as in the
//! camera, and so does the current the bend drew on each line, for the rail.

mod circuit;
mod gates;

use std::fmt::{self, Display, Formatter};
use std::ops::RangeInclusive;
use std::str::FromStr;

use crate::adc::Codes;
use crate::camera::Adc;
use crate::noise::{self, Pcg32};
use crate::power::Rail;
use crate::preset::BusBend;
use crate::validate::{non_negative, positive, within};
use crate::{Camera, Error, Preset, Timing};
use circuit::{Clamp, Drive, Kernel, Line, Network, Pixel, Sampling, Topology, Vector};
use gates::{Chips, FlipFlop, Gate, Inverter, Wire};

/// Salt of the bus circuit's noise sources.
const SALT: u32 = noise::salt("bus");

/// Salt of the per-line spread of the ADC drivers' output resistance.
const DRIVER_SALT: u32 = noise::salt("bus.driver");

/// Data lines the source DIP switch reaches, `D2` to `D11` (the bend's schematic).
const SOURCE_BANK: RangeInclusive<u8> = 2..=11;

/// Data lines the target selector reaches, `D4` to `D11` (the bend's schematic).
const TARGET_BANK: RangeInclusive<u8> = 4..=11;

/// Largest deviation of a [`Pcg32::gauss`] draw, in standard deviations.
///
/// Box-Muller on a 24-bit uniform reaches at most `sqrt(-2 ln 2^-24)`, about 5.77. A
/// pin voltage farther than this from the latch threshold reads the same for every
/// draw, so the latch skips the draw and stays exact.
const NOISE_REACH: f64 = 5.8;

/// Tolerance when locating the latch instant on the sub-step grid, in sub-steps.
const LATCH_TOLERANCE: f64 = 1e-6;

/// Tolerance when counting the sub-steps a driver's slew spans, in sub-steps.
///
/// A slew of exactly two sub-steps stays two instead of rounding up to three.
const RAMP_TOLERANCE: f64 = 1e-9;

/// Smallest ratio of the rail to its nominal voltage the circuit follows.
///
/// The run keeps its state over that ratio, which must not reach zero. A tenth of a
/// 3.3 V rail is a third of a volt, where every chip on it has long stopped working.
const MIN_RAIL_RATIO: f64 = 0.1;

/// Where the target selector connects the function output.
///
/// In a file this is written `"pin:N"` for data line `DN`, or `"gnd"`.
///
/// # Examples
///
/// ```
/// use vcb_core::bus::Target;
///
/// let target: Target = "pin:10".parse()?;
/// assert_eq!(target, Target::Pin(10));
/// assert_eq!(target.to_string(), "pin:10");
/// # Ok::<(), vcb_core::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "String", into = "String"))]
pub enum Target {
    /// Data line `Dk` of the bus.
    Pin(u8),
    /// Circuit ground.
    Gnd,
}

impl Display for Target {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pin(pin) => write!(f, "pin:{pin}"),
            Self::Gnd => f.write_str("gnd"),
        }
    }
}

impl FromStr for Target {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "gnd" {
            return Ok(Self::Gnd);
        }
        s.strip_prefix("pin:")
            .and_then(|pin| pin.parse().ok())
            .map(Self::Pin)
            .ok_or_else(|| {
                Error::config(format!(
                    "bus target must be \"pin:N\" or \"gnd\", got \"{s}\""
                ))
            })
    }
}

impl TryFrom<String> for Target {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Target> for String {
    fn from(value: Target) -> Self {
        value.to_string()
    }
}

/// The gate between node F and the target line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Function {
    /// A wire: F drives the target directly.
    #[default]
    Bypass,
    /// A 74HC00 NAND gate with tied inputs.
    Invert,
    /// A 74HC74 D flip-flop clocked by F with `D` tied to `/Q`.
    Divide,
}

/// The node the potentiometer loads to ground.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum PotNode {
    /// Node F, after the capacitor: F is the high-pass of the source mix around 0 V.
    #[default]
    F,
    /// Node S, before the capacitor, as the schematic draws it.
    ///
    /// F then has no resistive path but the gate inputs' leakage, so the capacitor
    /// shifts the source mix's level and the input clamp diodes restore it.
    S,
}

/// The resolved bend circuit, ready to run on the ADC's code stream.
///
/// Built by [`Params::resolve`] from the preset's `[bend.bus]` section, the camera's
/// `[adc]` and `[logic]` sections and the derived [`Timing`]; the circuit's network is
/// solved once there, so [`bend`] only evaluates it.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Data lines in the source bank, as configured.
    source_pins: Vec<u8>,
    /// Where node T is connected.
    target: Target,
    /// The gate between node F and node T.
    function: Function,
    /// Duration of one pixel period in seconds.
    pixel_period_s: f64,
    /// Circuit sub-steps per pixel period.
    oversample: u32,
    /// Code the ADC drives during blanking.
    dark_code: u16,
    /// Seed of the image processor's input noise.
    seed: u64,
    /// The solved network.
    net: Network,
    /// The network with the logic unpowered, the gate's output floating; `None` in bypass.
    dead: Option<Network>,
    /// Supply voltage below which the 74HC chips stop working, in volts.
    logic_min_v: f64,
    /// Thresholds and delays of the logic chips.
    chips: Chips,
    /// The image processor's input latch.
    latch: Latch,
    /// The slew of the line drivers.
    ramp: Ramp,
}

/// The image processor's input latch on the bus lines.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Latch {
    /// Logic threshold of the inputs in volts.
    threshold_v: f64,
    /// Thermal noise at the inputs in volts, one standard deviation.
    noise_v: f64,
    /// Distance from the threshold beyond which noise cannot change a reading, in volts.
    reach_v: f64,
    /// Sub-step of the pixel period whose voltages are latched.
    step: u32,
    /// Lines the bend touches and the processor reads; only these take latched bits.
    mask: u16,
}

impl Params {
    /// Resolves the bend circuit from the preset's `[bend.bus]` section; `None` without one.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a source pin lies outside `D2` to `D11`, the
    /// target pin outside `D4` to `D11`, a pin does not exist on the camera's ADC, a pin
    /// is selected twice or none is, or a component value is out of range.
    pub fn resolve(
        camera: &Camera,
        preset: &Preset,
        timing: &Timing,
    ) -> Result<Option<Self>, Error> {
        let Some(bus) = &preset.bend.bus else {
            return Ok(None);
        };
        let adc = &camera.adc;
        within("adc.bits", f64::from(adc.bits), 1.0, f64::from(u16::BITS))?;
        let sources = source_mask(&bus.source_pins, adc.bits)?;
        let target = target_mask(bus.target, adc.bits)?;
        positive("bend.bus.cap_uf", bus.cap_uf)?;
        non_negative("bend.bus.pot_kohm", bus.pot_kohm)?;
        non_negative("bend.bus.series_ohm", bus.series_ohm)?;
        non_negative("adc.line_pf", adc.line_pf)?;
        positive("adc.driver_slew_v_per_us", adc.driver_slew_v_per_us)?;

        let seed = noise::stage_seed(preset.seed, SALT);
        let unit = noise::stage_seed(camera.sensor.unit_seed, DRIVER_SALT);
        let oversample = timing.oversample.max(1);
        let dt_s = timing.pixel_period_s / f64::from(oversample);
        let tapped = sources | target;
        let lines: Vec<Line> = (0..adc.bits)
            .filter(|bit| tapped & (1 << bit) != 0)
            .map(|bit| Line {
                bit,
                driver_ohm: driver_ohm(adc, bit, unit),
            })
            .collect();
        let read = ((1u64 << adc.dsp_bits.min(16)) - 1) << adc.dsp_lsb.min(16);
        let latched = tapped & read as u16;
        // Gate timing, the drivers' slew and the line time constants are sub-line
        // constants: scaled so they keep their native length in pixel periods at
        // match-input resolution. The driver resistance is physical, so the line
        // capacitance carries the scale.
        let scale = timing.pixel_period_s / timing.native_pixel_period_s;
        let ramp_s = adc.v_oh / (adc.driver_slew_v_per_us * 1e6) * scale;
        if ramp_s > timing.pixel_period_s * (1.0 + RAMP_TOLERANCE) {
            return Err(Error::config(format!(
                "adc.driver_slew_v_per_us {}: a full swing takes {:.1} ns, longer than the \
                 {:.1} ns native pixel period",
                adc.driver_slew_v_per_us,
                ramp_s / scale * 1e9,
                timing.native_pixel_period_s * 1e9
            )));
        }
        let ramp = Ramp::new(ramp_s, dt_s);
        let latch = latch_step(adc.latch_phase, oversample);
        let topology = Topology {
            lines: &lines,
            sources,
            target: bus.target,
            function: bus.function,
            common_bus: bus.common_bus,
            series_ohm: bus.series_ohm,
            cap_f: bus.cap_uf * 1e-6,
            line_f: adc.line_pf * 1e-12 * scale,
            pot_ohm: bus.pot_kohm * 1e3,
            pot_node: bus.pot_node,
            gate_ohm: camera.logic.hc_r_out_ohm,
            gate_driven: true,
            v_oh: adc.v_oh,
            clamp_v: camera.logic.clamp_diode_v,
            clamp_ohm: camera.logic.clamp_r_ohm,
            leak_ohm: camera.logic.input_leak_mohm * 1e6,
            dt_s,
            bits: adc.bits,
            sampling: Sampling {
                oversample: oversample as usize,
                ramp: &ramp.weights,
                latch: latch as usize,
            },
        };
        let net = Network::build(&topology)?;
        let dead = match bus.function {
            Function::Bypass => None,
            Function::Invert | Function::Divide => Some(Network::build(&Topology {
                gate_driven: false,
                ..topology
            })?),
        };
        let noise_v = adc.dsp_noise_mv * 1e-3;
        Ok(Some(Self {
            source_pins: bus.source_pins.clone(),
            target: bus.target,
            function: bus.function,
            pixel_period_s: timing.pixel_period_s,
            oversample,
            dark_code: adc.black_level,
            seed,
            net,
            dead,
            logic_min_v: camera.power.logic_min_v,
            chips: Chips::new(&camera.logic, scale, dt_s),
            latch: Latch {
                threshold_v: adc.dsp_threshold_v,
                noise_v,
                reach_v: NOISE_REACH * noise_v,
                step: latch,
                mask: latched,
            },
            ramp,
        }))
    }

    /// Returns the data lines in the source bank.
    #[must_use]
    pub fn source_pins(&self) -> &[u8] {
        &self.source_pins
    }

    /// Returns where the target selector connects node T.
    #[must_use]
    pub fn target(&self) -> Target {
        self.target
    }

    /// Returns the gate between node F and node T.
    #[must_use]
    pub fn function(&self) -> Function {
        self.function
    }
}

/// Returns whether two bends are wired alike: the same circuit with other component values.
///
/// The wiring is the set of source pins, the target, the function, the potentiometer's
/// node and whether it is set to zero (a short to ground), the common bus and whether
/// patch resistors are fitted. Bends that differ only in the capacitor's, the pot's or
/// the patch resistors' values are the same circuit, whose state can carry over.
#[must_use]
pub fn same_wiring(a: Option<&BusBend>, b: Option<&BusBend>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let pins = |bend: &BusBend| {
                bend.source_pins.iter().fold(0u64, |mask, &pin| {
                    mask | 1u64.checked_shl(u32::from(pin)).unwrap_or(0)
                })
            };
            pins(a) == pins(b)
                && a.target == b.target
                && a.function == b.function
                && a.pot_node == b.pot_node
                && (a.pot_kohm > 0.0) == (b.pot_kohm > 0.0)
                && a.common_bus == b.common_bus
                && (a.series_ohm > 0.0) == (b.series_ohm > 0.0)
        }
        _ => false,
    }
}

/// The slew of the line drivers on the sub-step grid.
#[derive(Debug, Clone, PartialEq)]
struct Ramp {
    /// Mean fraction of a transition completed during each sub-step it spans.
    weights: Box<[f64]>,
}

impl Ramp {
    /// Resolves a full swing of `ramp_s` seconds onto sub-steps of `dt_s` seconds.
    ///
    /// The driver's source moves linearly from one level to the other over `ramp_s`; a
    /// sub-step sees its mean level, the integral of `min(1, t / ramp_s)` over the
    /// sub-step divided by its length.
    fn new(ramp_s: f64, dt_s: f64) -> Self {
        let steps = (ramp_s / dt_s - RAMP_TOLERANCE).ceil().max(0.0) as usize;
        let weights = (0..steps)
            .map(|j| {
                let (start, end) = (j as f64 * dt_s, (j + 1) as f64 * dt_s);
                if end <= ramp_s {
                    (start + end) / (2.0 * ramp_s)
                } else {
                    ((ramp_s * ramp_s - start * start) / (2.0 * ramp_s) + end - ramp_s) / dt_s
                }
            })
            .collect();
        Self { weights }
    }

    /// Returns the driver levels of sub-step `step` of a transition from `from` to `to`.
    #[inline]
    fn drive(&self, from: usize, to: usize, step: u64) -> Drive {
        match usize::try_from(step).ok().and_then(|j| self.weights.get(j)) {
            Some(&weight) if from != to => Drive::Ramp { from, to, weight },
            _ => Drive::Steady(to),
        }
    }

    /// Returns the sub-steps a transition spans.
    fn steps(&self) -> u64 {
        self.weights.len() as u64
    }
}

/// Returns the bit mask of the source pins after checking them against the DIP bank.
fn source_mask(pins: &[u8], bits: u8) -> Result<u16, Error> {
    let mut mask = 0u16;
    for &pin in pins {
        if !SOURCE_BANK.contains(&pin) || pin >= bits {
            return Err(Error::config(format!(
                "bend.bus.source_pins {pins:?}: the source bank reaches D{} to D{} of a \
                 {bits}-bit ADC, not D{pin}",
                SOURCE_BANK.start(),
                SOURCE_BANK.end().min(&bits.saturating_sub(1))
            )));
        }
        if mask & (1 << pin) != 0 {
            return Err(Error::config(format!(
                "bend.bus.source_pins {pins:?} selects D{pin} twice"
            )));
        }
        mask |= 1 << pin;
    }
    if mask == 0 {
        return Err(Error::config(
            "bend.bus.source_pins must select at least one pin",
        ));
    }
    Ok(mask)
}

/// Returns the bit mask of the target pin, 0 for ground, after checking the selector.
fn target_mask(target: Target, bits: u8) -> Result<u16, Error> {
    match target {
        Target::Gnd => Ok(0),
        Target::Pin(pin) if TARGET_BANK.contains(&pin) && pin < bits => Ok(1 << pin),
        Target::Pin(pin) => Err(Error::config(format!(
            "bend.bus.target: the target selector reaches gnd and D{} to D{} of a {bits}-bit \
             ADC, not D{pin}",
            TARGET_BANK.start(),
            TARGET_BANK.end().min(&bits.saturating_sub(1))
        ))),
    }
}

/// Returns the resistance between line `bit`'s ideal driver and its pin in ohms.
///
/// The AFE driver's resistance is log-normal around `driver_r_ohm` with a relative
/// spread of `driver_r_spread`, a fixed property of the unit hashed from the line and
/// `unit`, the seed derived from the camera's `unit_seed`; the bus series resistor adds
/// to it.
fn driver_ohm(adc: &Adc, bit: u8, unit: u64) -> f64 {
    let deviation = f64::from(noise::gauss(u32::from(bit), 0, DRIVER_SALT, unit));
    adc.driver_r_ohm * (adc.driver_r_spread * deviation).exp() + adc.bus_series_ohm
}

/// Returns the sub-step whose end is the first at or after `phase` of the pixel period.
fn latch_step(phase: f64, oversample: u32) -> u32 {
    let end = (phase * f64::from(oversample) - LATCH_TOLERANCE)
        .ceil()
        .max(0.0) as u32;
    end.clamp(1, oversample) - 1
}

/// Persistent state of the bend circuit between frames.
///
/// Holds the voltages of the coupling capacitor and the line capacitances, which input
/// protection diodes conduct, the logic levels, pending transitions and edge timers of
/// the inverter and the flip-flop, and the image processor's input-noise stream, which
/// each frame reseeds from its index. The default is the power-on state: every
/// capacitance discharged, no diode conducting, the inverter's output high for its low
/// input, `Q` low.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    /// The capacitor and the clamp diodes on node F.
    coupling: Coupling,
    /// The 74HC00 inverter.
    inverter: Inverter,
    /// The 74HC74 flip-flop.
    flip_flop: FlipFlop,
    /// The image processor's input-noise stream.
    noise: Pcg32,
    /// Mean current the bend drew from the supply rail on each readout line of the last frame, in milliamperes.
    ///
    /// The current the high drivers and the gate's output push into the
    /// potentiometer, the contested lines, the gate inputs and the clamp diodes, less
    /// what the diodes to the supply return, taken at the latch instants of the line's
    /// stored samples: one entry per readout line in readout order, which the
    /// [`power`](crate::power) stage reads as a load on the rail one frame later.
    /// Empty until the bend has run.
    pub line_current_ma: Vec<f32>,
}

/// Analog state of the network: the capacitors' charge and the protection diodes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Coupling {
    /// Voltages of the capacitances: the coupling capacitor (S minus F), then the line nodes.
    z: Vector,
    /// Which protection diodes conducted in the last sub-step.
    clamp: Clamp,
    /// Sub-steps in which a protection diode conducted, since power-on.
    #[cfg(feature = "test-util")]
    clamped: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            coupling: Coupling::default(),
            inverter: Inverter::default(),
            flip_flop: FlipFlop::default(),
            noise: Pcg32::new(0, 0),
            line_current_ma: Vec::new(),
        }
    }
}

#[cfg(feature = "test-util")]
impl State {
    /// Returns the capacitor voltage, node S minus node F, in volts.
    #[must_use]
    pub fn cap_v(&self) -> f64 {
        self.coupling.z[0]
    }

    /// Returns the sub-steps since power-on in which an input protection diode conducted.
    #[must_use]
    pub fn clamped_steps(&self) -> u64 {
        self.coupling.clamped
    }
}

/// Rewrites `codes` with the words the image processor latches through the bend.
///
/// Without a bend (`params` is `None`) the words are unchanged. With one, the circuit
/// runs through the frame in readout order, `oversample` sub-steps per pixel period and
/// in closed form across blanking, starting from `state` and leaving its final state
/// there. Only the lines the bend touches can change. The processor's input noise for
/// frame `index` comes from a stream seeded by the stage seed and `index`.
///
/// `rail` is the supply rail of every readout line from
/// [`power::track`](crate::power::track), which powers the drivers, the 74HC logic
/// and the processor's inputs: every pixel period runs at the rail of its middle, and
/// each blanking interval at the rail of its middle (the vertical blanking where the
/// last line left it), as the module documentation's "Supply" describes. The mean
/// current the bend drew from the rail on each line is left in
/// [`State::line_current_ma`].
///
/// # Panics
///
/// Panics if the codes' pixel period is not the one `params` was resolved for, or
/// `rail` has another number of lines than the codes.
pub fn bend(
    params: Option<&Params>,
    state: &mut State,
    rail: &Rail,
    index: u64,
    codes: &mut Codes,
) {
    let Some(params) = params else {
        return;
    };
    let layout = codes.layout();
    assert!(
        (layout.pixel_period_s() / params.pixel_period_s - 1.0).abs() < 1e-9,
        "codes have a pixel period of {} s but the bend was resolved for {} s",
        layout.pixel_period_s(),
        params.pixel_period_s
    );
    assert_eq!(
        rail.lines(),
        layout.rows(),
        "the rail has {} lines but the codes {}",
        rail.lines(),
        layout.rows()
    );
    let oversample = u64::from(params.oversample);
    let line_len = layout.line_len() as usize;
    let hblank = u64::from(layout.hblank_px()) * oversample;
    let line_steps = u64::from(layout.line_len() + layout.hblank_px()) * oversample;
    let blank = Blanking {
        line: hblank,
        frame: hblank + u64::from(layout.vblank_lines()) * line_steps,
        line_px: f64::from(layout.line_len() + layout.hblank_px()),
        hblank_px: f64::from(layout.hblank_px()),
    };
    state.noise = Pcg32::new(params.seed, index);
    let mut currents = std::mem::take(&mut state.line_current_ma);
    currents.resize(layout.rows() as usize, 0.0);
    let words = codes.words_mut();
    dispatch(
        params,
        state,
        Work::Frame {
            words,
            line_len,
            blank,
            rail,
            currents: &mut currents,
        },
    );
    state.line_current_ma = currents;
}

/// Blanking after a line and after the last line of a frame, and the line it ends.
#[derive(Debug, Clone, Copy)]
struct Blanking {
    /// Sub-steps of horizontal blanking after a line.
    line: u64,
    /// Sub-steps of blanking after the last line of a frame, the vertical blanking included.
    frame: u64,
    /// Pixel periods of a whole line, blanking included: the span of one rail entry.
    line_px: f64,
    /// Pixel periods of horizontal blanking.
    hblank_px: f64,
}

/// What a run of the circuit does.
enum Work<'w> {
    /// Rewrites a frame of lines, each followed by blanking, on a supply rail.
    Frame {
        words: &'w mut [u16],
        line_len: usize,
        blank: Blanking,
        rail: &'w Rail,
        /// The mean current drawn from the rail on every line, in milliamperes.
        currents: &'w mut [f32],
    },
    /// Rewrites consecutive pixel periods without blanking.
    #[cfg(feature = "test-util")]
    Words(&'w mut [u16]),
    /// Probes consecutive pixel periods without blanking.
    #[cfg(feature = "test-util")]
    Trace {
        codes: &'w [u16],
        probes: &'w mut Vec<Probe>,
    },
}

/// Runs `work` with the gate of the function switch and the network's state count.
///
/// The state count becomes a constant, so every sub-step's arithmetic runs on
/// fixed-size arrays.
fn dispatch(params: &Params, state: &mut State, work: Work<'_>) {
    fn gated<const M: usize>(params: &Params, state: &mut State, work: Work<'_>) {
        let State {
            coupling,
            inverter,
            flip_flop,
            noise,
            ..
        } = state;
        match params.function {
            Function::Bypass => Run::<_, M>::new(params, &mut Wire, noise, coupling).work(work),
            Function::Invert => Run::<_, M>::new(params, inverter, noise, coupling).work(work),
            Function::Divide => Run::<_, M>::new(params, flip_flop, noise, coupling).work(work),
        }
    }
    macro_rules! states {
        ($($m:literal)*) => {
            match params.net.states() {
                $($m => gated::<$m>(params, state, work),)*
                states => unreachable!("the bend circuit has {states} state variables"),
            }
        };
    }
    states!(1 2 3 4 5 6 7 8 9 10 11 12 13);
}

/// The sub-step arithmetic of one network for `M` state variables.
struct Tables<'a, const M: usize> {
    /// The network.
    net: &'a Network,
    /// The sub-step arithmetic of every diode state.
    kernels: [Kernel<'a, M>; 3],
    /// Whole pixel periods by diode state and gate level.
    pixels: [[Pixel<'a, M>; 2]; 3],
}

impl<'a, const M: usize> Tables<'a, M> {
    fn new(net: &'a Network) -> Self {
        Self {
            net,
            kernels: net.kernels(),
            pixels: net.pixels(),
        }
    }
}

/// The circuit running with one gate on the function switch and `M` state variables.
struct Run<'a, G, const M: usize> {
    params: &'a Params,
    gate: &'a mut G,
    /// Where the coupling state is written back when the run ends.
    state: &'a mut Coupling,
    /// The circuit's evolving state.
    core: Core<'a, M>,
    /// The network with the bend's logic powered.
    live: Tables<'a, M>,
    /// The network with the bend's logic unpowered; `None` in bypass.
    dead: Option<Tables<'a, M>>,
    /// Whether the logic is unpowered: the rail is below `logic_min_v`.
    off: bool,
}

/// The evolving state of a run, apart from the gate.
struct Core<'a, const M: usize> {
    params: &'a Params,
    noise: &'a mut Pcg32,
    /// The diode state and the probes' count, as at the end of the last sub-step.
    coupling: Coupling,
    /// The capacitances' voltages at the end of the last sub-step over `ratio`.
    z: [f64; M],
    /// Driver pattern of the last pixel period; blanking, and so the black code, at the start.
    last: usize,
    /// The image processor's latch, its noise referred to the rail.
    latch: Latch,
    /// The rail in volts, `NaN` before it is first set.
    rail_v: f64,
    /// The rail over the nominal, in which every source, threshold and `z` is expressed.
    ratio: f64,
    /// Current drawn from the rail at the latch instants of the line so far, summed, in amperes.
    drawn_a: f64,
}

impl<'a, G: Gate + Clone + Default, const M: usize> Run<'a, G, M> {
    fn new(
        params: &'a Params,
        gate: &'a mut G,
        noise: &'a mut Pcg32,
        state: &'a mut Coupling,
    ) -> Self {
        Self {
            params,
            gate,
            core: Core {
                params,
                noise,
                coupling: *state,
                z: std::array::from_fn(|k| state.z[k]),
                last: params.net.pattern(params.dark_code),
                latch: params.latch,
                rail_v: f64::NAN,
                ratio: 1.0,
                drawn_a: 0.0,
            },
            state,
            live: Tables::new(&params.net),
            dead: params.dead.as_ref().map(Tables::new),
            off: false,
        }
    }

    /// Does `work` and writes the state back, in volts.
    fn work(mut self, work: Work<'_>) {
        match work {
            Work::Frame {
                words,
                line_len,
                blank,
                rail,
                currents,
            } => self.frame(words, line_len, blank, rail, currents),
            #[cfg(feature = "test-util")]
            Work::Words(codes) => {
                for code in codes {
                    *code = self.period(*code);
                }
            }
            #[cfg(feature = "test-util")]
            Work::Trace { codes, probes } => self.trace(codes, probes),
        }
        let core = &mut self.core;
        for (volts, scaled) in core.coupling.z.iter_mut().zip(core.z) {
            *volts = scaled * core.ratio;
        }
        *self.state = core.coupling;
    }

    /// Runs a frame of `line_len` samples per line, each line followed by blanking.
    ///
    /// Records the mean current drawn from the rail on every line in `currents`.
    fn frame(
        &mut self,
        words: &mut [u16],
        line_len: usize,
        blank: Blanking,
        rail: &Rail,
        currents: &mut [f32],
    ) {
        let nominal_v = rail.nominal_v();
        let lines = words.len() / line_len;
        let supply = rail.lines_v().iter().zip(currents);
        for (line, (samples, (&[start_v, end_v], current))) in
            words.chunks_exact_mut(line_len).zip(supply).enumerate()
        {
            let slope_v = (end_v - start_v) / blank.line_px;
            self.core.drawn_a = 0.0;
            for (sample, word) in samples.iter_mut().enumerate() {
                self.supply(slope_v.mul_add(sample as f64 + 0.5, start_v), nominal_v);
                *word = self.period(*word);
            }
            *current = (self.core.drawn_a / line_len as f64 * 1e3) as f32;
            if line + 1 == lines {
                self.supply(end_v, nominal_v);
                self.blank(blank.frame);
            } else {
                let middle_px = 0.5f64.mul_add(blank.hblank_px, line_len as f64);
                self.supply(slope_v.mul_add(middle_px, start_v), nominal_v);
                self.blank(blank.line);
            }
        }
    }

    /// Sets the rail at `rail_v` volts for what follows; `nominal_v` is the camera's.
    ///
    /// Below `logic_min_v` the 74HC chips lose their supply: their output floats, and on
    /// entering that state each forgets its logic levels and timers, as at power-on.
    fn supply(&mut self, rail_v: f64, nominal_v: f64) {
        self.core.supply(rail_v, nominal_v);
        let off = self.dead.is_some() && rail_v < self.params.logic_min_v;
        if off && !self.off {
            *self.gate = G::default();
        }
        self.off = off;
    }

    /// Runs one pixel period with `code` on the drivers and returns the latched word.
    #[inline]
    fn period(&mut self, code: u16) -> u16 {
        let Self {
            core,
            gate,
            live,
            dead,
            off,
            ..
        } = self;
        match dead {
            Some(dead) if *off => core.period(&mut Wire, dead, code),
            _ => core.period(&mut **gate, live, code),
        }
    }

    /// Advances the circuit through `steps` sub-steps of blanking.
    fn blank(&mut self, steps: u64) {
        let Self {
            core,
            gate,
            live,
            dead,
            off,
            ..
        } = self;
        match dead {
            Some(dead) if *off => core.blank(&mut Wire, dead, steps),
            _ => core.blank(&mut **gate, live, steps),
        }
    }
}

impl<const M: usize> Core<'_, M> {
    /// Sets the rail at `rail_v` volts; `nominal_v` is the camera's.
    ///
    /// Every source and threshold of the circuit scales with the rail, so the state is
    /// carried over the rail's ratio to nominal: a new ratio re-expresses it, and the
    /// latch's thermal noise, which does not scale, is referred to the new ratio.
    #[inline]
    fn supply(&mut self, rail_v: f64, nominal_v: f64) {
        if rail_v.to_bits() == self.rail_v.to_bits() {
            return;
        }
        self.rail_v = rail_v;
        let ratio = (rail_v / nominal_v).max(MIN_RAIL_RATIO);
        if ratio.to_bits() != self.ratio.to_bits() {
            let factor = self.ratio / ratio;
            for volts in &mut self.z {
                *volts *= factor;
            }
            self.ratio = ratio;
            let latch = &self.params.latch;
            self.latch.noise_v = latch.noise_v / ratio;
            self.latch.reach_v = latch.reach_v / ratio;
        }
    }

    /// Runs one pixel period with `code` on the drivers and returns the latched word.
    ///
    /// While the gate's output and the diode state hold through the pixel, which is
    /// the rule, the whole period is evaluated in closed form from its start state:
    /// node F at every sub-step for the diodes and gates, the latched pins, the end
    /// state. When the gate's output changes or a diode would switch inside the pixel,
    /// the gate is put back and the pixel runs sub-step by sub-step. Both give the same
    /// voltages to rounding.
    #[inline]
    fn period<H: Gate + Clone>(&mut self, gate: &mut H, tables: &Tables<'_, M>, code: u16) -> u16 {
        let params = self.params;
        let net = tables.net;
        let pattern = net.pattern(code);
        let from = std::mem::replace(&mut self.last, pattern);
        let ramp = (from != pattern).then_some(from);
        let clamp = self.coupling.clamp;
        let high = gate.high();
        let pixel = &tables.pixels[clamp.index()][usize::from(high)];
        let start = self.z;
        let saved = gate.clone();
        let latch_step = self.latch.step as usize;
        let mut latch_f_v = 0.0;
        let mut holds = true;
        for (step, f_v) in pixel.f(pattern, ramp, &start).enumerate() {
            gate.tick();
            if gate.high() != high || !net.consistent(clamp, f_v) {
                holds = false;
                break;
            }
            if step == latch_step {
                latch_f_v = f_v;
            }
            gate.sense(f_v, &params.chips);
        }
        if !holds {
            *gate = saved;
            self.last = from;
            return self.pixel(gate, tables, code, |_, _, _, _| {});
        }
        let supply = net.supply();
        let mut bits = 0;
        let mut drawn_a = 0.0;
        for (i, (bit, volts)) in pixel.pins(pattern, ramp, &start).enumerate() {
            if self.latch.mask & (1 << bit) != 0 {
                bits |= u16::from(read(&self.latch, self.noise, volts)) << bit;
            }
            if pattern & (1 << i) != 0 {
                drawn_a += (supply.v_oh - volts) * supply.line_s[i];
            }
        }
        if high {
            drawn_a += (supply.v_oh - pixel.t_v(pattern, ramp, &start)) * supply.gate_s;
        }
        if clamp == Clamp::Rail {
            drawn_a -= supply.clamp_s * (latch_f_v - supply.ceiling_v);
        }
        self.drawn_a += drawn_a * self.ratio;
        self.z = pixel.end(pattern, ramp, &start);
        count_clamped(&mut self.coupling, u64::from(params.oversample));
        (code & !self.latch.mask) | bits
    }

    /// Runs one pixel period sub-step by sub-step and returns the latched word.
    ///
    /// `probe` sees the diode state, driver levels, previous state and gate level of
    /// every sub-step, from which every node voltage of that sub-step follows.
    #[inline]
    fn pixel<H: Gate>(
        &mut self,
        gate: &mut H,
        tables: &Tables<'_, M>,
        code: u16,
        mut probe: impl FnMut(Clamp, Drive, &[f64], f64),
    ) -> u16 {
        let params = self.params;
        let net = tables.net;
        let pattern = net.pattern(code);
        let from = std::mem::replace(&mut self.last, pattern);
        let mut latched = 0;
        for step in 0..params.oversample {
            let drive = params.ramp.drive(from, pattern, u64::from(step));
            gate.tick();
            let high = usize::from(gate.high());
            let clamp = &mut self.coupling.clamp;
            let f_v = net.settle_in(&tables.kernels, clamp, drive, &self.z, high);
            let clamp = *clamp;
            probe(clamp, drive, &self.z, level(high == 1));
            if step == self.latch.step {
                latched = self.latch(tables, drive, high, pattern, f_v);
            }
            Network::kernel(&tables.kernels, clamp).advance(drive, &mut self.z, high);
            count_clamped(&mut self.coupling, 1);
            gate.sense(f_v, &params.chips);
        }
        (code & !self.latch.mask) | latched
    }

    /// Reads every latched pin against the threshold plus thermal noise.
    ///
    /// Adds the current the rail delivers at this sub-step to the line's sum: the
    /// drivers of `pattern` that are high, the gate if `high`, less what the diodes to
    /// the supply return from node F at `f_v`.
    fn latch(
        &mut self,
        tables: &Tables<'_, M>,
        drive: Drive,
        high: usize,
        pattern: usize,
        f_v: f64,
    ) -> u16 {
        let kernel = Network::kernel(&tables.kernels, self.coupling.clamp);
        let supply = tables.net.supply();
        let mut bits = 0;
        let mut drawn_a = 0.0;
        for (i, (bit, volts)) in kernel.pins(drive, &self.z, high).enumerate() {
            if self.latch.mask & (1 << bit) != 0 {
                bits |= u16::from(read(&self.latch, self.noise, volts)) << bit;
            }
            if pattern & (1 << i) != 0 {
                drawn_a += (supply.v_oh - volts) * supply.line_s[i];
            }
        }
        if high == 1 {
            drawn_a += (supply.v_oh - kernel.t_v(drive, &self.z, high)) * supply.gate_s;
        }
        if self.coupling.clamp == Clamp::Rail {
            drawn_a -= supply.clamp_s * (f_v - supply.ceiling_v);
        }
        self.drawn_a += drawn_a * self.ratio;
        bits
    }

    /// Advances the circuit through `steps` sub-steps of blanking.
    ///
    /// The drivers slew to the black code and the lines settle in explicit sub-steps;
    /// so do the sub-steps after the gate's output or the diodes change. Each round then
    /// runs one sub-step explicitly, which settles the diodes and completes a gate
    /// transition that falls due, and as many sub-steps in closed form as the gate's
    /// pending transition and the diodes allow.
    fn blank<H: Gate>(&mut self, gate: &mut H, tables: &Tables<'_, M>, mut steps: u64) {
        let params = self.params;
        let net = tables.net;
        let dark = net.pattern(params.dark_code);
        let from = std::mem::replace(&mut self.last, dark);
        let mut elapsed = 0u64;
        let mut explicit = params.ramp.steps() + net.settle_steps(self.coupling.clamp);
        while steps > 0 {
            let drive = params.ramp.drive(from, dark, elapsed);
            let (was_high, was_clamped) = (gate.high(), self.coupling.clamp);
            gate.tick();
            let high = usize::from(gate.high());
            let clamp = &mut self.coupling.clamp;
            net.settle_in(&tables.kernels, clamp, drive, &self.z, high);
            Network::kernel(&tables.kernels, *clamp).advance(drive, &mut self.z, high);
            count_clamped(&mut self.coupling, 1);
            steps -= 1;
            elapsed = elapsed.saturating_add(1);
            explicit = if gate.high() != was_high || self.coupling.clamp != was_clamped {
                // A new source or diode state excites the line capacitances again.
                net.settle_steps(self.coupling.clamp)
            } else {
                explicit.saturating_sub(1)
            };
            if explicit > 0 {
                continue;
            }

            let due = u64::from(gate.pending());
            let until_due = if due > 0 { due - 1 } else { steps };
            let clamp = self.coupling.clamp;
            let mut z: Vector = self.coupling.z;
            z[..M].copy_from_slice(&self.z);
            let level = level(high == 1);
            let quiet = until_due
                .min(steps)
                .min(net.quiet_steps(clamp, &z, dark, level));
            net.hold(clamp, &mut z, dark, level, quiet);
            self.z.copy_from_slice(&z[..M]);
            count_clamped(&mut self.coupling, quiet);
            gate.idle(quiet);
            steps -= quiet;
        }
    }
}

/// Returns whether a pin at `volts` reads high, against the threshold plus thermal noise.
///
/// A draw from the noise stream is taken only where the noise can change the reading.
#[inline]
fn read(latch: &Latch, noise: &mut Pcg32, volts: f64) -> bool {
    let margin = volts - latch.threshold_v;
    if margin.abs() >= latch.reach_v {
        margin > 0.0
    } else {
        margin > latch.noise_v * f64::from(noise.gauss())
    }
}

/// Counts `steps` sub-steps in the coupling's present diode state, for the probes.
#[inline]
fn count_clamped(coupling: &mut Coupling, steps: u64) {
    #[cfg(feature = "test-util")]
    if coupling.clamp != Clamp::Off {
        coupling.clamped += steps;
    }
    let _ = (coupling, steps);
}

/// Returns a logic level as the multiplier of a source's high voltage.
fn level(high: bool) -> f64 {
    f64::from(u8::from(high))
}

/// Node voltages of one circuit sub-step, recorded by [`trace`].
#[cfg(feature = "test-util")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probe {
    /// Node S, the source bus, in volts.
    pub s_v: f64,
    /// Node F, the gate input, in volts.
    pub f_v: f64,
    /// Node T, the target bus, in volts.
    pub t_v: f64,
    /// Current the input protection diodes push into node F, in amperes.
    pub clamp_a: f64,
    /// Whether the active gate's output is high; always false in bypass.
    pub gate_high: bool,
    /// Pin voltage of every bus line, indexed by bit number, in volts.
    ///
    /// Lines outside the circuit show their unloaded driver, 0 V or `v_oh`.
    pub pin_v: [f64; 16],
}

/// Runs the circuit for one pixel period per code, without blanking, probing each sub-step.
///
/// Appends one [`Probe`] per sub-step to `probes`. The state advances as in [`bend`]
/// on the nominal rail; the latched words are discarded. The drivers start from the
/// black code, as after blanking.
#[cfg(feature = "test-util")]
pub fn trace(params: &Params, state: &mut State, codes: &[u16], probes: &mut Vec<Probe>) {
    dispatch(params, state, Work::Trace { codes, probes });
}

/// Runs the circuit for one pixel period per code and replaces each code by the latched word.
///
/// The state advances as in [`bend`] on the nominal rail, with the image processor's
/// input noise of frame `index`, but without blanking. The drivers start from the
/// black code, as after blanking.
#[cfg(feature = "test-util")]
pub fn latch_words(params: &Params, state: &mut State, index: u64, codes: &mut [u16]) {
    state.noise = Pcg32::new(params.seed, index);
    dispatch(params, state, Work::Words(codes));
}

#[cfg(feature = "test-util")]
impl<G: Gate + Clone + Default, const M: usize> Run<'_, G, M> {
    fn trace(&mut self, codes: &[u16], probes: &mut Vec<Probe>) {
        let net = &self.params.net;
        let Self {
            core, gate, live, ..
        } = self;
        for &code in codes {
            core.pixel(&mut **gate, live, code, |clamp, drive, z, gate| {
                let solution = net.solution(clamp);
                let (s_v, t_v) = solution.probes.buses(drive, z, gate);
                let f_v = solution.f.at(0, drive, z, gate);
                let v_oh = solution.probes.v_oh();
                let mut pin_v: [f64; 16] =
                    std::array::from_fn(|bit| level((code >> bit) & 1 == 1) * v_oh);
                for (bit, volts) in solution.probes.pins(drive, z, gate) {
                    pin_v[usize::from(bit)] = volts;
                }
                probes.push(Probe {
                    s_v,
                    f_v,
                    t_v,
                    clamp_a: net.clamp_current(clamp, f_v),
                    gate_high: gate > 0.5,
                    pin_v,
                });
            });
        }
    }
}

/// Drives the active gate's input with a node-F waveform, one voltage per sub-step.
///
/// Appends the gate's output level during each sub-step to `out`: the inverter's in
/// invert mode, `Q` in divide mode, always low in bypass. Only the gate's state in
/// `state` advances.
#[cfg(feature = "test-util")]
pub fn drive_gate(params: &Params, state: &mut State, f_v: &[f64], out: &mut Vec<bool>) {
    fn drive(gate: &mut impl Gate, chips: &Chips, f_v: &[f64], out: &mut Vec<bool>) {
        for &volts in f_v {
            gate.tick();
            out.push(gate.high());
            gate.sense(volts, chips);
        }
    }
    match params.function {
        Function::Bypass => drive(&mut Wire, &params.chips, f_v, out),
        Function::Invert => drive(&mut state.inverter, &params.chips, f_v, out),
        Function::Divide => drive(&mut state.flip_flop, &params.chips, f_v, out),
    }
}
