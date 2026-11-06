//! The master clock bend: the timing generator and AFE reclocked from a replacement oscillator.
//!
//! The panel sets the preset's `[bend.clock]` section, the LTC1799 mod: the crystal of
//! the timing generator and the AFE is replaced by an oscillator in megahertz, while the
//! processor keeps its own crystal at the camera's pixel clock. The oscillator's knob is
//! dimmed while it runs at that nominal frequency, where the bend changes nothing, and
//! its reset returns it there; next to it the panel shows the ratio of the two clocks
//! in percent and the sensor's line time in microseconds. The oscillator wanders by a
//! peak drift in parts per million at a rate in hertz, follows the supply rail by a
//! sensitivity in parts per million per volt, and the processor re-synchronises each
//! line within a capture window in pixels, 0 being free-running. [`ReclockPanel`]
//! keeps every value while the crystal is in place.
//!
//! The panel is titled "Master clock" to keep it apart from the Clocks panel of the
//! CCD's transfer clocks (`[bend.clocks]`).

use eframe::egui::Ui;
use vcb_core::preset::{Mode, ReclockBend};
use vcb_core::{Camera, Preset, Timing};

use crate::panels::{self, Edit, Knob, PanelId};

/// The oscillator knob: 9 to 54 MHz, a third to twice the A520's 27 MHz, in kilohertz steps.
pub const OSCILLATOR: Knob = Knob::linear(9.0..=54.0, " MHz", 3);

/// The drift knob: up to 1 % of slow wander, logarithmic, with zero for a steady oscillator.
pub const DRIFT: Knob = Knob::log(0.0..=10_000.0, 1.0, " ppm");

/// The drift-rate knob: 0.01 to 10 Hz, logarithmic.
pub const DRIFT_RATE: Knob = Knob::log(0.01..=10.0, 0.01, " Hz");

/// The supply-sensitivity knob: up to half a percent per volt either way.
pub const SUPPLY: Knob = Knob::linear(-5000.0..=5000.0, " ppm/V", 0);

/// The HD-lock knob: a capture window up to 1000 pixels, logarithmic, 0 free-running.
pub const HD_LOCK: Knob = Knob::log(0.0..=1000.0, 1.0, " px").zero_reads("free-running");

/// The oscillator of a panel never set: the A520's 27 MHz pixel clock, where it changes nothing.
pub const DEFAULT_OSC_MHZ: f64 = 27.0;

/// The master clock bend's switch and values.
#[derive(Debug, Clone, PartialEq)]
pub struct ReclockPanel {
    /// Whether the crystal is replaced: the bend is patched in.
    pub enabled: bool,
    /// The values, kept while the crystal is in place.
    pub bend: ReclockBend,
}

impl Default for ReclockPanel {
    /// A steady oscillator at the A520's pixel clock, the crystal in place.
    fn default() -> Self {
        Self {
            enabled: false,
            bend: ReclockBend::new(DEFAULT_OSC_MHZ),
        }
    }
}

impl ReclockPanel {
    /// The panel showing `preset`'s master clock bend, or the default values, off.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the panel from `preset`; without a master clock bend only the switch moves.
    pub fn load(&mut self, preset: &Preset) {
        match preset.bend.clock {
            Some(bend) => {
                self.enabled = true;
                self.bend = bend;
            }
            None => self.enabled = false,
        }
    }

    /// The preset's `[bend.clock]` section; `None` while the crystal is in place.
    #[must_use]
    pub fn bend(&self) -> Option<ReclockBend> {
        self.enabled.then_some(self.bend)
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.clock = self.bend();
    }

    /// Draws the panel for `camera` in readout `mode`; `timing` is the current readout, if known.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        camera: &Camera,
        mode: Mode,
        timing: Option<&Timing>,
    ) -> Edit {
        let mut edit = Edit::default();
        edit.record(&panels::bend_header(
            ui,
            PanelId::Clock.title(),
            &mut self.enabled,
            "Master clock",
            ["crystal", "replaced"],
        ));
        let nominal_hz = camera.clocking(mode).pixel_clock_hz;
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            let bend = &mut self.bend;
            panels::grid(ui, "reclock", |ui| {
                panels::default_row(
                    ui,
                    &mut edit,
                    "Oscillator",
                    "Frequency of the oscillator that replaces the crystal of the timing generator and the AFE; the processor keeps its own, the camera's pixel clock shown dimmed",
                    &mut bend.osc_mhz,
                    nominal_hz / 1e6,
                    &OSCILLATOR,
                );
                panels::hint_row(ui, &ratio_readout(bend.osc_mhz, nominal_hz, timing));
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Drift",
                    "Peak slow wander of the oscillator, as an RC oscillator's warming in the hand; 0 is steady",
                    &mut bend.drift_ppm,
                    &DRIFT,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Drift rate",
                    "How fast the drift wanders; a steady oscillator draws the same tears in every frame, drift moves them",
                    &mut bend.drift_hz,
                    &DRIFT_RATE,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Supply sensitivity",
                    &format!(
                        "Change of frequency per volt the supply leaves the camera's {:.1} V; it acts through the supply rail, so only while the Power panel's rail bend is on (LTC1799 class: 1000 ppm/V)",
                        camera.power.rail_v
                    ),
                    &mut bend.ppm_per_v,
                    &SUPPLY,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "HD lock",
                    "The processor's line-start capture window in pixels of the camera's clock: a sensor line start within it restarts the processor's line there; 0 is free-running",
                    &mut bend.hd_lock_px,
                    &HD_LOCK,
                );
            });
            let ratio = bend.osc_mhz * 1e6 / nominal_hz;
            panels::hint(ui, &slip_readout(slip_px(ratio, camera, mode)));
        });
        edit
    }
}

/// The oscillator against the crystal in percent, and the sensor's line time if `timing` is known.
///
/// `nominal_hz` is the camera's pixel clock in the readout mode. The line time is the
/// processor's line in `timing` divided by the ratio, so it shows the oscillator set on
/// the knob even while the bend is off.
#[must_use]
pub fn ratio_readout(osc_mhz: f64, nominal_hz: f64, timing: Option<&Timing>) -> String {
    let ratio = osc_mhz * 1e6 / nominal_hz;
    let text = format!(
        "{:.3} % of {} MHz",
        ratio * 100.0,
        panels::significant(nominal_hz / 1e6, 3)
    );
    match timing {
        Some(timing) => {
            let line_s = timing.dsp_pixel_period_s * f64::from(timing.line_len()) / ratio;
            format!("{text}, sensor line {:.2} µs", line_s * 1e6)
        }
        None => text,
    }
}

/// Pixels of the camera's clock the sensor's line start slips per line at clock `ratio`.
///
/// `(ratio - 1)` times the camera's native line in `mode`, optical black, active
/// samples and blanking: positive when the sensor runs fast.
#[must_use]
pub fn slip_px(ratio: f64, camera: &Camera, mode: Mode) -> f64 {
    let line = camera.sensor.ob_cols + camera.sensor.native_width + camera.clocking(mode).hblank_px;
    (ratio - 1.0) * f64::from(line)
}

/// What the slip per line means for the HD lock window.
fn slip_readout(slip_px: f64) -> String {
    if slip_px.abs() < 0.05 {
        "The sensor's lines keep step with the processor's.".to_owned()
    } else {
        format!(
            "Each sensor line slips {:.1} px against the processor's; an HD lock wider than that holds every line.",
            slip_px.abs()
        )
    }
}
