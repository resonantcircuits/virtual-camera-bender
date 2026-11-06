//! The bend circuit as a piecewise-linear resistive network around its capacitances.
//!
//! [`Network::build`] merges the nodes that ideal wires join, stamps the conductance
//! matrix of what is left, inverts it once and expresses every voltage the simulation
//! needs as a linear function of the circuit's sources:
//!
//! - the ADC line drivers, each 0 V or `v_oh` behind its output resistance,
//! - the gate driving node T, 0 V or `v_oh` behind the 74HC output resistance,
//! - the backward-Euler companion of every capacitance: a conductance `C / dt` in
//!   parallel with a current source `C / dt` times the capacitance's voltage at the
//!   previous sub-step. The coupling capacitor joins S and F; the line capacitance of
//!   every tapped bus line and of the bend's wires S and T joins its node to ground,
//!   summed over the nodes an ideal wire merges,
//! - the constant sources at node F: the gate inputs' leakage to half the supply and,
//!   while one conducts, an input protection diode.
//!
//! The capacitances' voltages at the previous sub-step form the network's state: the
//! coupling capacitor's voltage first, then the voltage of every merged node with line
//! capacitance in the order of the unknowns. Without line capacitance the state is the
//! coupling capacitor's voltage alone.
//!
//! # Exact sub-steps
//!
//! The companion network gives the backward-Euler step `z' = A z + B u` of the state
//! `z` under the sources `u`. Every state variable is the voltage across one
//! capacitance `C_k`, so `A` is similar to a symmetric matrix through the scaling
//! `sqrt(C_k / dt)`, with real eigenvalues `λ = 1 / (1 + σ dt)` for the network's
//! decay rates `σ`. Replacing each by `e^(-σ dt) = e^(1 - 1 / λ)` on the same
//! eigenvectors turns the step into the exact response of the network to sources held
//! over the sub-step, `z' = Φ z + (I - Φ) z*(u)` with `z*` the steady state. This
//! matters for the line capacitances: their time constants of a few nanoseconds are
//! comparable to a sub-step at the native clock, where a backward-Euler step would let
//! a line keep several percent of each step at the latch instead of a few
//! microvolts. The voltages the simulation reads follow from the state and the sources
//! at the end of the sub-step as the companion network relates them.
//!
//! Where line capacitance sits on both ends of the coupling capacitor (in bypass, S
//! and T with F), the three capacitances form a loop and the state is redundant: the
//! capacitor's voltage is the difference of its nodes'. `A` then has an eigenvalue of
//! zero, whose mode the exact step also removes at once, and the relation between
//! voltages and state is taken on the other modes.
//!
//! The protection diodes make the network piecewise linear. Each is an ideal switch in
//! series with `clamp_diode_v` and `clamp_r_ohm`, closed while F lies beyond the
//! ground or supply rail by more than the diode voltage, so the network has three
//! states ([`Clamp`]), each solved once. A sub-step picks the state consistent with F.
//!
//! The driver part of every voltage is tabulated over all level patterns of the tapped
//! lines and both levels of the gate output, so one sub-step costs a table lookup and
//! one multiply-add per state variable for each voltage, in fixed-size arithmetic
//! ([`Kernel`]). While the drivers slew, a sub-step blends the tables of the old and
//! the new pattern. At the latch the voltages read are every tapped pin, for the image
//! processor and for the current the drivers source, and node T, for the current the
//! gate sources.
//!
//! With the bend's logic unpowered ([`Topology::gate_driven`] false) the gate's output
//! floats: node T loses its path through the 74HC output resistance and the gate's
//! level has no part in any voltage. The state's layout is the same either way, so
//! the simulation can move between the powered and the unpowered network.
//!
//! # Constant sources
//!
//! With the sources and the diode state held, the state follows `z' = Φ z + b`. The
//! line capacitances charge within a few sub-steps, the coupling capacitor over many
//! lines, so after a short transient only the slowest mode of `Φ` is left:
//! `z = z* + c r λ^n`, with `λ` its retention per sub-step, `r` its shape and `z*` the
//! fixed point. [`Network::settle_steps`] tells how many sub-steps the fast modes take
//! to fall below [`FAST_TOLERANCE`]; from then on [`Network::hold`] advances the state
//! in closed form. With the coupling capacitor alone this is the exact geometric sum
//! of the recursion.

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

/// Most state variables of a network: the coupling capacitor and every node with line capacitance.
///
/// The source bank spans `D2` to `D11` and the target bank lies within it, so at most
/// ten pins are tapped; with S and T that is at most 12 nodes besides the capacitor.
pub(crate) const MAX_STATES: usize = 13;

/// The state of the network: capacitor voltages in volts, the coupling capacitor first.
pub(crate) type Vector = [f64; MAX_STATES];

/// Most tapped pins: the source bank's ten lines, which hold the target bank.
pub(crate) const MAX_PINS: usize = 10;

/// Most voltages read at the latch: every tapped pin, then node T.
pub(crate) const MAX_SENSED: usize = MAX_PINS + 1;

/// Share of a deviation from the slow mode below which blanking may drop the fast modes.
///
/// The state's entries are voltages of a few volts, so the closed form errs by
/// picovolts, far below the image processor's input noise.
const FAST_TOLERANCE: f64 = 1e-12;

/// Sweeps of the Jacobi eigenvalue iteration, far more than a matrix of 13 rows needs.
///
/// Cyclic Jacobi converges quadratically; six to ten sweeps reach rounding level.
const JACOBI_SWEEPS: u32 = 50;

/// Longest settling of the fast modes counted, in sub-steps: 2^40 sub-steps is hours.
///
/// Fast modes that slow mean that blanking steps through every sub-step instead.
const MAX_SETTLE: u64 = 1 << 40;

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

    pub(crate) fn index(self) -> usize {
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
    /// Capacitance to ground of every tapped line and of the wires S and T, in farads.
    pub line_f: f64,
    /// Potentiometer to ground in ohms; 0 grounds its node.
    pub pot_ohm: f64,
    /// The node the potentiometer loads.
    pub pot_node: PotNode,
    /// Output resistance of the gate driving node T in ohms.
    pub gate_ohm: f64,
    /// Whether the gate's output drives node T; false while the logic is unpowered.
    pub gate_driven: bool,
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
    /// Width of the ADC codes in bits.
    pub bits: u8,
    /// How a pixel period falls onto sub-steps.
    pub sampling: Sampling<'a>,
}

/// How a pixel period falls onto sub-steps: their number, the drivers' slew, the latch.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sampling<'a> {
    /// Sub-steps per pixel period.
    pub oversample: usize,
    /// Mean fraction of a driver transition completed in each sub-step it spans.
    pub ramp: &'a [f64],
    /// Sub-step whose voltages the image processor latches.
    pub latch: usize,
}

impl Topology<'_> {
    /// Returns whether a gate drives node T.
    fn drives_t(&self) -> bool {
        self.function != Function::Bypass && self.gate_driven
    }

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

/// The levels of the line drivers during one sub-step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Drive {
    /// Every driver holds the levels of a pattern.
    Steady(usize),
    /// The drivers slew from one pattern to another.
    Ramp {
        /// Pattern before the transition.
        from: usize,
        /// Pattern after the transition.
        to: usize,
        /// Mean fraction of the transition completed during the sub-step.
        weight: f64,
    },
}

impl Drive {
    /// Returns the `N` voltages of a gated table under these levels and the gate level `gate`.
    ///
    /// A gated table holds `N` voltages for every pattern with the gate low, then high.
    #[inline]
    fn row<const N: usize>(self, table: &[f64], gate: usize) -> [f64; N] {
        let at = |pattern: usize, k: usize| table[((pattern << 1) | gate) * N + k];
        match self {
            Self::Steady(pattern) => std::array::from_fn(|k| at(pattern, k)),
            Self::Ramp { from, to, weight } => {
                std::array::from_fn(|k| weight * at(to, k) + (1.0 - weight) * at(from, k))
            }
        }
    }

