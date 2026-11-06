//! The supply-rail bend: extra loads and disturbances on the camera's 3.3 V rail.
//!
//! The panel sets the preset's `[bend.rail]` section. The rail is a source behind the
//! regulator's and battery's impedance, decoupled by a capacitor, and it feeds the
//! AFE, the bus drivers with the bend's logic, and the processor's input. The panel
//! loads it with a steady current in milliamperes, seeded spikes of a current, a mean
//! rate in hertz and a length in milliseconds, and ripple of a peak voltage in
//! millivolts at a frequency in hertz; the source impedance in ohms starts dimmed at
//! the camera's and may be raised as a dying battery's. Below, the camera's rail and
//! the three thresholds where its parts fail, and the rail the set load reaches.
//! [`RailPanel`] keeps every value while the rail is regulated.

use eframe::egui::Ui;
use vcb_core::camera::Power;
use vcb_core::preset::RailBend;
use vcb_core::{Camera, Preset};

use crate::panels::{self, Edit, Knob, PanelId};

/// The load knobs: up to 5 A, logarithmic, with zero for none.
pub const CURRENT: Knob = Knob::log(0.0..=5000.0, 1.0, " mA");

/// The spike-rate knob: up to 100 a second, logarithmic, with zero for none.
pub const SPIKE_RATE: Knob = Knob::log(0.0..=100.0, 0.01, " Hz");

/// The spike-length knob: 10 µs to 100 ms, logarithmic.
pub const SPIKE_LENGTH: Knob = Knob::log(0.01..=100.0, 0.01, " ms");

/// The ripple knob: up to 1 V peak, logarithmic, with zero for none.
pub const RIPPLE: Knob = Knob::log(0.0..=1000.0, 0.1, " mV");

/// The ripple-frequency knob: 1 Hz to 100 kHz, logarithmic.
pub const RIPPLE_FREQUENCY: Knob = Knob::log(1.0..=100_000.0, 1.0, " Hz");

/// The source-impedance knob: 10 mΩ to 100 Ω, logarithmic.
pub const SOURCE: Knob = Knob::log(0.01..=100.0, 0.01, " Ω");

/// The supply-rail bend's switch and values.
#[derive(Debug, Clone, PartialEq)]
pub struct RailPanel {
    /// Whether the rail is loaded: the bend is patched in.
    pub enabled: bool,
    /// The values, kept while the rail is regulated.
    pub bend: RailBend,
}

impl Default for RailPanel {
    /// A steady 150 mA added load, as LEDs and an added oscillator draw, regulated.
    fn default() -> Self {
        Self {
            enabled: false,
            bend: RailBend {
                load_ma: 150.0,
                ..RailBend::default()
            },
        }
    }
}

impl RailPanel {
    /// The panel showing `preset`'s rail bend, or the default values, regulated.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the panel from `preset`; without a rail bend only the switch moves.
    pub fn load(&mut self, preset: &Preset) {
        match preset.bend.rail {
            Some(bend) => {
                self.enabled = true;
                self.bend = bend;
            }
            None => self.enabled = false,
        }
    }

    /// The preset's `[bend.rail]` section; `None` while the rail is regulated.
    #[must_use]
    pub fn bend(&self) -> Option<RailBend> {
        self.enabled.then_some(self.bend)
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.rail = self.bend();
    }

