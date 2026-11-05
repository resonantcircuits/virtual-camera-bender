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
//!   ADC converts the pedestal and the drivers hold the black code.
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
//!   its pin, a direct short at its bus node. The lines the bend touches return the
//!   latched bits; every other line keeps the ADC's bit. No bit is chosen at random:
//!   the same codes and state give the same bits, except where a pin voltage lies
//!   within the thermal noise of the threshold.
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
//! The gate delays, the minimum pulse, the setup time and `1 / ff_fmax_hz` are
//! multiplied by `pixel_period_s / native_pixel_period_s`, so at match-input
//! resolution they span the same number of pixel periods as on the camera's own clock.
//!
//! # Numerical method
//!
//! Time advances in `oversample` sub-steps per pixel period. The capacitor is replaced
//! by its backward-Euler companion, a conductance `C / dt` in parallel with a current
//! source `C / dt` times its voltage at the previous sub-step, and each protection
//! diode by a switch in series with its forward voltage and resistance, which leaves a
//! piecewise-linear resistive network with three states: no diode, the diodes to
//! ground, or the diodes to the supply conducting. Ideal wires merge nodes, so a
//! circuit of direct shorts has two or three unknown node voltages, and one with patch
//! resistors at most `bits + 3`. [`Params::resolve`] stamps and inverts the
//! conductance matrix of each state once, and tabulates every needed voltage (node F,
//! the capacitor voltage, each latched pin) as a linear function of the driver levels,
//! the gate output and the previous capacitor voltage. A sub-step solves F in the
//! previous sub-step's diode state, switches state if F contradicts it, and is then a
//! table lookup and two multiply-adds per voltage.
//!
//! A gate output changes at a sub-step boundary: from the first sub-step that ends at
//! or after its due time, and at least one sub-step after the input crossing that
//! causes it. The latch reads the voltages of the first sub-step that ends at or
//! after `latch_phase`.
//!
//! During horizontal and vertical blanking no samples are stored but the circuit keeps
//! running with the black code on the drivers. The drivers do not switch then, so the
//! capacitor voltage is advanced over the interval in closed form (the backward-Euler
//! recursion summed as a geometric series), split where a pending gate transition
//! completes and where a conducting diode turns off, both exactly. The approximation
//! is that the gates see no new input crossings during blanking: a crossing of F
//! caused by the black level is registered at the first sub-step of the next line.
//!
//! # State
//!
//! The capacitor voltage, the diodes' state, the gates' logic levels, pending
//! transitions and edge timers persist in [`State`] from frame to frame, as in the
//! camera.

mod circuit;
mod gates;

use std::fmt::{self, Display, Formatter};
use std::ops::RangeInclusive;
use std::str::FromStr;

use crate::adc::Codes;
use crate::camera::Adc;
use crate::noise::{self, Pcg32};
use crate::validate::{non_negative, positive, within};
use crate::{Camera, Error, Preset, Timing};
use circuit::{Clamp, Line, Network, Topology};
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
    /// Thresholds and delays of the logic chips.
    chips: Chips,
    /// The image processor's input latch.
    latch: Latch,
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
        let net = Network::build(&Topology {
            lines: &lines,
            sources,
            target: bus.target,
            function: bus.function,
            common_bus: bus.common_bus,
            series_ohm: bus.series_ohm,
            cap_f: bus.cap_uf * 1e-6,
            pot_ohm: bus.pot_kohm * 1e3,
            pot_node: bus.pot_node,
            gate_ohm: camera.logic.hc_r_out_ohm,
            v_oh: adc.v_oh,
            clamp_v: camera.logic.clamp_diode_v,
            clamp_ohm: camera.logic.clamp_r_ohm,
            leak_ohm: camera.logic.input_leak_mohm * 1e6,
            dt_s,
            latched,
            bits: adc.bits,
        })?;
        // Gate timing is a sub-line constant: scaled so it keeps its native length in
        // pixel periods at match-input resolution.
        let scale = timing.pixel_period_s / timing.native_pixel_period_s;
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
            chips: Chips::new(&camera.logic, scale, dt_s),
            latch: Latch {
                threshold_v: adc.dsp_threshold_v,
                noise_v,
                reach_v: NOISE_REACH * noise_v,
                step: latch_step(adc.latch_phase, oversample),
                mask: latched,
            },
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
/// Holds the capacitor's voltage, which input protection diodes conduct, the logic
/// levels, pending transitions and edge timers of the inverter and the flip-flop, and
/// the image processor's input-noise stream, which each frame reseeds from its index.
/// The default is the power-on state: the capacitor discharged, no diode conducting,
/// the inverter's output high for its low input, `Q` low.
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
}