    /// Returns voltage `k` of a gated table with `count` voltages per pattern and gate level.
    #[inline]
    fn select_gated(self, table: &[f64], count: usize, k: usize, gate: usize) -> f64 {
        let at = |pattern: usize| table[((pattern << 1) | gate) * count + k];
        match self {
            Self::Steady(pattern) => at(pattern),
            Self::Ramp { from, to, weight } => weight * at(to) + (1.0 - weight) * at(from),
        }
    }

    /// Returns voltage `k` of a table with `count` voltages per pattern under these levels.
    #[inline]
    fn select(self, table: &[f64], count: usize, k: usize) -> f64 {
        match self {
            Self::Steady(pattern) => table[pattern * count + k],
            Self::Ramp { from, to, weight } => {
                weight * table[to * count + k] + (1.0 - weight) * table[from * count + k]
            }
        }
    }
}

/// Voltages that are linear in the sources, with the driver part tabulated.
#[derive(Clone, PartialEq)]
pub(crate) struct Responses {
    /// Number of voltages.
    count: usize,
    /// Number of state variables.
    states: usize,
    /// Volts from every pattern of high tapped drivers and the constant sources, `count` per pattern.
    table: Box<[f64]>,
    /// The table with the gate's part added: `count` per pattern with the gate low, then high.
    gated: Box<[f64]>,
    /// Volts per volt of each previous state variable, `states` consecutive entries per voltage.
    state: Box<[f64]>,
    /// Volts contributed by a high gate output, per voltage.
    gate: Box<[f64]>,
}

impl Responses {
    /// Tabulates voltages given by their coefficients.
    fn new(voltages: &[Linear], tapped: usize, states: usize) -> Self {
        let count = voltages.len();
        let mut table = vec![0.0; count << tapped];
        for (k, voltage) in voltages.iter().enumerate() {
            for (pattern, volts) in voltage.tabulate().iter().enumerate() {
                table[pattern * count + k] = *volts;
            }
        }
        let gate: Box<[f64]> = voltages.iter().map(|voltage| voltage.gate).collect();
        let gated = table
            .chunks_exact(count.max(1))
            .flat_map(|row| {
                let high = row.iter().zip(&gate).map(|(volts, gain)| volts + gain);
                row.iter().copied().chain(high).collect::<Vec<_>>()
            })
            .collect();
        Self {
            count,
            states,
            table: table.into_boxed_slice(),
            gated,
            state: voltages
                .iter()
                .flat_map(|voltage| voltage.state.iter().copied())
                .collect(),
            gate,
        }
    }

    /// Returns voltage `k` for the driver levels, the previous state and the gate level.
    #[inline]
    pub(crate) fn at(&self, k: usize, drive: Drive, z: &[f64], gate: f64) -> f64 {
        let coefficients = &self.state[k * self.states..(k + 1) * self.states];
        let mut volts = drive.select(&self.table, self.count, k);
        for (coefficient, previous) in coefficients.iter().zip(z) {
            volts += coefficient * previous;
        }
        volts + self.gate[k] * gate
    }

    /// Returns the constant part of voltage `k` for a steady pattern and gate level.
    fn source(&self, k: usize, pattern: usize, gate: f64) -> f64 {
        self.table[pattern * self.count + k] + self.gate[k] * gate
    }

    /// Returns the coefficient of state variable `j` in voltage `k`.
    fn coefficient(&self, k: usize, j: usize) -> f64 {
        self.state[k * self.states + j]
    }
}

/// The voltages read at the latch, as functions of the sources.
#[derive(Clone, PartialEq)]
pub(crate) struct Latched {
    /// Bit number of every tapped line.
    bits: Box<[u8]>,
    /// The pin voltage of every tapped line, in the order of `bits`, then node T.
    volts: Responses,
}

/// What the rail feeds into the circuit: the sources' high level and conductances.
///
/// The current the rail delivers is the sum over the sources that are high of their
/// level minus the node they drive, times their conductance, less what the protection
/// diodes to the supply return.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Supply {
    /// High level of the line drivers and the gate output, in volts.
    pub v_oh: f64,
    /// Conductance from each tapped line's driver to its pin, in siemens, in tapped-line order.
    pub line_s: [f64; MAX_PINS],
    /// Conductance from the gate's output to node T, in siemens; 0 without a driven gate.
    pub gate_s: f64,
    /// Conductance of the conducting protection diodes on node F, in siemens.
    pub clamp_s: f64,
    /// Voltage of node F above which the diodes to the supply conduct.
    pub ceiling_v: f64,
}

/// A voltage as a linear function of the sources, one coefficient per tapped driver.
#[derive(Debug, Clone, PartialEq)]
struct Linear {
    /// Volts contributed by each tapped driver when high, in tapped-line order.
    drivers: Box<[f64]>,
    /// Volts contributed by the constant sources: leakage bias and a conducting diode.
    bias: f64,
    /// Volts per volt of each previous state variable.
    state: Box<[f64]>,
    /// Volts contributed by a high gate output.
    gate: f64,
}

impl Linear {
    /// Returns the coefficients of the sources: every driver, then the bias, then the gate.
    fn inputs(&self) -> Vec<f64> {
        let mut inputs = self.drivers.to_vec();
        inputs.extend([self.bias, self.gate]);
        inputs
    }

    /// Builds a voltage from the coefficients of [`inputs`](Self::inputs) and of the state.
    fn from_parts(inputs: &[f64], state: Vec<f64>) -> Self {
        let drivers = inputs.len() - 2;
        Self {
            drivers: inputs[..drivers].into(),
            bias: inputs[drivers],
            state: state.into_boxed_slice(),
            gate: inputs[drivers + 1],
        }
    }

    /// Tabulates the driver part and the bias over every pattern of high drivers.
    fn tabulate(&self) -> Box<[f64]> {
        let mut table = vec![self.bias; 1 << self.drivers.len()];
        for pattern in 1..table.len() {
            let lowest = pattern.trailing_zeros() as usize;
            table[pattern] = table[pattern & (pattern - 1)] + self.drivers[lowest];
        }
        table.into_boxed_slice()
    }
}

/// Voltages that only probes read: the buses and every tapped pin.
#[cfg(feature = "test-util")]
#[derive(Clone, PartialEq)]
pub(crate) struct Probes {
    /// High level of the line drivers in volts.
    v_oh: f64,
    /// Bit number of every tapped line.
    bits: Box<[u8]>,
    /// Node S, node T, then the pin of every tapped line.
    volts: Responses,
}

#[cfg(feature = "test-util")]
impl Probes {
    /// Returns the high level of the line drivers in volts.
    pub(crate) fn v_oh(&self) -> f64 {
        self.v_oh
    }

    /// Returns the voltages of S and T at the given sources and state.
    pub(crate) fn buses(&self, drive: Drive, z: &[f64], gate: f64) -> (f64, f64) {
        (
            self.volts.at(0, drive, z, gate),
            self.volts.at(1, drive, z, gate),
        )
    }

    /// Returns `(bit, volts)` for every tapped pin at the given sources and state.
    pub(crate) fn pins<'a>(
        &'a self,
        drive: Drive,
        z: &'a [f64],
        gate: f64,
    ) -> impl Iterator<Item = (u8, f64)> + 'a {
        self.bits
            .iter()
            .enumerate()
            .map(move |(i, &bit)| (bit, self.volts.at(2 + i, drive, z, gate)))
    }
}

/// The slowest mode of the state recursion with constant sources.
#[derive(Debug, Clone, PartialEq)]
struct Slow {
    /// Share of the mode's amplitude left after one sub-step.
    retention: f64,
    /// The mode's shape, largest entry 1.
    right: Vector,
    /// Projection onto the mode: the left eigenvector, scaled so `left · right = 1`.
    left: Vector,
    /// Sub-steps the faster modes take to fall below [`FAST_TOLERANCE`]; `u64::MAX` for never.
    settle: u64,
    /// `(I - A)^-1`, row major, which maps the constant sources to the fixed point.
    fixed: Box<[f64]>,
}

