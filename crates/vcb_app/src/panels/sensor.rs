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
//!
//! Last, the "Light leak" block sets the preset's `[bend.leak]` section with its own
//! power switch, the body sealed or open: light entering behind the lens from one edge
//! of the sensor, past the aperture, the shutter and the IR-cut filter. The edge is a
//! choice of the four in the sensor's landscape orientation; the radiance at the
//! opening is in multiples of reference white's, the spread over which the glow falls
//! by `1/e` a percentage of the frame across the edge, the colour three multiples of
//! white's red, green and blue in linear sRGB, and the near infrared a multiple of
//! daylight's share. A readout shows the glow's `1/e` distance in the sensor's columns
//! or rows and what of it reaches the far edge, `e^(-1 / spread_frac)`. [`LeakPanel`]
//! keeps the values while the body is sealed.

use eframe::egui::Ui;
use vcb_core::preset::{Edge, LeakBend, Mode, SensorBend, SensorOverride};
use vcb_core::{Camera, Preset, Timing};

use crate::panels::{self, Edit, Knob, Scale};
use crate::theme;

/// The exposure knob: 0.1 ms to 1 s, logarithmic, stored in seconds.
pub const EXPOSURE: Knob = Knob::log(0.1..=1000.0, 0.1, " ms").scaled(Scale::Times(1e3));

/// The temperature knob: freeze spray at -20 °C to a heat gun at 120 °C, in whole degrees.
pub const TEMPERATURE: Knob = Knob::linear(-20.0..=120.0, " °C", 0);

/// How far the substrate-bias knob turns either way from the camera's nominal bias, in volts.
pub const BIAS_SPAN_V: f64 = 4.0;

/// The temperature of a fresh sensor bend: a heat gun on the sensor, in degrees Celsius.
pub const DEFAULT_TEMPERATURE_C: f64 = 70.0;

/// The leak's radiance knob: a hundredth to ten times reference white, logarithmic.
pub const RADIANCE: Knob = Knob::log(0.01..=10.0, 0.01, " × white");

/// The spread knob of a leak from the left or right: 1 to 200 % of the frame's width, logarithmic.
pub const SPREAD_WIDTH: Knob =
    Knob::log(1.0..=200.0, 1.0, " % of width").scaled(Scale::Times(100.0));

/// The spread knob of a leak from the top or bottom: 1 to 200 % of the frame's height, logarithmic.
pub const SPREAD_HEIGHT: Knob =
    Knob::log(1.0..=200.0, 1.0, " % of height").scaled(Scale::Times(100.0));

/// The knob of each of the leak's colour channels: 0 to twice white's, linear.
pub const COLOUR: Knob = Knob::linear(0.0..=2.0, " × white", 2);

/// The leak's near-infrared knob: 0 to five times daylight's share, linear.
pub const NIR: Knob = Knob::linear(0.0..=5.0, " × daylight", 2);

/// The edge choices in panel order, each read as the preset writes it.
pub const EDGES: [(Edge, &str); 4] = [
    (Edge::Left, "left"),
    (Edge::Right, "right"),
    (Edge::Top, "top"),
    (Edge::Bottom, "bottom"),
];

/// Width of the edge choice, in points.
const EDGE_WIDTH: f32 = theme::GRID * 9.0;

/// The tooltip of the leak's edge.
const EDGE_HOVER: &str = "The side of the sensor the light gets in from, through a drilled hole, an opened seam or a loose lens mount; in the sensor's landscape orientation, so a portrait frame shows the leak turned with the picture";

/// The colour channels in panel order: the label, the index into `colour` and the tooltip.
const CHANNELS: [(&str, usize, &str); 3] = [
    (
        "Red",
        0,
        "The leak's red in linear sRGB relative to white's, 1 for daylight; light through red plastic keeps its red",
    ),
    (
        "Green",
        1,
        "The leak's green in linear sRGB relative to white's, 1 for daylight; red plastic passes little of it",
    ),
    (
        "Blue",
        2,
        "The leak's blue in linear sRGB relative to white's, 1 for daylight; red plastic passes almost none",
    ),
];

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

/// The light leak's switch and values, a block at the end of the sensor panel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LeakPanel {
    /// Whether the body is open: the section is written.
    pub enabled: bool,
    /// The values, kept while the body is sealed.
    pub bend: LeakBend,
}