/// Analog state around node F: the capacitor's charge and the protection diodes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Coupling {
    /// Capacitor voltage, node S minus node F, in volts.
    cap_v: f64,
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
        }
    }
}

#[cfg(feature = "test-util")]
impl State {
    /// Returns the capacitor voltage, node S minus node F, in volts.
    #[must_use]
    pub fn cap_v(&self) -> f64 {
        self.coupling.cap_v
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
/// # Panics
///
/// Panics if the codes' pixel period is not the one `params` was resolved for.
pub fn bend(params: Option<&Params>, state: &mut State, index: u64, codes: &mut Codes) {
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
    let oversample = u64::from(params.oversample);
    let line_len = layout.line_len() as usize;
    let hblank = u64::from(layout.hblank_px()) * oversample;
    let line_steps = u64::from(layout.line_len() + layout.hblank_px()) * oversample;
    let blank = Blanking {
        line: hblank,
        frame: hblank + u64::from(layout.vblank_lines()) * line_steps,
    };
    state.noise = Pcg32::new(params.seed, index);
    let State {
        coupling,
        inverter,
        flip_flop,
        noise,
    } = state;
    let words = codes.words_mut();
    match params.function {
        Function::Bypass => {
            Run::new(params, &mut Wire, noise, coupling).frame(words, line_len, blank);
        }
        Function::Invert => {
            Run::new(params, inverter, noise, coupling).frame(words, line_len, blank);
        }
        Function::Divide => {
            Run::new(params, flip_flop, noise, coupling).frame(words, line_len, blank);
        }
    }
}

/// Sub-steps of blanking after a line and after the last line of a frame.
#[derive(Debug, Clone, Copy)]
struct Blanking {
    line: u64,
    frame: u64,
}

/// The circuit running with one gate on the function switch.
struct Run<'a, G> {
    params: &'a Params,
    gate: &'a mut G,
    noise: &'a mut Pcg32,
    /// Where the coupling state is written back when the run ends.
    state: &'a mut Coupling,
    /// The coupling state at the end of the last sub-step.
    coupling: Coupling,
}

impl<'a, G: Gate> Run<'a, G> {
    fn new(
        params: &'a Params,
        gate: &'a mut G,
        noise: &'a mut Pcg32,
        state: &'a mut Coupling,
    ) -> Self {
        Self {
            params,
            gate,
            noise,
            coupling: *state,
            state,
        }
    }

    /// Runs a frame of `line_len` samples per line, each line followed by blanking.
    fn frame(mut self, words: &mut [u16], line_len: usize, blank: Blanking) {
        let lines = words.len() / line_len;
        for (line, samples) in words.chunks_exact_mut(line_len).enumerate() {
            for word in samples {
                *word = self.pixel(*word, |_, _, _, _| {});
            }
            self.blank(if line + 1 == lines {
                blank.frame
            } else {
                blank.line
            });
        }
        *self.state = self.coupling;
    }

