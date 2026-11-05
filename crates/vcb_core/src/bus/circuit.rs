//! The bend circuit as a piecewise-linear resistive network around one capacitor.
//!
//! [`Network::build`] merges the nodes that ideal wires join, stamps the conductance
//! matrix of what is left, inverts it once and expresses every voltage the simulation
//! needs as a linear function of the circuit's sources:
//!
//! - the ADC line drivers, each 0 V or `v_oh` behind its output resistance,
//! - the gate driving node T, 0 V or `v_oh` behind the 74HC output resistance,
//! - the backward-Euler companion of the capacitor: a conductance `C / dt` between S
//!   and F in parallel with a current source `C / dt` times the capacitor voltage of
//!   the previous sub-step,
//! - the constant sources at node F: the gate inputs' leakage to half the supply and,
//!   while one conducts, an input protection diode.
//!
//! The protection diodes make the network piecewise linear. Each is an ideal switch in
//! series with `clamp_diode_v` and `clamp_r_ohm`, closed while F lies beyond the
//! ground or supply rail by more than the diode voltage, so the network has three
//! states ([`Clamp`]), each solved once. A sub-step picks the state consistent with F.
//!
//! The driver part of every voltage is tabulated over all level patterns of the tapped
//! lines, so one sub-step costs a table lookup and two multiply-adds per voltage.

use std::fmt::{self, Debug, Formatter};

use crate::Error;
use crate::bus::{Function, PotNode, Target};

/// Logical node of the circuit ground, the reference of every voltage.
const GND: usize = 0;
/// Logical node of the source bus S.
const S: usize = 1;
/// Logical node F between the capacitor and the gate inputs.
const F: usize = 2;
/// Logical node of the target bus T.
const T: usize = 3;
/// Logical node of the first tapped pin; tapped line `i` has node `PIN0 + i`.
const PIN0: usize = 4;

/// Gate inputs on node F: the inverter's input and the flip-flop's clock.
///
/// Each brings its own protection diodes and leakage, so their conductances double.
const GATE_INPUTS: f64 = 2.0;

/// Smallest pivot accepted when inverting the conductance matrix, relative to its largest entry.
///
/// A smaller pivot means a node without a conductive path to ground, whose voltage the
/// network does not define.
const MIN_PIVOT: f64 = 1e-12;

/// Which input protection diodes on node F conduct.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Clamp {
    /// None: F lies between the rails widened by the diode voltage.
    #[default]
    Off,
    /// The diodes to ground: F is below `-clamp_diode_v`.
    Ground,
    /// The diodes to the supply: F is above `v_oh + clamp_diode_v`.
    Rail,
}

impl Clamp {
    /// All states, in the order of [`index`](Self::index).
    const ALL: [Self; 3] = [Self::Off, Self::Ground, Self::Rail];

    fn index(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Ground => 1,
            Self::Rail => 2,
        }
    }
}

/// One bus line wired into the circuit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Line {
    /// Bit number `k` of line `Dk`.
    pub bit: u8,
    /// Resistance between the line's ideal driver and its pin in ohms.
    pub driver_ohm: f64,
}

/// The circuit's connections and element values, in SI units.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Topology<'a> {
    /// Tapped lines in ascending bit order: the source pins and the target pin.
    pub lines: &'a [Line],
    /// Bit mask of the lines in the source bank.
    pub sources: u16,
    /// Where the target selector connects node T.
    pub target: Target,
    /// The gate between node F and node T.
    pub function: Function,
    /// Whether the source and target banks share one bus wire.
    pub common_bus: bool,
    /// Patch resistor between each tapped pin and its bus in ohms; 0 is a direct wire.
    pub series_ohm: f64,
    /// Coupling capacitor in farads.
    pub cap_f: f64,
    /// Potentiometer to ground in ohms; 0 grounds its node.
    pub pot_ohm: f64,
    /// The node the potentiometer loads.
    pub pot_node: PotNode,
    /// Output resistance of the gate driving node T in ohms.
    pub gate_ohm: f64,
    /// Supply of the logic chips, high level of the drivers and the gate output, in volts.
    pub v_oh: f64,
    /// Forward voltage of an input protection diode in volts.
    pub clamp_v: f64,
    /// Series resistance of a conducting protection diode, per gate input, in ohms.
    pub clamp_ohm: f64,
    /// Leakage of a gate input as a resistance to half the supply, in ohms.
    pub leak_ohm: f64,
    /// Duration of one sub-step in seconds.
    pub dt_s: f64,
    /// Bit mask of the tapped lines whose pins the image processor latches.
    pub latched: u16,
    /// Width of the ADC codes in bits.
    pub bits: u8,
}

