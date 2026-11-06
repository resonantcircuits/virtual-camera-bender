//! The 74HC00 inverter and the 74HC74 flip-flop of the bend, on the sub-step grid.
//!
//! Both chips read node F through a CMOS input: a switching threshold near half the
//! supply with a small hysteresis band. Their outputs drive node T through the 74HC
//! output resistance, which the network in [`circuit`](super::circuit) models; this
//! module holds only the logic levels and their timing.
//!
//! Time advances in circuit sub-steps. Everything that moves node F (the line drivers,
//! the gate outputs) switches at a sub-step boundary, so an input crossing registered
//! in a sub-step happened at its start. A transition due `delay` later therefore acts
//! from the sub-step that ends at or after that instant: `ceil(delay / dt) - 1`
//! sub-steps after the crossing, and at least one, since a gate cannot answer within
//! the sub-step that moved its input.

use crate::camera::Logic;

/// Tolerance when converting a duration to whole sub-steps, in sub-steps.
///
/// Absorbs floating-point rounding, so a delay of exactly two sub-steps stays two
/// instead of rounding up to three.
const STEP_TOLERANCE: f64 = 1e-6;

/// Input thresholds and delays of the logic chips, in volts and sub-steps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Chips {
    /// Input voltage above which a low input turns high.
    pub rise_v: f64,
    /// Input voltage below which a high input turns low.
    pub fall_v: f64,
    /// Sub-steps from an input crossing of the inverter to its output turning.
    pub inv_delay: u32,
    /// Sub-steps from a seen clock edge of the flip-flop to `Q` toggling.
    pub ff_delay: u32,
    /// Shortest low time before a rising clock edge that the flip-flop sees.
    pub ff_min_pulse: u64,
    /// Shortest interval after an accepted clock edge before the next one is seen.
    pub ff_period: u64,
    /// Time `D` must be stable before a clock edge to be captured.
    pub ff_setup: u64,
}

impl Chips {
    /// Converts the camera's logic section to sub-steps of `dt_s` seconds.
    ///
    /// `scale` is `Timing::resolution_scale`, the processor's capture period over the
    /// native pixel period. Every duration of the logic section is multiplied by it, so
    /// a delay spans the same number of processor periods at match-input resolution as
    /// at the camera's own pixel clock; a bent sensor clock changes how many sensor
    /// periods it spans.
    pub(crate) fn new(logic: &Logic, scale: f64, dt_s: f64) -> Self {
        let steps = |seconds: f64| (seconds * scale / dt_s - STEP_TOLERANCE).ceil().max(0.0);
        let delay = |ns: f64| (steps(ns * 1e-9) - 1.0).max(1.0) as u32;
        let half = logic.hc_hysteresis_v / 2.0;
        Self {
            rise_v: logic.hc_threshold_v + half,
            fall_v: logic.hc_threshold_v - half,
            inv_delay: delay(logic.hc_prop_delay_ns),
            ff_delay: delay(logic.ff_prop_delay_ns),
            ff_min_pulse: steps(logic.ff_min_pulse_ns * 1e-9) as u64,
            ff_period: steps(1.0 / logic.ff_fmax_hz) as u64,
            ff_setup: steps(logic.ff_setup_ns * 1e-9) as u64,
        }
    }

    /// Returns whether an input at `level` turns to the other level at `f_v` volts.
    fn crosses(&self, level: bool, f_v: f64) -> bool {
        if level {
            f_v < self.fall_v
        } else {
            f_v > self.rise_v
        }
    }
}

/// A logic chip that reads node F and may drive node T.
pub(crate) trait Gate {
    /// Advances one sub-step: ages the timers and completes a transition that falls due.
    fn tick(&mut self);

    /// Returns whether the output is high.
    fn high(&self) -> bool;

    /// Presents the node-F voltage solved for the current sub-step.
    fn sense(&mut self, f_v: f64, chips: &Chips);

    /// Returns the sub-steps until the pending output transition, 0 if none is pending.
    fn pending(&self) -> u32;

    /// Advances `steps` sub-steps during which the input keeps its level.
    fn idle(&mut self, steps: u64);
}

/// The bypass wire: no gate drives node T.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Wire;

impl Gate for Wire {
    fn tick(&mut self) {}

    fn high(&self) -> bool {
        false
    }

    fn sense(&mut self, _f_v: f64, _chips: &Chips) {}

    fn pending(&self) -> u32 {
        0
    }

    fn idle(&mut self, _steps: u64) {}
}