    /// Runs one pixel period with `code` on the drivers and returns the latched word.
    ///
    /// `probe` sees the diode state, driver pattern, previous capacitor voltage and gate
    /// level of every sub-step, from which every node voltage of that sub-step follows.
    #[inline]
    fn pixel(&mut self, code: u16, mut probe: impl FnMut(Clamp, usize, f64, f64)) -> u16 {
        let params = self.params;
        let net = &params.net;
        let pattern = net.pattern(code);
        let mut latched = 0;
        for step in 0..params.oversample {
            self.gate.tick();
            let gate = level(self.gate.high());
            let c = &mut self.coupling;
            let f_v = net.settle(&mut c.clamp, pattern, c.cap_v, gate);
            probe(c.clamp, pattern, c.cap_v, gate);
            let solution = net.solution(c.clamp);
            if step == params.latch.step {
                latched = self.latch(pattern, gate);
            }
            self.coupling.cap_v = solution.cap.at(pattern, self.coupling.cap_v, gate);
            count_clamped(&mut self.coupling, 1);
            self.gate.sense(f_v, &params.chips);
        }
        (code & !params.latch.mask) | latched
    }

    /// Reads every latched pin against the threshold plus thermal noise.
    fn latch(&mut self, pattern: usize, gate: f64) -> u16 {
        let params = self.params;
        let latch = &params.latch;
        let pins = &params.net.solution(self.coupling.clamp).latched;
        let mut bits = 0;
        for (bit, volts) in pins.pins(pattern, self.coupling.cap_v, gate) {
            let margin = volts - latch.threshold_v;
            let high = if margin.abs() >= latch.reach_v {
                margin > 0.0
            } else {
                margin > latch.noise_v * f64::from(self.noise.gauss())
            };
            bits |= u16::from(high) << bit;
        }
        bits
    }

    /// Advances the circuit through `steps` sub-steps of blanking.
    ///
    /// Each round runs one sub-step explicitly, which settles the diodes after a change
    /// of the sources and completes a gate transition that falls due, and then as many
    /// sub-steps in closed form as the gate's pending transition and the diodes allow.
    fn blank(&mut self, mut steps: u64) {
        let params = self.params;
        let net = &params.net;
        let dark = net.pattern(params.dark_code);
        while steps > 0 {
            self.gate.tick();
            let gate = level(self.gate.high());
            let c = &mut self.coupling;
            net.settle(&mut c.clamp, dark, c.cap_v, gate);
            c.cap_v = net.solution(c.clamp).cap.at(dark, c.cap_v, gate);
            count_clamped(c, 1);
            steps -= 1;

            let due = u64::from(self.gate.pending());
            let until_due = if due > 0 { due - 1 } else { steps };
            let c = &mut self.coupling;
            let quiet = until_due
                .min(steps)
                .min(net.quiet_steps(c.clamp, c.cap_v, dark, gate));
            c.cap_v = net.hold(c.clamp, c.cap_v, dark, gate, quiet);
            count_clamped(c, quiet);
            self.gate.idle(quiet);
            steps -= quiet;
        }
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
/// Appends one [`Probe`] per sub-step to `probes`. The state advances as in [`bend`];
/// the latched words are discarded.
#[cfg(feature = "test-util")]
pub fn trace(params: &Params, state: &mut State, codes: &[u16], probes: &mut Vec<Probe>) {
    let State {
        coupling,
        inverter,
        flip_flop,
        noise,
    } = state;
    match params.function {
        Function::Bypass => Run::new(params, &mut Wire, noise, coupling).trace(codes, probes),
        Function::Invert => Run::new(params, inverter, noise, coupling).trace(codes, probes),
        Function::Divide => Run::new(params, flip_flop, noise, coupling).trace(codes, probes),
    }
}

#[cfg(feature = "test-util")]
impl<G: Gate> Run<'_, G> {
    fn trace(mut self, codes: &[u16], probes: &mut Vec<Probe>) {
        let net = &self.params.net;
        for &code in codes {
            self.pixel(code, |clamp, pattern, cap_v, gate| {
                let solution = net.solution(clamp);
                let (s_v, t_v) = solution.probes.buses(pattern, cap_v, gate);
                let f_v = solution.f.at(pattern, cap_v, gate);
                let v_oh = solution.probes.v_oh();
                let mut pin_v: [f64; 16] =
                    std::array::from_fn(|bit| level((code >> bit) & 1 == 1) * v_oh);
                for (bit, volts) in solution.probes.pins(pattern, cap_v, gate) {
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
        *self.state = self.coupling;
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