impl LeakPanel {
    /// The panel showing `preset`'s light leak, or the default values, sealed.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the panel from `preset`; without a light leak only the switch moves.
    pub fn load(&mut self, preset: &Preset) {
        match preset.bend.leak {
            Some(bend) => {
                self.enabled = true;
                self.bend = bend;
            }
            None => self.enabled = false,
        }
    }

    /// The preset's `[bend.leak]` section; `None` while the body is sealed.
    #[must_use]
    pub fn bend(&self) -> Option<LeakBend> {
        self.enabled.then_some(self.bend)
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.leak = self.bend();
    }

    /// Draws the block for `camera`, whose sensor's columns and rows the readout counts.
    pub fn show(&mut self, ui: &mut Ui, camera: &Camera) -> Edit {
        let mut edit = Edit::default();
        edit.record(&panels::block_header(
            ui,
            "Light leak",
            &mut self.enabled,
            "Body",
            ["sealed", "open"],
        ));
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            let bend = &mut self.bend;
            panels::grid(ui, "sensor-leak", |ui| {
                panels::row_label(ui, "Edge").on_hover_text(EDGE_HOVER);
                edit.record(
                    &panels::dropdown(ui, "sensor-leak-edge", &mut bend.edge, &EDGES, EDGE_WIDTH)
                        .on_hover_text(EDGE_HOVER),
                );
                ui.label("");
                ui.end_row();
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Radiance",
                    "Brightness of the leak where it enters, in multiples of reference white's radiance: it comes in behind the aperture, so it is not stopped down with the scene, and auto-exposure metering through the sensor darkens the scene to make room for it",
                    &mut bend.radiance,
                    &RADIANCE,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Spread",
                    "How far into the frame the glow falls to 1/e of its brightness at the opening, as a share of the frame across the leak: a soft glow scattered by the cavity, tight from a small hole, a veil from a wide seam",
                    &mut bend.spread_frac,
                    spread_knob(bend.edge),
                );
                for (label, channel, hover) in CHANNELS {
                    panels::knob_row(
                        ui,
                        &mut edit,
                        label,
                        hover,
                        &mut bend.colour[channel],
                        &COLOUR,
                    );
                }
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Near infrared",
                    "The leak's near infrared against its visible light, 1 for daylight: it enters behind the IR-cut filter, so the sensor takes all of it, and a daylight leak develops pink under the daylight balance, as a full-spectrum camera does; 0 for a light without infrared",
                    &mut bend.nir,
                    &NIR,
                );
            });
            panels::hint(ui, &leak_readout(camera, bend));
        });
        edit
    }
}

/// The spread knob for a leak from `edge`: in percent of the frame's width or height.
#[must_use]
pub fn spread_knob(edge: Edge) -> &'static Knob {
    match edge {
        Edge::Left | Edge::Right => &SPREAD_WIDTH,
        Edge::Top | Edge::Bottom => &SPREAD_HEIGHT,
    }
}

/// The edge across the sensor from `edge`.
#[must_use]
pub fn opposite(edge: Edge) -> Edge {
    match edge {
        Edge::Left => Edge::Right,
        Edge::Right => Edge::Left,
        Edge::Top => Edge::Bottom,
        Edge::Bottom => Edge::Top,
    }
}

/// The share of the leak's brightness at the opening left at the far edge, `e^(-1 / spread_frac)`.
#[must_use]
pub fn far_edge_share(bend: &LeakBend) -> f64 {
    (-1.0 / bend.spread_frac).exp()
}

/// The readout under the leak's knobs: the brightness at the opening, the glow's reach, the far edge.
///
/// The `1/e` distance is counted in the sensor's columns for a leak from the left or
/// right and in its rows for one from the top or bottom.
#[must_use]
pub fn leak_readout(camera: &Camera, bend: &LeakBend) -> String {
    if bend.radiance <= 0.0 {
        return "No light gets in.".to_owned();
    }
    let sensor = &camera.sensor;
    let (extent, lines) = match bend.edge {
        Edge::Left | Edge::Right => (sensor.native_width, "columns"),
        Edge::Top | Edge::Bottom => (sensor.native_height, "rows"),
    };
    let reach = if bend.spread_frac >= 1.0 {
        format!("1/e beyond the sensor's {extent} {lines}")
    } else {
        format!(
            "1/e after {:.0} of the sensor's {extent} {lines}",
            bend.spread_frac * f64::from(extent)
        )
    };
    format!(
        "{} × white at the {} edge, {reach}, {} % at the {} edge.",
        panels::significant(bend.radiance, 3),
        bend.edge,
        panels::significant(far_edge_share(bend) * 100.0, 3),
        opposite(bend.edge),
    )
}
