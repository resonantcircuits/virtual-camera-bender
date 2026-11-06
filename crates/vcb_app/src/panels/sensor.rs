//! The sensor panel: the exposure time and whether the readout smears.
//!
//! The exposure time is the preset's `[sensor] exposure_s`. It starts dimmed at the
//! camera's own value; moving the knob writes the override and its reset button
//! removes it again (and the section with it). The collected light scales with the
//! exposure over the camera's nominal exposure, so a longer time brightens the frame
//! until the well or the converter clips, unless auto-exposure compensates within its
//! two stops; dark charge and hot photosites grow with it too. A new exposure reaches
//! the running camera without a power cycle.
//!
//! The knob sets the still exposure. In movie mode the panel shows, read-only, what the
//! electronic shutter integrates: the preset's or the camera's movie exposure, at most
//! one frame period. The smear flag belongs to the readout mode's shutter and is shown
//! read-only too: the mechanical shutter of the still readout closes during the
//! transfer, the electronic shutter of the movie readout leaves the array lit.
//!
//! Below, the "Sensor bend" block sets the preset's `[bend.sensor]` section with its
//! own power switch: the sensor's temperature in degrees Celsius and its substrate
//! bias in volts, each dimmed at the camera's value (`reference_temp_c`, the nominal
//! `substrate_v`) until moved, with a reset back to it. A readout shows the dark
//! current at the set temperature as a multiple of the camera's, `2^((T - T_ref) /
//! dark_doubling_c)`, and the full well at the set bias, with the overflow drain's
//! remaining capacity below nominal. [`SensorBendPanel`] keeps both values while the
//! sensor is stock.

use eframe::egui::Ui;
use vcb_core::preset::{Mode, SensorBend, SensorOverride};
use vcb_core::{Camera, Preset, Timing};

use crate::panels::{self, Edit, Knob, Scale};

/// The exposure knob: 0.1 ms to 1 s, logarithmic, stored in seconds.
pub const EXPOSURE: Knob = Knob::log(0.1..=1000.0, 0.1, " ms").scaled(Scale::Times(1e3));

/// The temperature knob: freeze spray at -20 °C to a heat gun at 120 °C, in whole degrees.
pub const TEMPERATURE: Knob = Knob::linear(-20.0..=120.0, " °C", 0);

/// How far the substrate-bias knob turns either way from the camera's nominal bias, in volts.
pub const BIAS_SPAN_V: f64 = 4.0;

/// The temperature of a fresh sensor bend: a heat gun on the sensor, in degrees Celsius.
pub const DEFAULT_TEMPERATURE_C: f64 = 70.0;

/// The substrate-bias knob for `camera`: nominal ± [`BIAS_SPAN_V`], to a hundredth of a volt.
#[must_use]
pub fn bias_knob(camera: &Camera) -> Knob {
    let nominal = camera.sensor.substrate_v;
    Knob::linear(
        (nominal - BIAS_SPAN_V).max(0.0)..=nominal + BIAS_SPAN_V,
        " V",
        2,
    )
}

/// The preset's exposure override, if it sets one.
#[must_use]
pub fn exposure_override(preset: &Preset) -> Option<f64> {
    preset.sensor.and_then(|sensor| sensor.exposure_s)
}

/// Sets the preset's exposure override, or removes it with `None`.
///
/// Removing the last value of the `[sensor]` section removes the section.
pub fn set_exposure(sensor: &mut Option<SensorOverride>, exposure_s: Option<f64>) {
    if exposure_s.is_some() {
        sensor
            .get_or_insert_with(SensorOverride::default)
            .exposure_s = exposure_s;
    } else if let Some(section) = sensor.as_mut() {
        section.exposure_s = None;
        if *section == SensorOverride::default() {
            *sensor = None;
        }
    }
}