impl Topology<'_> {
    /// Returns the voltage behind the conducting protection diodes of `clamp`.
    fn clamp_source_v(&self, clamp: Clamp) -> f64 {
        match clamp {
            Clamp::Off => 0.0,
            Clamp::Ground => -self.clamp_v,
            Clamp::Rail => self.v_oh + self.clamp_v,
        }
    }

    /// Returns the conductance of the protection diodes of `clamp` on node F.
    fn clamp_siemens(&self, clamp: Clamp) -> f64 {
        match clamp {
            Clamp::Off => 0.0,
            Clamp::Ground | Clamp::Rail => GATE_INPUTS / self.clamp_ohm,
        }
    }
}

/// A voltage as a linear function of the sources, with the driver part tabulated.
#[derive(Clone, PartialEq)]
pub(crate) struct Response {
    /// Volts contributed by every pattern of high tapped drivers and the constant sources.
    table: Box<[f64]>,
    /// Volts per volt of the previous sub-step's capacitor voltage.
    cap: f64,
    /// Volts contributed by a high gate output.
    gate: f64,
}

impl Response {
    /// Returns the voltage for a driver pattern, previous capacitor voltage and gate level.
    #[inline]
    pub(crate) fn at(&self, pattern: usize, cap_v: f64, gate: f64) -> f64 {
        self.table[pattern] + self.cap * cap_v + self.gate * gate
    }
}

/// The pins the image processor latches, as functions of the sources.
#[derive(Clone, PartialEq)]
pub(crate) struct Latched {
    /// Bit number of every latched line.
    bits: Box<[u8]>,
    /// Driver part of every pin voltage, `bits.len()` consecutive entries per pattern.
    table: Box<[f64]>,
    /// Volts per volt of the previous capacitor voltage, per latched line.
    cap: Box<[f64]>,
    /// Volts contributed by a high gate output, per latched line.
    gate: Box<[f64]>,
}

impl Latched {
    /// Returns `(bit, volts)` for every latched pin at the given sources.
    #[inline]
    pub(crate) fn pins(
        &self,
        pattern: usize,
        cap_v: f64,
        gate: f64,
    ) -> impl Iterator<Item = (u8, f64)> + '_ {
        let count = self.bits.len();
        let row = &self.table[pattern * count..(pattern + 1) * count];
        self.bits
            .iter()
            .zip(row)
            .zip(self.cap.iter().zip(&self.gate))
            .map(move |((&bit, &drive), (&cap, &gain))| (bit, drive + cap * cap_v + gain * gate))
    }
}

/// A voltage as a linear function of the sources, one coefficient per tapped driver.
#[derive(Debug, Clone, PartialEq)]
struct Linear {
    /// Volts contributed by each tapped driver when high, in tapped-line order.
    drivers: Box<[f64]>,
    /// Volts contributed by the constant sources: leakage bias and a conducting diode.
    bias: f64,
    /// Volts per volt of the previous capacitor voltage.
    cap: f64,
    /// Volts contributed by a high gate output.
    gate: f64,
}

impl Linear {
    /// Tabulates the driver part and the bias over every pattern of high drivers.
    fn tabulate(&self) -> Box<[f64]> {
        let mut table = vec![self.bias; 1 << self.drivers.len()];
        for pattern in 1..table.len() {
            let lowest = pattern.trailing_zeros() as usize;
            table[pattern] = table[pattern & (pattern - 1)] + self.drivers[lowest];
        }
        table.into_boxed_slice()
    }

    fn response(&self) -> Response {
        Response {
            table: self.tabulate(),
            cap: self.cap,
            gate: self.gate,
        }
    }