    /// Draws the panel for `camera`, whose `[power]` section describes the rail.
    pub fn show(&mut self, ui: &mut Ui, camera: &Camera) -> Edit {
        let mut edit = Edit::default();
        edit.record(&panels::bend_header(
            ui,
            PanelId::Power.title(),
            &mut self.enabled,
            "Supply rail",
            ["regulated", "loaded"],
        ));
        let power = &camera.power;
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            let bend = &mut self.bend;
            panels::section(ui, "Loads");
            panels::grid(ui, "rail-load", |ui| {
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Steady load",
                    "Current drawn from the rail beyond the camera's own: LEDs, an added oscillator, a pot to ground",
                    &mut bend.load_ma,
                    &CURRENT,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Spike current",
                    "Current of each load spike: a flash charger or a lens motor starting",
                    &mut bend.spike_ma,
                    &CURRENT,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Spike rate",
                    "Mean number of spikes a second, arriving at random; one arriving during a spike extends it",
                    &mut bend.spike_hz,
                    &SPIKE_RATE,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Spike length",
                    "Duration of one spike",
                    &mut bend.spike_ms,
                    &SPIKE_LENGTH,
                );
            });
            panels::section(ui, "Source");
            panels::grid(ui, "rail-source", |ui| {
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Ripple",
                    "Peak ripple of the converter or a weak battery; the rail sees it through the source impedance and the decoupling",
                    &mut bend.ripple_mv,
                    &RIPPLE,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Ripple frequency",
                    "Frequency of the ripple; the rail is sampled once a line, so ripple faster than about half the line rate aliases",
                    &mut bend.ripple_hz,
                    &RIPPLE_FREQUENCY,
                );
                panels::override_row(
                    ui,
                    &mut edit,
                    "Source impedance",
                    "Output impedance of the regulator and battery; a dying battery raises it, and every milliampere then sags the rail further",
                    &mut bend.source_ohm,
                    power.source_ohm,
                    &SOURCE,
                );
            });
            panels::hint(ui, &thresholds(power));
            panels::hint(ui, &reach(power, bend));
        });
        edit
    }
}

/// The rail a load holds, and its lowest with spikes and ripple, in volts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sag {
    /// The rail under the steady load alone.
    pub steady_v: f64,
    /// The rail at the bottom of a spike and of the ripple together.
    pub lowest_v: f64,
}

/// The rail `bend` holds on the camera's `power` supply, before the bus bend's own current.
///
/// The camera's rail sits at `rail_v` under its quiescent load, so only the added
/// current sags it, by the source impedance `R` times the current. A spike of length
/// `t` charges the decoupling `C` down by `1 - e^(-t / RC)` of its full sag, and the
/// ripple reaches the rail through the same RC low-pass, so the lowest rail is the
/// steady one less both. The loads starve at 0 V.
#[must_use]
pub fn sag(power: &Power, bend: &RailBend) -> Sag {
    let ohm = bend.source_ohm.unwrap_or(power.source_ohm);
    let tau_s = ohm * power.decoupling_uf * 1e-6;
    let steady_v = power.rail_v - ohm * bend.load_ma * 1e-3;
    let spike_v = if bend.spike_hz > 0.0 {
        ohm * bend.spike_ma * 1e-3 * (1.0 - (-bend.spike_ms * 1e-3 / tau_s).exp())
    } else {
        0.0
    };
    let omega_tau = 2.0 * std::f64::consts::PI * bend.ripple_hz * tau_s;
    let ripple_v = bend.ripple_mv * 1e-3 / omega_tau.hypot(1.0);
    Sag {
        steady_v: steady_v.max(0.0),
        lowest_v: (steady_v - spike_v - ripple_v).max(0.0),
    }
}

/// The camera's rail and its three thresholds in volts, highest first.
#[must_use]
pub fn thresholds(power: &Power) -> String {
    let listed: Vec<String> = limits(power)
        .iter()
        .map(|(v, name)| format!("{name} {v:.2} V"))
        .collect();
    format!("Rail {:.2} V; {}.", power.rail_v, listed.join(", "))
}

/// What the set load does to the rail, and the thresholds its lowest falls below.
#[must_use]
pub fn reach(power: &Power, bend: &RailBend) -> String {
    let Sag { steady_v, lowest_v } = sag(power, bend);
    let mut text = format!("The load holds the rail at {steady_v:.2} V");
    if steady_v - lowest_v >= 0.005 {
        text = format!("{text}, {lowest_v:.2} V at the lowest");
    }
    let crossed: Vec<String> = limits(power)
        .into_iter()
        .filter(|(v, _)| lowest_v < *v)
        .map(|(_, name)| format!("the {name}"))
        .collect();
    if let Some((last, rest)) = crossed.split_last() {
        let below = if rest.is_empty() {
            last.clone()
        } else {
            format!("{} and {last}", rest.join(", "))
        };
        text = format!("{text}, below {below}");
    }
    format!("{text}; the bus bend's own current comes on top.")
}

/// The rail's thresholds with their names, highest first.
fn limits(power: &Power) -> Vec<(f64, &'static str)> {
    let mut limits = vec![
        (power.afe_uvlo_v, "AFE lockout"),
        (power.logic_min_v, "logic minimum"),
        (power.dsp_brownout_v, "processor brownout"),
    ];
    limits.sort_by(|a, b| b.0.total_cmp(&a.0));
    limits
}