/// The time the sensor integrates in `mode`, as the simulator resolves it, in seconds.
///
/// Still: the preset's exposure, else the camera's still exposure. Movie: the preset's
/// exposure, else the camera's movie exposure, at most one frame period.
#[must_use]
pub fn integration_s(camera: &Camera, preset: &Preset, mode: Mode) -> f64 {
    let set = exposure_override(preset);
    match mode {
        Mode::Still => set.unwrap_or(camera.still.exposure_s),
        Mode::Movie => set
            .unwrap_or(camera.movie.exposure_s)
            .min(1.0 / camera.movie.fps),
    }
}

/// Draws the sensor controls for `preset` on `camera`.
pub fn show(ui: &mut Ui, preset: &mut Preset, camera: &Camera) -> Edit {
    let mut edit = Edit::default();
    panels::header(ui, "Sensor", None);
    let mode = preset.readout.mode;
    panels::grid(ui, "sensor", |ui| {
        match mode {
            Mode::Still => {
                let mut exposure = exposure_override(preset);
                let before = exposure;
                panels::override_row(
                    ui,
                    &mut edit,
                    "Exposure",
                    "Integration time: the light collected, the dark charge and the hot photosites grow with it",
                    &mut exposure,
                    camera.still.exposure_s,
                    &EXPOSURE,
                );
                if exposure != before {
                    set_exposure(&mut preset.sensor, exposure);
                }
            }
            Mode::Movie => {
                let frame_s = 1.0 / camera.movie.fps;
                let source = if exposure_override(preset).is_some() {
                    "from the preset"
                } else {
                    "the camera's"
                };
                panels::value_row(
                    ui,
                    "Exposure",
                    &format!(
                        "{:.2} ms, {source}",
                        integration_s(camera, preset, Mode::Movie) * 1e3
                    ),
                    &format!(
                        "The electronic shutter integrates at most one frame, {:.1} ms at {} fps; set the exposure in still mode",
                        frame_s * 1e3,
                        camera.movie.fps
                    ),
                );
            }
        }
        let (smear, shutter) = match mode {
            Mode::Still => (camera.still.smear, "mechanical shutter"),
            Mode::Movie => (camera.movie.smear, "electronic shutter"),
        };
        panels::value_row(
            ui,
            "Smear",
            &format!("{}, {shutter}", if smear { "on" } else { "off" }),
            "Whether light reaches the vertical register during the transfer; set by the readout mode",
        );
    });
    let nominal = camera.sensor.nominal_exposure_s;
    let exposure = integration_s(camera, preset, mode);
    panels::hint(
        ui,
        &format!(
            "{:+.2} stops of light against the nominal {:.2} ms; auto-exposure may take back up to two.",
            (exposure / nominal).log2(),
            nominal * 1e3
        ),
    );
    edit
}

/// The sensor bend's switch and values.
#[derive(Debug, Clone, PartialEq)]
pub struct SensorBendPanel {
    /// Whether the sensor is bent: the section is written.
    pub enabled: bool,
    /// The values, kept while the sensor is stock; `None` is the camera's own.
    pub bend: SensorBend,
}

impl Default for SensorBendPanel {
    /// A heat gun at 70 °C on the sensor, the bias at the camera's, stock.
    fn default() -> Self {
        Self {
            enabled: false,
            bend: SensorBend {
                temperature_c: Some(DEFAULT_TEMPERATURE_C),
                substrate_v: None,
            },
        }
    }
}

impl SensorBendPanel {
    /// The panel showing `preset`'s sensor bend, or the default values, stock.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the panel from `preset`; without a sensor bend only the switch moves.
    pub fn load(&mut self, preset: &Preset) {
        match preset.bend.sensor {
            Some(bend) => {
                self.enabled = true;
                self.bend = bend;
            }
            None => self.enabled = false,
        }
    }

    /// The preset's `[bend.sensor]` section; `None` while the sensor is stock.
    #[must_use]
    pub fn bend(&self) -> Option<SensorBend> {
        self.enabled.then_some(self.bend)
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.sensor = self.bend();
    }