    /// Returns the voltage for a driver pattern, previous capacitor voltage and gate level.
    #[cfg(feature = "test-util")]
    fn at(&self, pattern: usize, cap_v: f64, gate: f64) -> f64 {
        let drive: f64 = self
            .drivers
            .iter()
            .enumerate()
            .filter(|&(i, _)| (pattern >> i) & 1 == 1)
            .map(|(_, volts)| volts)
            .sum();
        self.bias + drive + self.cap * cap_v + self.gate * gate
    }
}

/// Voltages that only probes read: the buses and every tapped pin.
#[cfg(feature = "test-util")]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Probes {
    /// High level of the line drivers in volts.
    v_oh: f64,
    /// Bit number of every tapped line.
    bits: Box<[u8]>,
    /// Node S.
    s: Linear,
    /// Node T.
    t: Linear,
    /// The pin of every tapped line.
    pins: Box<[Linear]>,
}

#[cfg(feature = "test-util")]
impl Probes {
    /// Returns the high level of the line drivers in volts.
    pub(crate) fn v_oh(&self) -> f64 {
        self.v_oh
    }

    /// Returns the voltages of S and T at the given sources.
    pub(crate) fn buses(&self, pattern: usize, cap_v: f64, gate: f64) -> (f64, f64) {
        (
            self.s.at(pattern, cap_v, gate),
            self.t.at(pattern, cap_v, gate),
        )
    }

    /// Returns `(bit, volts)` for every tapped pin at the given sources.
    pub(crate) fn pins(
        &self,
        pattern: usize,
        cap_v: f64,
        gate: f64,
    ) -> impl Iterator<Item = (u8, f64)> + '_ {
        self.bits
            .iter()
            .zip(&self.pins)
            .map(move |(&bit, pin)| (bit, pin.at(pattern, cap_v, gate)))
    }
}

/// The network solved for one state of the protection diodes.
#[derive(Clone, PartialEq)]
pub(crate) struct Solution {
    /// The capacitor voltage, node S minus node F.
    pub cap: Response,
    /// Node F, the input of both gates.
    pub f: Response,
    /// The pins the image processor latches.
    pub latched: Latched,
    /// Bus and pin voltages for probes.
    #[cfg(feature = "test-util")]
    pub probes: Probes,
}

/// The resolved bend circuit: every needed voltage as a function of the sources.
#[derive(Clone, PartialEq)]
pub(crate) struct Network {
    /// Mask of the ADC's code bits.
    code_mask: u16,
    /// Pattern of tapped-line levels for every code; bit `i` is tapped line `i`.
    patterns: Box<[u16]>,
    /// The network in every state of the protection diodes, indexed by [`Clamp::index`].
    solutions: [Solution; 3],
    /// Voltage of node F below which the diodes to ground conduct.
    floor_v: f64,
    /// Voltage of node F above which the diodes to the supply conduct.
    ceiling_v: f64,
    /// Conductance of the conducting diodes on node F.
    #[cfg(feature = "test-util")]
    clamp_s: f64,
}

impl Network {
    /// Builds the network of a topology and solves it for every source and diode state.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a node has no conductive path to ground or the
    /// capacitor has no path to discharge, which the bend's topologies never produce.
    pub(crate) fn build(top: &Topology<'_>) -> Result<Self, Error> {
        let (index, unknowns) = merge(top).unknowns();
        let [off, ground, rail] = Clamp::ALL;
        let solutions = [
            solve(top, &index, unknowns, off)?,
            solve(top, &index, unknowns, ground)?,
            solve(top, &index, unknowns, rail)?,
        ];
        let code_mask = ((1u32 << top.bits) - 1) as u16;
        let patterns = (0..=code_mask)
            .map(|code| {
                top.lines
                    .iter()
                    .enumerate()
                    .fold(0u16, |pattern, (i, line)| {
                        pattern | (((code >> line.bit) & 1) << i)
                    })
            })
            .collect();
        Ok(Self {
            code_mask,
            patterns,
            solutions,
            floor_v: top.clamp_source_v(Clamp::Ground),
            ceiling_v: top.clamp_source_v(Clamp::Rail),
            #[cfg(feature = "test-util")]
            clamp_s: top.clamp_siemens(Clamp::Ground),
        })
    }

    /// Returns the pattern of tapped-line levels that `code` drives onto the bus.
    #[inline]
    pub(crate) fn pattern(&self, code: u16) -> usize {
        usize::from(self.patterns[usize::from(code & self.code_mask)])
    }