impl Slow {
    /// Describes the slowest mode of the exact step from its eigen-decomposition.
    fn new(exact: &Exact) -> Self {
        let m = exact.states;
        let mut unit = [0.0; MAX_STATES];
        unit[0] = 1.0;
        if m == 1 {
            return Self {
                retention: exact.retentions[0],
                right: unit,
                left: unit,
                settle: 0,
                fixed: Box::new([]),
            };
        }
        let slowest = (0..m)
            .max_by(|&i, &j| exact.retentions[i].total_cmp(&exact.retentions[j]))
            .unwrap_or(0);
        let vector = |i: usize| (0..m).map(move |k| exact.vectors[k * m + i]);
        // Φ = S^-1 Q M Qᵀ S: a mode's right vector is S^-1 q, its left vector S q.
        let mut right = [0.0; MAX_STATES];
        let mut left = [0.0; MAX_STATES];
        for (k, q) in vector(slowest).enumerate() {
            right[k] = q / exact.scale[k];
            left[k] = q * exact.scale[k];
        }
        let largest = right[..m].iter().fold(0.0f64, |max, v| max.max(v.abs()));
        for k in 0..m {
            right[k] /= largest;
            left[k] *= largest;
        }
        // The fast modes' share of a deviation after `n` sub-steps is at most
        // `Σ μ_i^n |S^-1 q_i|_max |S q_i|_1`.
        let weights: Vec<(f64, f64)> = (0..m)
            .filter(|&i| i != slowest)
            .map(|i| {
                let out = vector(i)
                    .enumerate()
                    .map(|(k, q)| (q / exact.scale[k]).abs())
                    .fold(0.0, f64::max);
                let into: f64 = vector(i)
                    .enumerate()
                    .map(|(k, q)| (q * exact.scale[k]).abs())
                    .sum();
                (exact.retentions[i], out * into)
            })
            .collect();
        let residual = |n: u64| -> f64 {
            weights
                .iter()
                .map(|&(retention, weight)| weight * retention.powf(n as f64))
                .sum()
        };
        let settle = if residual(MAX_SETTLE) >= FAST_TOLERANCE {
            u64::MAX
        } else {
            // The residual falls with `n`: bisect for the first sub-step below tolerance.
            let (mut low, mut high) = (0u64, MAX_SETTLE);
            while low < high {
                let mid = low + (high - low) / 2;
                if residual(mid) < FAST_TOLERANCE {
                    high = mid;
                } else {
                    low = mid + 1;
                }
            }
            low
        };
        let fixed = invert(
            &add(
                &identity(m),
                &exact.transition.iter().map(|v| -v).collect::<Vec<_>>(),
            ),
            m,
        )
        .unwrap_or_default()
        .into_boxed_slice();
        Self {
            retention: exact.retentions[slowest],
            right,
            left,
            settle,
            fixed,
        }
    }
}

/// Requires a retention per sub-step in `[0, 1)`: the capacitances discharge.
fn check_retention(retention: f64) -> Result<(), Error> {
    if (0.0..1.0).contains(&retention) {
        Ok(())
    } else {
        Err(Error::config(format!(
            "bend.bus capacitor has no path to discharge (retention {retention} per sub-step)"
        )))
    }
}

/// Multiplies an `m` by `m` matrix with an `m` by `columns` matrix, row major.
fn product(a: &[f64], b: &[f64], m: usize, columns: usize) -> Vec<f64> {
    let mut product = vec![0.0; m * columns];
    for r in 0..m {
        for k in 0..m {
            let factor = a[r * m + k];
            for c in 0..columns {
                product[r * columns + c] += factor * b[k * columns + c];
            }
        }
    }
    product
}

/// Returns the `m` by `m` identity matrix, row major.
fn identity(m: usize) -> Vec<f64> {
    (0..m * m)
        .map(|i| f64::from(u8::from(i / m == i % m)))
        .collect()
}