/// A 74HC00 NAND gate with tied inputs: an inverter with inertial delay.
///
/// The output turns to the complement of the input `inv_delay` sub-steps after the
/// input crosses; an input pulse shorter than that is swallowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Inverter {
    /// Logic level of the input.
    input: bool,
    /// Logic level of the output.
    output: bool,
    /// Sub-steps until the output turns, 0 if it is settled.
    pending: u32,
}

impl Default for Inverter {
    /// The power-on state: node F at ground, so the input is low and the output high.
    fn default() -> Self {
        Self {
            input: false,
            output: true,
            pending: 0,
        }
    }
}

impl Gate for Inverter {
    fn tick(&mut self) {
        if self.pending > 0 {
            self.pending -= 1;
            if self.pending == 0 {
                self.output = !self.output;
            }
        }
    }

    fn high(&self) -> bool {
        self.output
    }

    fn sense(&mut self, f_v: f64, chips: &Chips) {
        if chips.crosses(self.input, f_v) {
            self.input = !self.input;
            // The output must turn only if it now equals the input; otherwise the input
            // returned before the output moved and the pending transition is cancelled.
            self.pending = if self.input == self.output {
                chips.inv_delay
            } else {
                0
            };
        }
    }

    fn pending(&self) -> u32 {
        self.pending
    }

    fn idle(&mut self, steps: u64) {
        if self.pending == 0 {
            return;
        }
        if u64::from(self.pending) <= steps {
            self.output = !self.output;
            self.pending = 0;
        } else {
            self.pending -= steps as u32;
        }
    }
}

/// A 74HC74 D flip-flop wired as a toggle: `D` from `/Q`, preset and clear tied high.
///
/// A rising clock edge is seen only if the clock was low for at least `ff_min_pulse`
/// and the last seen edge is at least `ff_period` (1 / fmax) ago. A seen edge captures
/// `D` as it was `ff_setup` before the edge, and `Q` takes the captured value
/// `ff_delay` sub-steps later. Because `D` is `/Q`, `Q` toggles on every seen edge
/// unless `Q` itself changed within the setup time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlipFlop {
    /// Logic level of the clock input.
    clock: bool,
    /// Logic level of `Q`.
    q: bool,
    /// Sub-steps until `Q` toggles, 0 if it is settled.
    pending: u32,
    /// Sub-steps since the clock input last crossed.
    since_crossing: u64,
    /// Sub-steps since the last seen rising edge.
    since_edge: u64,
    /// Sub-steps since `Q` last changed.
    since_q: u64,
}

impl Default for FlipFlop {
    /// The power-on state: clock low, `Q` low, no edge or change of `Q` in memory.
    fn default() -> Self {
        Self {
            clock: false,
            q: false,
            pending: 0,
            since_crossing: u64::MAX,
            since_edge: u64::MAX,
            since_q: u64::MAX,
        }
    }
}

impl Gate for FlipFlop {
    fn tick(&mut self) {
        self.since_crossing = self.since_crossing.saturating_add(1);
        self.since_edge = self.since_edge.saturating_add(1);
        self.since_q = self.since_q.saturating_add(1);
        if self.pending > 0 {
            self.pending -= 1;
            if self.pending == 0 {
                self.q = !self.q;
                self.since_q = 0;
            }
        }
    }

    fn high(&self) -> bool {
        self.q
    }

    fn sense(&mut self, f_v: f64, chips: &Chips) {
        if !chips.crosses(self.clock, f_v) {
            return;
        }
        self.clock = !self.clock;
        let low_time = self.since_crossing;
        self.since_crossing = 0;
        let seen =
            self.clock && low_time >= chips.ff_min_pulse && self.since_edge >= chips.ff_period;
        if !seen {
            return;
        }
        self.since_edge = 0;
        // `D` is `/Q`. Within the setup time after a change of `Q` the latch still sees
        // `D` from before the change, which equals the present `Q`.
        let d = if self.since_q >= chips.ff_setup {
            !self.q
        } else {
            self.q
        };
        if self.pending == 0 && d != self.q {
            self.pending = chips.ff_delay;
        }
    }

    fn pending(&self) -> u32 {
        self.pending
    }

    fn idle(&mut self, steps: u64) {
        self.since_crossing = self.since_crossing.saturating_add(steps);
        self.since_edge = self.since_edge.saturating_add(steps);
        self.since_q = self.since_q.saturating_add(steps);
        if self.pending == 0 {
            return;
        }
        if u64::from(self.pending) <= steps {
            self.q = !self.q;
            self.since_q = steps - u64::from(self.pending);
            self.pending = 0;
        } else {
            self.pending -= steps as u32;
        }
    }
}