    /// Returns the network solved for a state of the protection diodes.
    #[inline]
    pub(crate) fn solution(&self, clamp: Clamp) -> &Solution {
        &self.solutions[clamp.index()]
    }

    /// Returns whether `clamp` is the diode state of node F at `f_v` volts.
    fn consistent(&self, clamp: Clamp, f_v: f64) -> bool {
        match clamp {
            Clamp::Off => (self.floor_v..=self.ceiling_v).contains(&f_v),
            Clamp::Ground => f_v <= self.floor_v,
            Clamp::Rail => f_v >= self.ceiling_v,
        }
    }

    /// Solves node F for a sub-step and moves `clamp` to the diode state consistent with it.
    ///
    /// Starts from the state of the previous sub-step. A diode that conducts pulls F
    /// towards the threshold it crossed, so a state that F leaves is replaced by the
    /// neighbouring one, which F then satisfies; two changes cover every jump.
    #[inline]
    pub(crate) fn settle(&self, clamp: &mut Clamp, pattern: usize, cap_v: f64, gate: f64) -> f64 {
        let mut f_v = self.solution(*clamp).f.at(pattern, cap_v, gate);
        for _ in 0..2 {
            if self.consistent(*clamp, f_v) {
                break;
            }
            *clamp = match *clamp {
                Clamp::Off if f_v < self.floor_v => Clamp::Ground,
                Clamp::Off => Clamp::Rail,
                Clamp::Ground | Clamp::Rail => Clamp::Off,
            };
            f_v = self.solution(*clamp).f.at(pattern, cap_v, gate);
        }
        f_v
    }

    /// Returns the capacitor voltage after `steps` sub-steps with constant sources.
    ///
    /// The backward-Euler update of the capacitor voltage is `v' = a + b v`, with `b`
    /// in `[0, 1)` when the sources and the diode state are constant. Summed in closed
    /// form, `steps` updates give `v_n = v_inf + (v_0 - v_inf) b^n` with
    /// `v_inf = a / (1 - b)`: exactly what stepping through them one by one computes.
    pub(crate) fn hold(
        &self,
        clamp: Clamp,
        cap_v: f64,
        pattern: usize,
        gate: f64,
        steps: u64,
    ) -> f64 {
        if steps == 0 {
            return cap_v;
        }
        let cap = &self.solution(clamp).cap;
        let settled = cap.at(pattern, 0.0, gate) / (1.0 - cap.cap);
        settled + (cap_v - settled) * cap.cap.powf(steps as f64)
    }

    /// Returns how many coming sub-steps with constant sources keep `clamp` consistent.
    ///
    /// Node F is affine in the capacitor voltage, so with constant sources it moves
    /// geometrically from its next value towards its settled value. The state holds
    /// for every sub-step if both lie in it, and otherwise until F crosses the state's
    /// boundary. Returns `u64::MAX` when the state never ends.
    pub(crate) fn quiet_steps(&self, clamp: Clamp, cap_v: f64, pattern: usize, gate: f64) -> u64 {
        let solution = self.solution(clamp);
        let retention = solution.cap.cap;
        let settled_cap = solution.cap.at(pattern, 0.0, gate) / (1.0 - retention);
        let next = solution.f.at(pattern, cap_v, gate);
        let settled = solution.f.at(pattern, settled_cap, gate);
        if !self.consistent(clamp, next) {
            return 0;
        }
        if self.consistent(clamp, settled) {
            return u64::MAX;
        }
        if retention <= 0.0 {
            return 1;
        }
        let bound = match clamp {
            Clamp::Ground => self.floor_v,
            Clamp::Off if settled < self.floor_v => self.floor_v,
            Clamp::Rail | Clamp::Off => self.ceiling_v,
        };
        // F at the j-th coming sub-step is `settled + (next - settled) b^(j - 1)`; it
        // stays on the far side of `bound` while `b^(j - 1)` is at least `ratio`.
        let ratio = (bound - settled) / (next - settled);
        let steps = (ratio.ln() / retention.ln()).floor() + 1.0;
        if steps.is_finite() {
            steps.max(0.0) as u64
        } else {
            u64::MAX
        }
    }