/// Adds two matrices of equal shape.
fn add(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

/// Diagonalises a symmetric `m` by `m` matrix by cyclic Jacobi rotations.
///
/// Returns the eigenvalues and the orthonormal eigenvectors as the columns of a row-major matrix.
fn jacobi(mut matrix: Vec<f64>, m: usize) -> (Vec<f64>, Vec<f64>) {
    let mut vectors = identity(m);
    for _ in 0..JACOBI_SWEEPS {
        let off: f64 = (0..m)
            .flat_map(|p| (p + 1..m).map(move |q| (p, q)))
            .map(|(p, q)| matrix[p * m + q].powi(2))
            .sum();
        let diagonal: f64 = (0..m).map(|k| matrix[k * m + k].powi(2)).sum();
        if off <= f64::EPSILON * f64::EPSILON * diagonal {
            break;
        }
        for p in 0..m {
            for q in p + 1..m {
                let apq = matrix[p * m + q];
                if apq == 0.0 {
                    continue;
                }
                // The rotation that zeroes a[p][q] (Numerical Recipes, section 11.1).
                let theta = (matrix[q * m + q] - matrix[p * m + p]) / (2.0 * apq);
                let tangent = theta.signum() / (theta.abs() + theta.mul_add(theta, 1.0).sqrt());
                let cos = 1.0 / tangent.mul_add(tangent, 1.0).sqrt();
                let sin = tangent * cos;
                let rotate = |values: &mut [f64], first: usize, second: usize| {
                    let (x, y) = (values[first], values[second]);
                    values[first] = cos * x - sin * y;
                    values[second] = sin * x + cos * y;
                };
                for k in 0..m {
                    rotate(&mut matrix, k * m + p, k * m + q);
                }
                for k in 0..m {
                    rotate(&mut matrix, p * m + k, q * m + k);
                }
                for k in 0..m {
                    rotate(&mut vectors, k * m + p, k * m + q);
                }
            }
        }
    }
    ((0..m).map(|k| matrix[k * m + k]).collect(), vectors)
}

/// The exact sub-step of the network, derived from its backward-Euler step.
///
/// The backward-Euler step is `z' = A z + B u`, a voltage the companion network gives
/// as `h z + d u` (from the state one sub-step back and the present sources). The
/// exact step is `z' = Φ z + E u` with `E = (I - Φ) z*` and the steady state
/// `z* = (I - A)^-1 B u`. The companion network ties the voltages to the new state:
/// `h z_prev + d u = h A^-1 (z' - B u) + d u`, so at the end of an exact sub-step the
/// voltage is `h A^-1 Φ z + (h (I - A^-1 Φ) z* + d) u`. On every mode `A^-1 Φ` is
/// `μ / λ`, which is finite as `λ` vanishes.
struct Exact {
    /// Number of state variables.
    states: usize,
    /// Number of sources.
    sources: usize,
    /// `Φ`, the exact state matrix.
    transition: Vec<f64>,
    /// `E`, the exact response to the sources.
    driven: Vec<f64>,
    /// `A^-1 Φ`.
    voltage_state: Vec<f64>,
    /// `A^-1 (E - B)`.
    voltage_sources: Vec<f64>,
    /// Eigenvalues of `Φ`: the retention of every mode per sub-step.
    retentions: Vec<f64>,
    /// Eigenvectors in the scaled coordinates, as columns.
    vectors: Vec<f64>,
    /// The scaling `sqrt(C_k / dt)` that makes `A` symmetric.
    scale: Vec<f64>,
}

impl Exact {
    /// Derives the exact step from the backward-Euler step of the state voltages.
    ///
    /// `caps_f` holds the capacitance behind every state variable.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a capacitance has no path to discharge.
    fn new(step: &[Linear], caps_f: &[f64]) -> Result<Self, Error> {
        let m = step.len();
        let sources = step.first().map_or(0, |v| v.inputs().len());
        let euler: Vec<f64> = step.iter().flat_map(|v| v.state.iter().copied()).collect();
        let inputs: Vec<f64> = step.iter().flat_map(Linear::inputs).collect();
        let scale: Vec<f64> = caps_f.iter().map(|cap| cap.sqrt()).collect();
        let symmetric: Vec<f64> = (0..m * m)
            .map(|i| {
                let (row, col) = (i / m, i % m);
                let forward = scale[row] * euler[i] / scale[col];
                let backward = scale[col] * euler[col * m + row] / scale[row];
                f64::midpoint(forward, backward)
            })
            .collect();
        let (eigenvalues, vectors) = jacobi(symmetric, m);
        let mut retentions = Vec::with_capacity(m);
        for &lambda in &eigenvalues {
            // Rounding can leave the eigenvalue of a very stiff mode a hair below zero.
            check_retention(lambda.max(0.0))?;
            retentions.push(if lambda > 0.0 {
                (1.0 - 1.0 / lambda).exp()
            } else {
                0.0
            });
        }
        // A function of `A` on its eigenvectors: S^-1 Q diag(f(λ)) Qᵀ S.
        let on_modes = |f: &dyn Fn(usize) -> f64| -> Vec<f64> {
            (0..m * m)
                .map(|i| {
                    let (row, col) = (i / m, i % m);
                    let sum: f64 = (0..m)
                        .map(|k| vectors[row * m + k] * f(k) * vectors[col * m + k])
                        .sum();
                    sum * scale[col] / scale[row]
                })
                .collect()
        };
        let transition = on_modes(&|k| retentions[k]);
        // `A^-1 Φ`, through `μ / λ`, which stays finite where `λ` vanishes: a capacitor
        // loop (the coupling capacitor between two charged nodes) makes `A` singular.
        let ratio = on_modes(&|k| {
            if eigenvalues[k] > 0.0 {
                retentions[k] / eigenvalues[k]
            } else {
                0.0
            }
        });
        let settled = invert(
            &add(&identity(m), &euler.iter().map(|v| -v).collect::<Vec<_>>()),
            m,
        )
        .ok_or_else(|| Error::config("bend.bus circuit has a capacitance that never discharges"))?;
        let steady = product(&settled, &inputs, m, sources);
        let decayed = add(
            &identity(m),
            &transition.iter().map(|v| -v).collect::<Vec<_>>(),
        );
        let driven = product(&decayed, &steady, m, sources);
        // `A^-1 (E - B) = (I - A^-1 Φ) z*`.
        let voltage_sources = product(
            &add(&identity(m), &ratio.iter().map(|v| -v).collect::<Vec<_>>()),
            &steady,
            m,
            sources,
        );
        Ok(Self {
            states: m,
            sources,
            transition,
            driven,
            voltage_state: ratio,
            voltage_sources,
            retentions,
            vectors,
            scale,
        })
    }

    /// Returns state variable `k` at the end of an exact sub-step.
    fn next(&self, k: usize) -> Linear {
        let (m, q) = (self.states, self.sources);
        Linear::from_parts(
            &self.driven[k * q..(k + 1) * q],
            self.transition[k * m..(k + 1) * m].to_vec(),
        )
    }

    /// Returns a voltage of the companion network at the end of an exact sub-step.
    fn output(&self, voltage: &Linear) -> Linear {
        let (m, q) = (self.states, self.sources);
        let state = (0..m)
            .map(|c| {
                (0..m)
                    .map(|k| voltage.state[k] * self.voltage_state[k * m + c])
                    .sum()
            })
            .collect();
        let mut inputs = voltage.inputs();
        for (c, input) in inputs.iter_mut().enumerate() {
            *input += (0..m)
                .map(|k| voltage.state[k] * self.voltage_sources[k * q + c])
                .sum::<f64>();
        }
        Linear::from_parts(&inputs, state)
    }
}

/// The network solved for one state of the protection diodes.
#[derive(Clone, PartialEq)]
pub(crate) struct Solution {
    /// The state at the end of the sub-step: the capacitor voltage, then the line nodes.
    next: Responses,
    /// Node F, the input of both gates.
    pub f: Responses,
    /// The pins the image processor latches.
    pub latched: Latched,
    /// The slowest mode of the state with constant sources.
    slow: Slow,
    /// Bus and pin voltages for probes.
    #[cfg(feature = "test-util")]
    pub probes: Probes,
}

/// The arithmetic of one sub-step in one diode state, for `M` state variables.
///
/// The same voltages as [`Solution`]'s next state, node F and latched pins, with the
/// gate's part in the tables and the coefficients in fixed-size arrays, so a sub-step
/// compiles to straight-line code. The gate level is an index: 0 low, 1 high.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Kernel<'a, const M: usize> {
    /// Gated driver part of the next state, `M` values per pattern and gate level.
    next: &'a [f64],
    /// Volts of each next state variable per volt of each previous one.
    next_state: [[f64; M]; M],
    /// Gated driver part of node F, one value per pattern and gate level.
    f: &'a [f64],
    /// Volts of node F per volt of each previous state variable.
    f_state: [f64; M],
    /// Gated driver part of the sensed voltages, `pin_count + 1` values per pattern and gate level.
    pins: &'a [f64],
    /// Number of tapped pins; node T follows them.
    pin_count: usize,
    /// Bit number of every tapped pin.
    pin_bits: [u8; MAX_PINS],
    /// Volts of every tapped pin, then node T, per volt of each previous state variable.
    pin_state: [[f64; M]; MAX_SENSED],
}

impl<'a, const M: usize> Kernel<'a, M> {
    fn new(solution: &'a Solution) -> Self {
        let (next, f, latched) = (&solution.next, &solution.f, &solution.latched);
        assert_eq!(
            next.states, M,
            "a kernel of {M} states for a network of {}",
            next.states
        );
        let pin_count = latched.bits.len();
        assert!(
            pin_count <= MAX_PINS,
            "{pin_count} tapped pins, more than {MAX_PINS}"
        );
        Self {
            next: &next.gated,
            next_state: std::array::from_fn(|k| std::array::from_fn(|j| next.coefficient(k, j))),
            f: &f.gated,
            f_state: std::array::from_fn(|j| f.coefficient(0, j)),
            pins: &latched.volts.gated,
            pin_count,
            pin_bits: std::array::from_fn(|i| latched.bits.get(i).copied().unwrap_or(0)),
            pin_state: std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    if i <= pin_count {
                        latched.volts.coefficient(i, j)
                    } else {
                        0.0
                    }
                })
            }),
        }
    }

    /// Returns node F for the driver levels, the previous state and the gate level.
    #[inline]
    fn f(&self, drive: Drive, z: &[f64; M], gate: usize) -> f64 {
        let [driven] = drive.row::<1>(self.f, gate);
        affine(driven, &self.f_state, z)
    }

    /// Advances the state through one sub-step.
    #[inline]
    pub(crate) fn advance(&self, drive: Drive, z: &mut [f64; M], gate: usize) {
        let previous = *z;
        let driven = drive.row::<M>(self.next, gate);
        for ((volts, driven), coefficients) in z.iter_mut().zip(driven).zip(&self.next_state) {
            *volts = affine(driven, coefficients, &previous);
        }
    }

    /// Returns `(bit, volts)` for every tapped pin at the given sources and state.
    #[inline]
    pub(crate) fn pins<'k>(
        &'k self,
        drive: Drive,
        z: &'k [f64; M],
        gate: usize,
    ) -> impl Iterator<Item = (u8, f64)> + 'k {
        (0..self.pin_count).map(move |i| {
            let driven = drive.select_gated(self.pins, self.pin_count + 1, i, gate);
            (self.pin_bits[i], affine(driven, &self.pin_state[i], z))
        })
    }

    /// Returns node T at the given sources and state.
    #[inline]
    pub(crate) fn t_v(&self, drive: Drive, z: &[f64; M], gate: usize) -> f64 {
        let t = self.pin_count;
        let driven = drive.select_gated(self.pins, t + 1, t, gate);
        affine(driven, &self.pin_state[t], z)
    }
}