    /// Draws the block for `camera`, whose `[sensor]` section holds the reference values.
    pub fn show(&mut self, ui: &mut Ui, camera: &Camera) -> Edit {
        let mut edit = Edit::default();
        edit.record(&panels::block_header(
            ui,
            "Sensor bend",
            &mut self.enabled,
            "Sensor",
            ["stock", "bent"],
        ));
        let sensor = &camera.sensor;
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            let bend = &mut self.bend;
            panels::grid(ui, "sensor-bend", |ui| {
                panels::override_row(
                    ui,
                    &mut edit,
                    "Temperature",
                    &format!(
                        "The sensor under a heat gun or freeze spray: its dark current doubles every {} °C, and with it the grain, the hot photosites and the output amplifier's glow in the readout's first corner, while the read noise rises; the camera's {} °C shown dimmed",
                        panels::significant(sensor.dark_doubling_c, 2),
                        panels::significant(sensor.reference_temp_c, 2),
                    ),
                    &mut bend.temperature_c,
                    sensor.reference_temp_c,
                    &TEMPERATURE,
                );
                panels::override_row(
                    ui,
                    &mut edit,
                    "Substrate bias",
                    &format!(
                        "A pot on VSUB, which sets the overflow drain's barrier: above the camera's {:.1} V the well shrinks and highlights clip while the drain takes the excess; below it the well deepens but the drain takes less, none {} V below, so highlights bloom along the columns, and in movie mode the electronic shutter clears less and smears more",
                        sensor.substrate_v,
                        panels::significant(sensor.drain_loss_v, 2),
                    ),
                    &mut bend.substrate_v,
                    sensor.substrate_v,
                    &bias_knob(camera),
                );
            });
            panels::hint(ui, &bend_readout(camera, bend));
        });
        edit
    }
}

/// The dark current at `bend`'s temperature as a multiple of the camera's at its reference.
///
/// `2^((T - T_ref) / dark_doubling_c)`, `T` the bend's temperature or the camera's
/// `reference_temp_c`, `T_ref`; exactly 1 at the reference.
#[must_use]
pub fn dark_multiple(camera: &Camera, bend: &SensorBend) -> f64 {
    let sensor = &camera.sensor;
    let temperature_c = bend.temperature_c.unwrap_or(sensor.reference_temp_c);
    ((temperature_c - sensor.reference_temp_c) / sensor.dark_doubling_c).exp2()
}

/// The full well and the overflow drain's share of its capacity at `bend`'s substrate bias.
///
/// As the simulator resolves them ([`vcb_core::sensor::Params`]): the well in
/// electrons, and the drain's capacity relative to the nominal bias's, 1 at nominal
/// and above. `None` if the bend's values lie outside what the camera accepts.
#[must_use]
pub fn well_at_bias(camera: &Camera, bend: &SensorBend) -> Option<(f64, f64)> {
    let mut preset = Preset::new(camera.info.name.clone());
    preset.bend.sensor = Some(*bend);
    let sensor = &camera.sensor;
    let timing = Timing::derive(camera, &preset, sensor.native_width, sensor.native_height).ok()?;
    let params = vcb_core::sensor::Params::resolve(camera, &preset, &timing).ok()?;
    Some((f64::from(params.well_e()), f64::from(params.drain_ratio)))
}

/// The readout under the sensor bend's knobs: the dark current's multiple and the full well.
///
/// The overflow drain's remaining capacity joins it while the bias lies below nominal.
#[must_use]
pub fn bend_readout(camera: &Camera, bend: &SensorBend) -> String {
    let dark = format!(
        "Dark current {} × that at {} °C",
        panels::significant(dark_multiple(camera, bend), 3),
        panels::significant(camera.sensor.reference_temp_c, 2),
    );
    match well_at_bias(camera, bend) {
        Some((well_e, drain)) if drain < 1.0 => format!(
            "{dark}; full well {well_e:.0} e, overflow drain at {:.0} %.",
            drain * 100.0
        ),
        Some((well_e, _)) => format!("{dark}; full well {well_e:.0} e."),
        None => format!("{dark}."),
    }
}