    /// Returns the current the protection diodes push into node F at `f_v`, in amperes.
    #[cfg(feature = "test-util")]
    pub(crate) fn clamp_current(&self, clamp: Clamp, f_v: f64) -> f64 {
        match clamp {
            Clamp::Off => 0.0,
            Clamp::Ground => self.clamp_s * (self.floor_v - f_v),
            Clamp::Rail => self.clamp_s * (self.ceiling_v - f_v),
        }
    }
}

impl Debug for Network {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let off = self.solution(Clamp::Off);
        f.debug_struct("Network")
            .field("latched_lines", &off.latched.bits)
            .field("cap_retention", &off.cap.cap)
            .field("floor_v", &self.floor_v)
            .field("ceiling_v", &self.ceiling_v)
            .finish_non_exhaustive()
    }
}

/// Solves the network for one state of the protection diodes.
fn solve(
    top: &Topology<'_>,
    index: &[Option<usize>],
    unknowns: usize,
    clamp: Clamp,
) -> Result<Solution, Error> {
    let node = |logical: usize| index[logical];
    let has_gate = top.function != Function::Bypass;
    let cap_s = top.cap_f / top.dt_s;
    // Current the constant sources inject into node F, per volt of F's response.
    let leak_s = GATE_INPUTS / top.leak_ohm;
    let bias_a = leak_s * top.v_oh / 2.0 + top.clamp_siemens(clamp) * top.clamp_source_v(clamp);
    let inverse = invert(&conductances(top, index, unknowns, clamp), unknowns)
        .ok_or_else(|| Error::config("bend.bus circuit leaves a node without a path to ground"))?;

    let respond = |weights: &[(usize, f64)]| {
        let mut row = vec![0.0; unknowns];
        for &(logical, weight) in weights {
            if let Some(k) = node(logical) {
                for (r, g) in row.iter_mut().zip(&inverse[k * unknowns..]) {
                    *r += weight * g;
                }
            }
        }
        let at = |logical: usize| node(logical).map_or(0.0, |k| row[k]);
        Linear {
            drivers: (0..top.lines.len())
                .map(|i| at(PIN0 + i) / top.lines[i].driver_ohm * top.v_oh)
                .collect(),
            bias: at(F) * bias_a,
            cap: (at(S) - at(F)) * cap_s,
            gate: if has_gate {
                at(T) / top.gate_ohm * top.v_oh
            } else {
                0.0
            },
        }
    };

    let cap = respond(&[(S, 1.0), (F, -1.0)]);
    if !(0.0..1.0).contains(&cap.cap) {
        return Err(Error::config(format!(
            "bend.bus capacitor has no path to discharge (retention {} per sub-step)",
            cap.cap
        )));
    }
    let latched = tabulate_pins(
        &(0..top.lines.len())
            .filter(|&i| top.latched & (1 << top.lines[i].bit) != 0)
            .map(|i| (top.lines[i].bit, respond(&[(PIN0 + i, 1.0)])))
            .collect::<Vec<_>>(),
        top.lines.len(),
    );
    Ok(Solution {
        cap: cap.response(),
        f: respond(&[(F, 1.0)]).response(),
        latched,
        #[cfg(feature = "test-util")]
        probes: Probes {
            v_oh: top.v_oh,
            bits: top.lines.iter().map(|line| line.bit).collect(),
            s: respond(&[(S, 1.0)]),
            t: respond(&[(T, 1.0)]),
            pins: (0..top.lines.len())
                .map(|i| respond(&[(PIN0 + i, 1.0)]))
                .collect(),
        },
    })
}