/// Returns `constant + a · b`.
#[inline]
fn affine<const M: usize>(constant: f64, a: &[f64; M], b: &[f64; M]) -> f64 {
    constant + a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>()
}

/// One pixel period with the driver pattern, the gate level and the diode state held.
///
/// Within a pixel the drivers change only at its start (slewing over the first
/// sub-steps), so node F at every sub-step, the latched pins and the state at the end
/// are each linear in the state at the start of the pixel. With `Φ` the exact step,
/// `S_k = I + Φ + ... + Φ^(k-1)` and `Q_k = Σ_(j<k) Φ^(k-1-j) (1 - w_j)` for the ramp's
/// weights `w_j`, the state after `k` sub-steps is
/// `z_k = Φ^k z_0 + S_k T[to] + Q_k (T[from] - T[to])`, where `T` is the driven part of
/// the next state for a pattern. Every table here folds one such term into a value per
/// pattern, so a pixel costs one product with the start state per voltage, and no
/// voltage waits for another.
#[derive(Clone, PartialEq)]
pub(crate) struct Period {
    /// Node F at sub-step `k` per volt of each start state variable, `states` per sub-step.
    f_gain: Box<[f64]>,
    /// Node F at every sub-step for a steady pattern, `oversample` per pattern.
    f_steady: Box<[f64]>,
    /// The share of a slewing pixel's old pattern in node F, `oversample` per pattern.
    f_ramp: Box<[f64]>,
    /// The sensed voltages (tapped pins, then node T) per volt of each start state variable, `states` per voltage.
    pin_gain: Box<[f64]>,
    /// The sensed voltages for a steady pattern, one per voltage and pattern.
    pin_steady: Box<[f64]>,
    /// The share of a slewing pixel's old pattern in the sensed voltages.
    pin_ramp: Box<[f64]>,
    /// The state at the end per volt of the start state, `Φ^oversample`.
    end_gain: Box<[f64]>,
    /// The end state for a steady pattern, `states` per pattern.
    end_steady: Box<[f64]>,
    /// The share of a slewing pixel's old pattern in the end state.
    end_ramp: Box<[f64]>,
}

impl Period {
    /// Tabulates a pixel period of `solution` with the gate at `gate` (0 low, 1 high).
    fn new(solution: &Solution, gate: usize, sampling: &Sampling<'_>, patterns: usize) -> Self {
        let next = &solution.next;
        let m = next.states;
        let pins = solution.latched.bits.len() + 1;
        let steps = sampling.oversample;
        let slew = |k: usize| 1.0 - sampling.ramp.get(k).copied().unwrap_or(1.0);
        let phi = &next.state;
        // Φ^k, S_k and Q_k for k = 0 ..= oversample.
        let mut powers = vec![identity(m)];
        let mut sums = vec![vec![0.0; m * m]];
        let mut ramps = vec![vec![0.0; m * m]];
        for k in 0..steps {
            powers.push(product(phi, &powers[k], m, m));
            sums.push(add(&product(phi, &sums[k], m, m), &identity(m)));
            let scaled: Vec<f64> = identity(m).iter().map(|v| v * slew(k)).collect();
            ramps.push(add(&product(phi, &ramps[k], m, m), &scaled));
        }
        let driven = |p: usize| &next.gated[((p << 1) | gate) * m..(((p << 1) | gate) + 1) * m];
        let apply = |matrix: &[f64], vector: &[f64]| -> Vec<f64> {
            (0..m)
                .map(|r| (0..m).map(|c| matrix[r * m + c] * vector[c]).sum())
                .collect()
        };
        let row_times = |row: &[f64], vector: &[f64]| -> f64 {
            row.iter().zip(vector).map(|(a, b)| a * b).sum()
        };
        let row_matrix = |row: &[f64], matrix: &[f64]| -> Vec<f64> {
            (0..m)
                .map(|c| (0..m).map(|k| row[k] * matrix[k * m + c]).sum())
                .collect()
        };
        let f_row: Vec<f64> = (0..m).map(|j| solution.f.coefficient(0, j)).collect();
        let f_at = |p: usize| solution.f.gated[(p << 1) | gate];
        let mut f_gain = Vec::with_capacity(steps * m);
        for power in &powers[..steps] {
            f_gain.extend(row_matrix(&f_row, power));
        }
        let mut f_steady = Vec::with_capacity(patterns * steps);
        let mut f_ramp = Vec::with_capacity(patterns * steps);
        for p in 0..patterns {
            for k in 0..steps {
                f_steady.push(f_at(p) + row_times(&f_row, &apply(&sums[k], driven(p))));
                f_ramp.push(slew(k) * f_at(p) + row_times(&f_row, &apply(&ramps[k], driven(p))));
            }
        }
        let latch = sampling.latch;
        let pin_rows: Vec<Vec<f64>> = (0..pins)
            .map(|i| {
                (0..m)
                    .map(|j| solution.latched.volts.coefficient(i, j))
                    .collect()
            })
            .collect();
        let pin_at =
            |p: usize, i: usize| solution.latched.volts.gated[((p << 1) | gate) * pins + i];
        let pin_gain = pin_rows
            .iter()
            .flat_map(|row| row_matrix(row, &powers[latch]))
            .collect();
        let mut pin_steady = Vec::with_capacity(patterns * pins);
        let mut pin_ramp = Vec::with_capacity(patterns * pins);
        for p in 0..patterns {
            let steady = apply(&sums[latch], driven(p));
            let ramp = apply(&ramps[latch], driven(p));
            for (i, row) in pin_rows.iter().enumerate() {
                pin_steady.push(pin_at(p, i) + row_times(row, &steady));
                pin_ramp.push(slew(latch) * pin_at(p, i) + row_times(row, &ramp));
            }
        }
        let end_steady = (0..patterns)
            .flat_map(|p| apply(&sums[steps], driven(p)))
            .collect();
        let end_ramp = (0..patterns)
            .flat_map(|p| apply(&ramps[steps], driven(p)))
            .collect();
        Self {
            f_gain: f_gain.into_boxed_slice(),
            f_steady: f_steady.into_boxed_slice(),
            f_ramp: f_ramp.into_boxed_slice(),
            pin_gain,
            pin_steady: pin_steady.into_boxed_slice(),
            pin_ramp: pin_ramp.into_boxed_slice(),
            end_gain: powers[steps].clone().into_boxed_slice(),
            end_steady,
            end_ramp,
        }
    }
}

/// A pixel period in fixed-size arithmetic for `M` state variables.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pixel<'a, const M: usize> {
    /// Sub-steps per pixel period.
    steps: usize,
    /// Number of tapped pins; node T follows them.
    pin_count: usize,
    /// Bit number of every tapped pin.
    pin_bits: [u8; MAX_PINS],
    /// Node F at every sub-step per volt of the start state.
    f_gain: &'a [[f64; M]],
    /// Node F for steady patterns.
    f_steady: &'a [f64],
    /// The old pattern's share of node F.
    f_ramp: &'a [f64],
    /// The tapped pins, then node T, per volt of the start state.
    pin_gain: [[f64; M]; MAX_SENSED],
    /// The sensed voltages for steady patterns.
    pin_steady: &'a [f64],
    /// The old pattern's share of the sensed voltages.
    pin_ramp: &'a [f64],
    /// The end state per volt of the start state.
    end_gain: [[f64; M]; M],
    /// The end state for steady patterns.
    end_steady: &'a [f64],
    /// The old pattern's share of the end state.
    end_ramp: &'a [f64],
}

impl<'a, const M: usize> Pixel<'a, M> {
    fn new(period: &'a Period, bits: &[u8], steps: usize) -> Self {
        let pin_count = bits.len();
        Self {
            steps,
            pin_count,
            pin_bits: std::array::from_fn(|i| bits.get(i).copied().unwrap_or(0)),
            f_gain: period.f_gain.as_chunks::<M>().0,
            f_steady: &period.f_steady,
            f_ramp: &period.f_ramp,
            pin_gain: std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    if i <= pin_count {
                        period.pin_gain[i * M + j]
                    } else {
                        0.0
                    }
                })
            }),
            pin_steady: &period.pin_steady,
            pin_ramp: &period.pin_ramp,
            end_gain: std::array::from_fn(|k| std::array::from_fn(|j| period.end_gain[k * M + j])),
            end_steady: &period.end_steady,
            end_ramp: &period.end_ramp,
        }
    }

    /// Returns node F at every sub-step of a pixel driving `to`, slewing from `from` if set.
    #[inline]
    pub(crate) fn f<'k>(
        &'k self,
        to: usize,
        from: Option<usize>,
        z: &'k [f64; M],
    ) -> impl Iterator<Item = f64> + 'k {
        let n = self.steps;
        let row = |table: &'k [f64], pattern: usize| &table[pattern * n..(pattern + 1) * n];
        let steady = row(self.f_steady, to);
        let ramp = from.map(|from| (row(self.f_ramp, from), row(self.f_ramp, to)));
        self.f_gain
            .iter()
            .zip(steady)
            .enumerate()
            .map(move |(k, (gain, &base))| {
                let constant = match ramp {
                    Some((old, new)) => base + (old[k] - new[k]),
                    None => base,
                };
                affine(constant, gain, z)
            })
    }

    /// Returns `(bit, volts)` for every tapped pin of a pixel at the latch, from its start state.
    #[inline]
    pub(crate) fn pins<'k>(
        &'k self,
        to: usize,
        from: Option<usize>,
        z: &'k [f64; M],
    ) -> impl Iterator<Item = (u8, f64)> + 'k {
        (0..self.pin_count).map(move |i| (self.pin_bits[i], self.sensed(i, to, from, z)))
    }

    /// Returns node T of a pixel at the latch, from its start state.
    #[inline]
    pub(crate) fn t_v(&self, to: usize, from: Option<usize>, z: &[f64; M]) -> f64 {
        self.sensed(self.pin_count, to, from, z)
    }

    /// Returns sensed voltage `i` at the latch: a tapped pin, or node T after them.
    #[inline]
    fn sensed(&self, i: usize, to: usize, from: Option<usize>, z: &[f64; M]) -> f64 {
        let n = self.pin_count + 1;
        let mut constant = self.pin_steady[to * n + i];
        if let Some(from) = from {
            constant += self.pin_ramp[from * n + i] - self.pin_ramp[to * n + i];
        }
        affine(constant, &self.pin_gain[i], z)
    }

    /// Returns the state at the end of a pixel, from its start state.
    #[inline]
    pub(crate) fn end(&self, to: usize, from: Option<usize>, z: &[f64; M]) -> [f64; M] {
        std::array::from_fn(|k| {
            let mut constant = self.end_steady[to * M + k];
            if let Some(from) = from {
                constant += self.end_ramp[from * M + k] - self.end_ramp[to * M + k];
            }
            affine(constant, &self.end_gain[k], z)
        })
    }
}