/// Stamps the conductance matrix of the merged nodes in one diode state.
///
/// Includes the capacitor's companion conductance and the gate inputs' leakage.
/// `index` maps every logical node to its unknown, `None` for ground.
fn conductances(
    top: &Topology<'_>,
    index: &[Option<usize>],
    unknowns: usize,
    clamp: Clamp,
) -> Vec<f64> {
    let mut matrix = vec![0.0; unknowns * unknowns];
    let mut stamp = |a: usize, b: usize, siemens: f64| {
        let (a, b) = (index[a], index[b]);
        if a == b {
            return;
        }
        for k in [a, b].into_iter().flatten() {
            matrix[k * unknowns + k] += siemens;
        }
        if let (Some(a), Some(b)) = (a, b) {
            matrix[a * unknowns + b] -= siemens;
            matrix[b * unknowns + a] -= siemens;
        }
    };
    let target_bit = match top.target {
        Target::Pin(bit) => Some(bit),
        Target::Gnd => None,
    };
    for (i, line) in top.lines.iter().enumerate() {
        stamp(PIN0 + i, GND, 1.0 / line.driver_ohm);
        if top.series_ohm > 0.0 {
            // A pin in both banks reaches the merged bus through its one patch resistor.
            if top.sources & (1 << line.bit) != 0 {
                stamp(PIN0 + i, S, 1.0 / top.series_ohm);
            } else if Some(line.bit) == target_bit {
                stamp(PIN0 + i, T, 1.0 / top.series_ohm);
            }
        }
    }
    if top.pot_ohm > 0.0 {
        stamp(pot_node(top), GND, 1.0 / top.pot_ohm);
    }
    stamp(S, F, top.cap_f / top.dt_s);
    if top.function != Function::Bypass {
        stamp(T, GND, 1.0 / top.gate_ohm);
    }
    stamp(
        F,
        GND,
        GATE_INPUTS / top.leak_ohm + top.clamp_siemens(clamp),
    );
    matrix
}

/// Returns the logical node the potentiometer loads.
fn pot_node(top: &Topology<'_>) -> usize {
    match top.pot_node {
        PotNode::F => F,
        PotNode::S => S,
    }
}

/// Joins the logical nodes that ideal wires and closed switches connect.
fn merge(top: &Topology<'_>) -> Nodes {
    let mut nodes = Nodes::new(PIN0 + top.lines.len());
    let target_bit = match top.target {
        Target::Pin(bit) => Some(bit),
        Target::Gnd => None,
    };
    let mut shared = false;
    for (i, line) in top.lines.iter().enumerate() {
        let source = top.sources & (1 << line.bit) != 0;
        let target = Some(line.bit) == target_bit;
        shared |= source && target;
        if top.series_ohm <= 0.0 {
            if source {
                nodes.join(PIN0 + i, S);
            }
            if target {
                nodes.join(PIN0 + i, T);
            }
        }
    }
    if top.common_bus || shared {
        nodes.join(S, T);
    }
    if top.target == Target::Gnd {
        nodes.join(T, GND);
    }
    if top.function == Function::Bypass {
        nodes.join(F, T);
    }
    if top.pot_ohm <= 0.0 {
        nodes.join(pot_node(top), GND);
    }
    nodes
}

/// Lays out the latched pins' driver tables with all pins of one pattern adjacent.
fn tabulate_pins(pins: &[(u8, Linear)], tapped: usize) -> Latched {
    let count = pins.len();
    let patterns = 1usize << tapped;
    let mut table = vec![0.0; patterns * count];
    for (l, (_, pin)) in pins.iter().enumerate() {
        for (pattern, volts) in pin.tabulate().iter().enumerate() {
            table[pattern * count + l] = *volts;
        }
    }
    Latched {
        bits: pins.iter().map(|(bit, _)| *bit).collect(),
        table: table.into_boxed_slice(),
        cap: pins.iter().map(|(_, pin)| pin.cap).collect(),
        gate: pins.iter().map(|(_, pin)| pin.gate).collect(),
    }
}

/// Disjoint sets of logical nodes; ground is always the root of its set.
struct Nodes {
    parent: Vec<usize>,
}

impl Nodes {
    fn new(count: usize) -> Self {
        Self {
            parent: (0..count).collect(),
        }
    }

    fn root(&self, mut node: usize) -> usize {
        while self.parent[node] != node {
            node = self.parent[node];
        }
        node
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        if a == GND {
            self.parent[b] = GND;
        } else {
            self.parent[a] = b;
        }
    }

    /// Numbers the merged nodes other than ground.
    ///
    /// Returns the unknown of every logical node (`None` for ground) and their count.
    fn unknowns(&self) -> (Vec<Option<usize>>, usize) {
        let mut number = vec![None; self.parent.len()];
        let mut count = 0;
        for node in 0..self.parent.len() {
            let root = self.root(node);
            if root != GND && number[root].is_none() {
                number[root] = Some(count);
                count += 1;
            }
        }
        let index = (0..self.parent.len())
            .map(|node| number[self.root(node)])
            .collect();
        (index, count)
    }
}