/// The resolved bend circuit: every needed voltage as a function of the sources.
#[derive(Clone, PartialEq)]
pub(crate) struct Network {
    /// Mask of the ADC's code bits.
    code_mask: u16,
    /// Pattern of tapped-line levels for every code; bit `i` is tapped line `i`.
    patterns: Box<[u16]>,
    /// Number of state variables: the coupling capacitor and the nodes with line capacitance.
    states: usize,
    /// The network in every state of the protection diodes, indexed by [`Clamp::index`].
    solutions: [Solution; 3],
    /// Pixel periods by diode state and gate level.
    periods: [[Period; 2]; 3],
    /// Sub-steps per pixel period.
    oversample: usize,
    /// Voltage of node F below which the diodes to ground conduct.
    floor_v: f64,
    /// Voltage of node F above which the diodes to the supply conduct.
    ceiling_v: f64,
    /// Conductance of the conducting diodes on node F.
    clamp_s: f64,
    /// What the rail feeds into the circuit.
    supply: Supply,
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
        let caps = line_capacitance(top, &index, unknowns);
        let charged: Vec<usize> = (0..unknowns).filter(|&k| caps[k] > 0.0).collect();
        let states = 1 + charged.len();
        assert!(
            states <= MAX_STATES,
            "the bend circuit has {states} state variables, more than {MAX_STATES}"
        );
        assert!(
            top.lines.len() <= MAX_PINS,
            "the bend circuit taps {} lines, more than {MAX_PINS}",
            top.lines.len()
        );
        let nodes = Nodes {
            index: &index,
            unknowns,
            caps: &caps,
            charged: &charged,
        };
        let [off, ground, rail] = Clamp::ALL;
        let solutions = [
            solve(top, &nodes, off)?,
            solve(top, &nodes, ground)?,
            solve(top, &nodes, rail)?,
        ];
        let code_mask = ((1u32 << top.bits) - 1) as u16;
        let count = 1 << top.lines.len();
        let periods = solutions
            .each_ref()
            .map(|solution| [0, 1].map(|gate| Period::new(solution, gate, &top.sampling, count)));
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
            states,
            solutions,
            periods,
            oversample: top.sampling.oversample,
            floor_v: top.clamp_source_v(Clamp::Ground),
            ceiling_v: top.clamp_source_v(Clamp::Rail),
            clamp_s: top.clamp_siemens(Clamp::Ground),
            supply: Supply {
                v_oh: top.v_oh,
                line_s: std::array::from_fn(|i| {
                    top.lines.get(i).map_or(0.0, |line| 1.0 / line.driver_ohm)
                }),
                gate_s: if top.drives_t() {
                    1.0 / top.gate_ohm
                } else {
                    0.0
                },
                clamp_s: top.clamp_siemens(Clamp::Rail),
                ceiling_v: top.clamp_source_v(Clamp::Rail),
            },
        })
    }

    /// Returns what the rail feeds into the circuit.
    #[inline]
    pub(crate) fn supply(&self) -> &Supply {
        &self.supply
    }

    /// Returns the pattern of tapped-line levels that `code` drives onto the bus.
    #[inline]
    pub(crate) fn pattern(&self, code: u16) -> usize {
        usize::from(self.patterns[usize::from(code & self.code_mask)])
    }

    /// Returns the number of state variables.
    #[inline]
    pub(crate) fn states(&self) -> usize {
        self.states
    }

    /// Returns the network solved for a state of the protection diodes.
    #[inline]
    pub(crate) fn solution(&self, clamp: Clamp) -> &Solution {
        &self.solutions[clamp.index()]
    }

    /// Returns the kernel of `clamp` among `kernels`.
    #[inline]
    pub(crate) fn kernel<'k, 'a, const M: usize>(
        kernels: &'k [Kernel<'a, M>; 3],
        clamp: Clamp,
    ) -> &'k Kernel<'a, M> {
        &kernels[clamp.index()]
    }

    /// Returns whether `clamp` is the diode state of node F at `f_v` volts.
    pub(crate) fn consistent(&self, clamp: Clamp, f_v: f64) -> bool {
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
    #[cfg(test)]
    fn settle(&self, clamp: &mut Clamp, drive: Drive, z: &[f64], gate: f64) -> f64 {
        let mut f_v = self.solution(*clamp).f.at(0, drive, z, gate);
        for _ in 0..2 {
            if self.consistent(*clamp, f_v) {
                break;
            }
            *clamp = match *clamp {
                Clamp::Off if f_v < self.floor_v => Clamp::Ground,
                Clamp::Off => Clamp::Rail,
                Clamp::Ground | Clamp::Rail => Clamp::Off,
            };
            f_v = self.solution(*clamp).f.at(0, drive, z, gate);
        }
        f_v
    }

    /// Returns the sub-step arithmetic of every diode state for `M` state variables.
    ///
    /// # Panics
    ///
    /// Panics if the network does not have `M` state variables.
    pub(crate) fn kernels<const M: usize>(&self) -> [Kernel<'_, M>; 3] {
        self.solutions.each_ref().map(Kernel::new)
    }

    /// Returns the pixel periods of every diode state and gate level for `M` state variables.
    ///
    /// # Panics
    ///
    /// Panics if the network does not have `M` state variables.
    pub(crate) fn pixels<const M: usize>(&self) -> [[Pixel<'_, M>; 2]; 3] {
        assert_eq!(
            self.states, M,
            "pixels of {M} states for a network of {}",
            self.states
        );
        std::array::from_fn(|c| {
            let bits = &self.solutions[c].latched.bits;
            std::array::from_fn(|g| Pixel::new(&self.periods[c][g], bits, self.oversample))
        })
    }

    /// Solves node F with `kernels` and moves `clamp` to the diode state consistent with it.
    ///
    /// The same as [`settle`](Self::settle), in fixed-size arithmetic.
    #[inline]
    pub(crate) fn settle_in<const M: usize>(
        &self,
        kernels: &[Kernel<'_, M>; 3],
        clamp: &mut Clamp,
        drive: Drive,
        z: &[f64; M],
        gate: usize,
    ) -> f64 {
        let mut f_v = kernels[clamp.index()].f(drive, z, gate);
        for _ in 0..2 {
            if self.consistent(*clamp, f_v) {
                break;
            }
            *clamp = match *clamp {
                Clamp::Off if f_v < self.floor_v => Clamp::Ground,
                Clamp::Off => Clamp::Rail,
                Clamp::Ground | Clamp::Rail => Clamp::Off,
            };
            f_v = kernels[clamp.index()].f(drive, z, gate);
        }
        f_v
    }

    /// Advances the state through one sub-step in the diode state `clamp`.
    #[cfg(test)]
    fn advance(&self, clamp: Clamp, drive: Drive, z: &mut Vector, gate: f64) {
        let next = &self.solution(clamp).next;
        let m = self.states;
        let mut out = [0.0; MAX_STATES];
        for (k, volts) in out[..m].iter_mut().enumerate() {
            *volts = next.at(k, drive, &z[..m], gate);
        }
        z[..m].copy_from_slice(&out[..m]);
    }

    /// Returns the sub-steps after a change of sources or diodes before [`hold`](Self::hold) applies.
    pub(crate) fn settle_steps(&self, clamp: Clamp) -> u64 {
        self.solution(clamp).slow.settle
    }

    /// Returns the state that constant sources hold forever.
    fn fixed_point(&self, clamp: Clamp, pattern: usize, gate: f64) -> Vector {
        let solution = self.solution(clamp);
        let next = &solution.next;
        let m = self.states;
        let mut fixed = [0.0; MAX_STATES];
        if m == 1 {
            fixed[0] = next.source(0, pattern, gate) / (1.0 - next.coefficient(0, 0));
            return fixed;
        }
        let inverse = &solution.slow.fixed;
        for (k, volts) in fixed[..m].iter_mut().enumerate() {
            *volts = (0..m)
                .map(|j| inverse[k * m + j] * next.source(j, pattern, gate))
                .sum();
        }
        fixed
    }

    /// Advances the state through `steps` sub-steps with constant sources.
    ///
    /// The state must have settled onto the slow mode ([`settle_steps`](Self::settle_steps)
    /// sub-steps since the last change). The slow mode's amplitude decays by its
    /// retention `b` per sub-step, `z_n = z* + (z_0 - z*) b^n` with one state variable:
    /// exactly what stepping through them one by one computes.
    pub(crate) fn hold(&self, clamp: Clamp, z: &mut Vector, pattern: usize, gate: f64, steps: u64) {
        if steps == 0 {
            return;
        }
        let slow = &self.solution(clamp).slow;
        let fixed = self.fixed_point(clamp, pattern, gate);
        let m = self.states;
        let mut along = 0.0;
        for k in 0..m {
            along += slow.left[k] * (z[k] - fixed[k]);
        }
        let decay = along * slow.retention.powf(steps as f64);
        for k in 0..m {
            z[k] = fixed[k] + decay * slow.right[k];
        }
    }

    /// Returns how many coming sub-steps with constant sources keep `clamp` consistent.
    ///
    /// Node F is affine in the state, so on the slow mode with constant sources it moves
    /// geometrically from its next value towards its settled value. The state holds for
    /// every sub-step if both lie in it, and otherwise until F crosses the state's
    /// boundary. Returns `u64::MAX` when the state never ends.
    pub(crate) fn quiet_steps(&self, clamp: Clamp, z: &Vector, pattern: usize, gate: f64) -> u64 {
        let solution = self.solution(clamp);
        let retention = solution.slow.retention;
        let m = self.states;
        let fixed = self.fixed_point(clamp, pattern, gate);
        let next = solution.f.at(0, Drive::Steady(pattern), &z[..m], gate);
        let settled = solution.f.at(0, Drive::Steady(pattern), &fixed[..m], gate);
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
            .field("states", &self.states)
            .field("cap_retention", &off.slow.retention)
            .field("settle_steps", &off.slow.settle)
            .field("floor_v", &self.floor_v)
            .field("ceiling_v", &self.ceiling_v)
            .finish_non_exhaustive()
    }
}

/// The merged nodes of a topology and their capacitances.
struct Nodes<'a> {
    /// The unknown of every logical node, `None` for ground.
    index: &'a [Option<usize>],
    /// Number of unknown node voltages.
    unknowns: usize,
    /// Line capacitance of every unknown in farads.
    caps: &'a [f64],
    /// The unknowns with line capacitance, in state order after the coupling capacitor.
    charged: &'a [usize],
}

/// Returns the line capacitance of every unknown: S, T and each tapped pin carry `line_f`.
fn line_capacitance(top: &Topology<'_>, index: &[Option<usize>], unknowns: usize) -> Vec<f64> {
    let mut caps = vec![0.0; unknowns];
    if top.line_f > 0.0 {
        let wires = [S, T]
            .into_iter()
            .chain((0..top.lines.len()).map(|i| PIN0 + i));
        for k in wires.filter_map(|logical| index[logical]) {
            caps[k] += top.line_f;
        }
    }
    caps
}

/// Solves the network for one state of the protection diodes.
fn solve(top: &Topology<'_>, nodes: &Nodes<'_>, clamp: Clamp) -> Result<Solution, Error> {
    let Nodes {
        index,
        unknowns,
        caps,
        charged,
    } = *nodes;
    let node = |logical: usize| index[logical];
    let has_gate = top.drives_t();
    let cap_s = top.cap_f / top.dt_s;
    // Current the constant sources inject into node F, per volt of F's response.
    let leak_s = GATE_INPUTS / top.leak_ohm;
    let bias_a = leak_s * top.v_oh / 2.0 + top.clamp_siemens(clamp) * top.clamp_source_v(clamp);
    let inverse = invert(&conductances(top, nodes, clamp), unknowns)
        .ok_or_else(|| Error::config("bend.bus circuit leaves a node without a path to ground"))?;
    let tapped = top.lines.len();
    let states = 1 + charged.len();

    // The response to unit weights on unknowns: one row of the inverse per weight.
    let respond = |weights: &[(usize, f64)]| {
        let mut row = vec![0.0; unknowns];
        for &(k, weight) in weights {
            for (r, g) in row.iter_mut().zip(&inverse[k * unknowns..]) {
                *r += weight * g;
            }
        }
        let at = |logical: usize| node(logical).map_or(0.0, |k| row[k]);
        Linear {
            drivers: (0..tapped)
                .map(|i| at(PIN0 + i) / top.lines[i].driver_ohm * top.v_oh)
                .collect(),
            bias: at(F) * bias_a,
            state: std::iter::once((at(S) - at(F)) * cap_s)
                .chain(charged.iter().map(|&k| row[k] * (caps[k] / top.dt_s)))
                .collect(),
            gate: if has_gate {
                at(T) / top.gate_ohm * top.v_oh
            } else {
                0.0
            },
        }
    };
    let logical = |weights: &[(usize, f64)]| {
        respond(
            &weights
                .iter()
                .filter_map(|&(logical, weight)| node(logical).map(|k| (k, weight)))
                .collect::<Vec<_>>(),
        )
    };

    // The backward-Euler step of the state, then the exact one on its eigenvectors.
    let step: Vec<Linear> = std::iter::once(logical(&[(S, 1.0), (F, -1.0)]))
        .chain(charged.iter().map(|&k| respond(&[(k, 1.0)])))
        .collect();
    let caps_f: Vec<f64> = std::iter::once(top.cap_f)
        .chain(charged.iter().map(|&k| caps[k]))
        .collect();
    let exact = Exact::new(&step, &caps_f)?;
    let logical = |weights: &[(usize, f64)]| exact.output(&logical(weights));
    let next: Vec<Linear> = (0..states).map(|k| exact.next(k)).collect();
    let next = Responses::new(&next, tapped, states);
    let slow = Slow::new(&exact);
    let sensed: Vec<Linear> = (0..tapped)
        .map(|i| logical(&[(PIN0 + i, 1.0)]))
        .chain(std::iter::once(logical(&[(T, 1.0)])))
        .collect();
    let latched = Latched {
        bits: top.lines.iter().map(|line| line.bit).collect(),
        volts: Responses::new(&sensed, tapped, states),
    };
    Ok(Solution {
        next,
        f: Responses::new(&[logical(&[(F, 1.0)])], tapped, states),
        latched,
        slow,
        #[cfg(feature = "test-util")]
        probes: Probes {
            v_oh: top.v_oh,
            bits: top.lines.iter().map(|line| line.bit).collect(),
            volts: Responses::new(
                &[logical(&[(S, 1.0)]), logical(&[(T, 1.0)])]
                    .into_iter()
                    .chain((0..tapped).map(|i| logical(&[(PIN0 + i, 1.0)])))
                    .collect::<Vec<_>>(),
                tapped,
                states,
            ),
        },
    })
}

/// Stamps the conductance matrix of the merged nodes in one diode state.
///
/// Includes the companion conductances of the coupling capacitor and the line
/// capacitances, and the gate inputs' leakage.
fn conductances(top: &Topology<'_>, nodes: &Nodes<'_>, clamp: Clamp) -> Vec<f64> {
    let (index, unknowns) = (nodes.index, nodes.unknowns);
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
    if top.drives_t() {
        stamp(T, GND, 1.0 / top.gate_ohm);
    }
    stamp(
        F,
        GND,
        GATE_INPUTS / top.leak_ohm + top.clamp_siemens(clamp),
    );
    for &k in nodes.charged {
        matrix[k * unknowns + k] += nodes.caps[k] / top.dt_s;
    }
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
fn merge(top: &Topology<'_>) -> Sets {
    let mut nodes = Sets::new(PIN0 + top.lines.len());
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

/// Disjoint sets of logical nodes; ground is always the root of its set.
struct Sets {
    parent: Vec<usize>,
}

impl Sets {
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

    fn topology(pot_node: PotNode, common_bus: bool, line_f: f64) -> Topology<'static> {
        Topology {
            lines: &LINES,
            sources: (1 << 9) | (1 << 8),
            target: Target::Pin(10),
            function: Function::Invert,
            common_bus,
            series_ohm: 47.0,
            cap_f: 1e-6,
            line_f,
            pot_ohm: 500.0,
            pot_node,
            gate_ohm: 90.0,
            gate_driven: true,
            v_oh: 3.3,
            clamp_v: 0.5,
            clamp_ohm: 50.0,
            leak_ohm: 3.3e6,
            dt_s: 1e-8,
            bits: 12,
            sampling: Sampling {
                oversample: 4,
                ramp: &[0.7],
                latch: 2,
            },
        }
    }

    /// Steps the state `steps` times with constant sources.
    fn step(
        net: &Network,
        clamp: Clamp,
        z: &Vector,
        pattern: usize,
        gate: f64,
        steps: u64,
    ) -> Vector {
        let mut stepped = *z;
        for _ in 0..steps {
            net.advance(clamp, Drive::Steady(pattern), &mut stepped, gate);
        }
        stepped
    }

    #[test]
    fn holding_the_sources_equals_stepping_through_them() {
        // The common bus lets the gate output reach the capacitor, so every term counts.
        let net = Network::build(&topology(PotNode::F, true, 0.0)).unwrap();
        assert_eq!(net.states(), 1);
        for clamp in Clamp::ALL {
            let next = &net.solution(clamp).next;
            assert!(next.gate[0].abs() > 1e-6);
            for (pattern, gate) in [(0b011, 1.0), (0b100, 0.0), (0b000, 1.0)] {
                let mut start = [0.0; MAX_STATES];
                start[0] = 0.7;
                let stepped = step(&net, clamp, &start, pattern, gate, 20_000);
                let mut held = start;
                net.hold(clamp, &mut held, pattern, gate, 20_000);
                assert!(
                    (held[0] - stepped[0]).abs() < 1e-9,
                    "{clamp:?}: held {} V, stepped {} V",
                    held[0],
                    stepped[0]
                );
            }
        }
    }

    #[test]
    fn with_line_capacitance_holding_matches_stepping_once_the_lines_settle() {
        // 100 pF on every wire behind 47 ohm patch resistors: every pin, S and T carry
        // their own state, and the fast modes take a few sub-steps to settle.
        let net = Network::build(&topology(PotNode::F, false, 100e-12)).unwrap();
        assert_eq!(net.states(), 1 + 2 + LINES.len());
        for clamp in Clamp::ALL {
            let settle = net.settle_steps(clamp);
            assert!(
                (2..1000).contains(&settle),
                "{clamp:?}: the lines settle in {settle} sub-steps"
            );
            for (pattern, gate) in [(0b011, 1.0), (0b100, 0.0)] {
                let mut start = [0.0; MAX_STATES];
                start[0] = 0.7;
                let settled = step(&net, clamp, &start, pattern, gate, settle);
                let stepped = step(&net, clamp, &settled, pattern, gate, 20_000);
                let mut held = settled;
                net.hold(clamp, &mut held, pattern, gate, 20_000);
                for k in 0..net.states() {
                    assert!(
                        (held[k] - stepped[k]).abs() < 1e-9,
                        "{clamp:?}, state {k}: held {} V, stepped {} V",
                        held[k],
                        stepped[k]
                    );
                }
            }
        }
    }

    #[test]
    fn a_clamp_holds_exactly_as_long_as_stepping_keeps_it_consistent() {
        // With the pot on S and every source low, a charged capacitor pulls F far below
        // ground; the diodes to ground conduct until the charge has drained.
        for line_f in [0.0, 100e-12] {
            let net = Network::build(&topology(PotNode::S, false, line_f)).unwrap();
            let mut clamp = Clamp::Off;
            let mut z = [0.0; MAX_STATES];
            z[0] = 2.5;
            net.settle(&mut clamp, Drive::Steady(0), &z[..net.states()], 1.0);
            assert_eq!(clamp, Clamp::Ground);
            // Let the line nodes settle onto the slow mode first.
            z = step(&net, clamp, &z, 0, 1.0, net.settle_steps(clamp));
            let quiet = net.quiet_steps(clamp, &z, 0, 1.0);
            let f = &net.solution(clamp).f;
            let mut stepped = 0;
            while net.consistent(clamp, f.at(0, Drive::Steady(0), &z[..net.states()], 1.0)) {
                net.advance(clamp, Drive::Steady(0), &mut z, 1.0);
                stepped += 1;
            }
            assert!(stepped > 10, "the clamp held only {stepped} sub-steps");
            assert!(
                quiet.abs_diff(stepped) <= 1,
                "{line_f} F: {quiet} quiet steps, {stepped} stepped"
            );
        }
    }
}