/// Inverts a dense `n` by `n` matrix by Gauss-Jordan elimination with partial pivoting.
///
/// Returns `None` if the matrix is singular to working precision.
fn invert(matrix: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut a = matrix.to_vec();
    let mut inverse = vec![0.0; n * n];
    for i in 0..n {
        inverse[i * n + i] = 1.0;
    }
    let largest = a.iter().fold(0.0f64, |max, v| max.max(v.abs()));
    for col in 0..n {
        let pivot =
            (col..n).max_by(|&i, &j| a[i * n + col].abs().total_cmp(&a[j * n + col].abs()))?;
        if a[pivot * n + col].abs() <= MIN_PIVOT * largest {
            return None;
        }
        for k in 0..n {
            a.swap(col * n + k, pivot * n + k);
            inverse.swap(col * n + k, pivot * n + k);
        }
        let scale = 1.0 / a[col * n + col];
        for k in 0..n {
            a[col * n + k] *= scale;
            inverse[col * n + k] *= scale;
        }
        for row in (0..n).filter(|&row| row != col) {
            let factor = a[row * n + col];
            for k in 0..n {
                a[row * n + k] -= factor * a[col * n + k];
                inverse[row * n + k] -= factor * inverse[col * n + k];
            }
        }
    }
    Some(inverse)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: [Line; 3] = [
        Line {
            bit: 8,
            driver_ohm: 160.0,
        },
        Line {
            bit: 9,
            driver_ohm: 150.0,
        },
        Line {
            bit: 10,
            driver_ohm: 140.0,
        },
    ];

    fn topology(pot_node: PotNode, common_bus: bool) -> Topology<'static> {
        Topology {
            lines: &LINES,
            sources: (1 << 9) | (1 << 8),
            target: Target::Pin(10),
            function: Function::Invert,
            common_bus,
            series_ohm: 47.0,
            cap_f: 1e-6,
            pot_ohm: 500.0,
            pot_node,
            gate_ohm: 90.0,
            v_oh: 3.3,
            clamp_v: 0.5,
            clamp_ohm: 50.0,
            leak_ohm: 3.3e6,
            dt_s: 1e-8,
            latched: 0b111 << 8,
            bits: 12,
        }
    }

    #[test]
    fn holding_the_sources_equals_stepping_through_them() {
        // The common bus lets the gate output reach the capacitor, so every term counts.
        let net = Network::build(&topology(PotNode::F, true)).unwrap();
        for clamp in Clamp::ALL {
            let cap = &net.solution(clamp).cap;
            assert!(cap.gate.abs() > 1e-6);
            for (pattern, gate) in [(0b011, 1.0), (0b100, 0.0), (0b000, 1.0)] {
                let mut stepped = 0.7;
                for _ in 0..20_000 {
                    stepped = cap.at(pattern, stepped, gate);
                }
                let held = net.hold(clamp, 0.7, pattern, gate, 20_000);
                assert!(
                    (held - stepped).abs() < 1e-9,
                    "{clamp:?}: held {held} V, stepped {stepped} V"
                );
            }
        }
    }

    #[test]
    fn a_clamp_holds_exactly_as_long_as_stepping_keeps_it_consistent() {
        // With the pot on S and every source low, a charged capacitor pulls F far below
        // ground; the diodes to ground conduct until the charge has drained.
        let net = Network::build(&topology(PotNode::S, false)).unwrap();
        let mut clamp = Clamp::Off;
        let cap_v = 2.5;
        net.settle(&mut clamp, 0, cap_v, 1.0);
        assert_eq!(clamp, Clamp::Ground);
        let quiet = net.quiet_steps(clamp, cap_v, 0, 1.0);
        let solution = net.solution(clamp);
        let mut v = cap_v;
        let mut stepped = 0;
        while net.consistent(clamp, solution.f.at(0, v, 1.0)) {
            v = solution.cap.at(0, v, 1.0);
            stepped += 1;
        }
        assert!(stepped > 10, "the clamp held only {stepped} sub-steps");
        assert!(
            quiet.abs_diff(stepped) <= 1,
            "{quiet} quiet steps, {stepped} stepped"
        );
    }
}
