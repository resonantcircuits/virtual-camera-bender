//! The memory bend: faulted address and data pins and the heat of the frame-buffer SDRAM.
//!
//! The panel sets the preset's `[bend.memory]` section. The processor writes the raw
//! frame into its SDRAM and reads it back a frame later. Three things a bender does to
//! the chip sit in the panel, under its one switch.
//!
//! The "Retention" block comes first: the SDRAM's temperature in degrees Celsius, a
//! heat gun on the board, dimmed at the camera's `reference_temp_c` (the preset's
//! `None`) until moved. A cell's retention halves every `retention_halving_c` degrees
//! around the median `retention_s` at the reference, log-normal over the cells with
//! the spread `retention_spread`, so the fraction of cells that leak before the
//! controller's refresh comes, every `refresh_ms`, is the normal distribution's
//! `Φ(ln(refresh / median) / spread)`; the readout shows it with the samples that hold
//! such a cell.
//!
//! The "Address pins" grid lists the address pins `a0` upward, which carry the row
//! address and then the column address, so one bad pin corrupts a row bit and a column
//! bit of the word address. The "Data pins" grid lists the data lines `d0` upward, one
//! bit of every stored sample each: the sample sits in a word's low bits, so on a
//! camera whose processor keeps fewer bits than the SDRAM has lines the pins above
//! carry no sample bit, and their rows are dimmed. Both grids have one row per pin:
//! the pin is sound, stuck low, stuck high or bridged to a second pin of its grid (both
//! then read the OR of the two), and a faulted pin's contact is solid or marginal. A
//! marginal contact comes and goes as a random telegraph process: faulted for a
//! percentage of the time, switching at a mean rate in hertz; those two values sit in a
//! grid below each pin grid.
//!
//! [`MemoryPanel`] keeps every pin's settings, also of a pin set back to sound and of
//! the whole bend while it is off, and the temperature. A pin may carry one fault, also
//! as the other end of a bridge, so a bridge's partner is chosen among the sound pins
//! of its grid and the partner's row shows the bridge instead of a choice of its own;
//! [`PinGrid::set_kind`] and [`PinGrid::set_partner`] refuse what the preset could not
//! hold.

use std::f64::consts::SQRT_2;

use eframe::egui::{self, RichText, Ui};
use vcb_core::camera::Memory;
use vcb_core::preset::{AddressFault, DataFault, FaultKind, MemoryBend};
use vcb_core::{Camera, Preset};

use crate::panels::{self, Edit, Knob, PanelId, Scale, sensor};
use crate::theme;

/// The knob of a marginal contact's faulted time: 1 to 100 % of the time.
pub const FAULTED: Knob = Knob::linear(1.0..=100.0, " % of time", 0).scaled(Scale::Times(100.0));

/// The knob of a marginal contact's switching rate: up to 1 kHz, logarithmic, with zero.
pub const SWITCHING: Knob = Knob::log(0.0..=1000.0, 0.01, " Hz");

/// The SDRAM's temperature knob: the sensor bend's, freeze spray at -20 °C to a heat gun at 120 °C.
pub const TEMPERATURE: Knob = sensor::TEMPERATURE;

/// The pin a new panel faults: `a10`, a row pin of the A520, stuck high.
pub const DEFAULT_PIN: u8 = 10;

/// Fraction of the time a contact first made marginal is faulted.
const DEFAULT_ON_FRAC: f64 = 0.3;

/// Mean switching rate of a contact first made marginal, in hertz.
const DEFAULT_SWITCH_HZ: f64 = 4.0;

/// Width of the fault choice, in points.
const KIND_WIDTH: f32 = theme::GRID * 9.0;

/// Width of the partner choice, in points.
const PARTNER_WIDTH: f32 = theme::GRID * 6.0;

/// The fault choices, in panel order; `None` is a sound pin.
const KINDS: [(Option<FaultKind>, &str); 4] = [
    (None, "sound"),
    (Some(FaultKind::Low), "low"),
    (Some(FaultKind::High), "high"),
    (Some(FaultKind::Bridge), "bridge"),
];

/// The contact choices: solid (false) or marginal (true).
const CONTACTS: [(bool, &str); 2] = [(false, "solid"), (true, "marginal")];

/// The tooltip of a pin's fault choice.
const KIND_HOVER: &str =
    "Sound; stuck at logic 0 or 1; or bridged to another pin, both reading the OR of the two";

/// The tooltip of a bridge's partner choice.
const PARTNER_HOVER: &str = "The other pin of the bridge, among the sound ones";

/// The tooltip of a faulted pin's contact choice.
const CONTACT_HOVER: &str =
    "Solid: always faulted. Marginal: a loose contact that comes and goes at random";

/// Expected number of leaking cells in the whole chip below which the readout names none.
///
/// Half a cell: the count rounds to zero, and the chip as made most likely holds none.
const NO_CELL: f64 = 0.5;

/// Expected number of leaking cells below which the readout counts them rather than give a share.
///
/// Up to ten thousand cells, a few hundredths of a percent of a 128 Mbit chip, a count
/// reads better than a share with many zeros.
const FEW_CELLS: f64 = 1e4;

/// Coefficients of the complementary error function's Chebyshev fit, from the constant up.
///
/// `erfc(z) ≈ t·exp(-z² + Σ c_k t^k)` with `t = 1 / (1 + z / 2)` for `z ≥ 0`, with a
/// fractional error below 1.2e-7 everywhere (Press et al., *Numerical Recipes*, 2nd ed.,
/// section 6.2, `erfcc`), so the share of leaking cells keeps three digits far into
/// the tail.
const ERFC: [f64; 10] = [
    -1.265_512_23,
    1.000_023_68,
    0.374_091_96,
    0.096_784_18,
    -0.186_288_06,
    0.278_868_07,
    -1.135_203_98,
    1.488_515_87,
    -0.822_152_23,
    0.170_872_77,
];

/// The settings of one pin, address or data, kept while the pin is sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinSetting {
    /// The pin, `a0` or `d0` upward.
    pub pin: u8,
    /// The fault; `None` while the pin is sound.
    pub kind: Option<FaultKind>,
    /// The other pin of a bridge, kept while the fault is another.
    pub with: u8,
    /// Whether the contact is marginal rather than solid.
    pub marginal: bool,
    /// Fraction of the time a marginal contact is faulted, in `(0, 1]`.
    pub on_frac: f64,
    /// Mean on-off cycles per second of a marginal contact.
    pub switch_hz: f64,
}

impl PinSetting {
    /// A sound pin, with the values a marginal contact starts from.
    #[must_use]
    pub fn sound(pin: u8) -> Self {
        Self {
            pin,
            kind: None,
            with: pin,
            marginal: false,
            on_frac: DEFAULT_ON_FRAC,
            switch_hz: DEFAULT_SWITCH_HZ,
        }
    }

    /// The preset's address fault for the pin; `None` while it is sound.
    #[must_use]
    pub fn fault(&self) -> Option<AddressFault> {
        let kind = self.kind?;
        Some(AddressFault {
            pin: self.pin,
            kind,
            with: (kind == FaultKind::Bridge).then_some(self.with),
            on_frac: self.marginal.then_some(self.on_frac),
            switch_hz: self.marginal.then_some(self.switch_hz),
        })
    }

    /// The preset's data fault for the pin; `None` while it is sound.
    #[must_use]
    pub fn data_fault(&self) -> Option<DataFault> {
        self.fault().map(|fault| DataFault {
            pin: fault.pin,
            kind: fault.kind,
            with: fault.with,
            on_frac: fault.on_frac,
            switch_hz: fault.switch_hz,
        })
    }
}

/// A fault of either of the preset's lists, as the grid reads it back.
#[derive(Clone, Copy)]
struct Stored {
    pin: u8,
    kind: FaultKind,
    with: Option<u8>,
    on_frac: Option<f64>,
    switch_hz: Option<f64>,
}

impl From<&AddressFault> for Stored {
    fn from(fault: &AddressFault) -> Self {
        Self {
            pin: fault.pin,
            kind: fault.kind,
            with: fault.with,
            on_frac: fault.on_frac,
            switch_hz: fault.switch_hz,
        }
    }
}

impl From<&DataFault> for Stored {
    fn from(fault: &DataFault) -> Self {
        Self {
            pin: fault.pin,
            kind: fault.kind,
            with: fault.with,
            on_frac: fault.on_frac,
            switch_hz: fault.switch_hz,
        }
    }
}

/// The settings of every pin of one grid, address or data, set so far.
///
/// The operations keep what the preset can hold: each pin carries at most one fault,
/// its own or as the other end of a bridge.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PinGrid {
    /// Every pin set so far, the preset's faults first in its order, then in order of first use.
    ///
    /// The preset's faults apply in their order, so the panel writes them back in it.
    pub pins: Vec<PinSetting>,
}

impl PinGrid {
    /// Sets the grid to `faults` in their order, every other pin sound with its settings kept.
    fn load(&mut self, faults: impl IntoIterator<Item = Stored>) {
        let mut kept = std::mem::take(&mut self.pins);
        for setting in &mut kept {
            setting.kind = None;
        }
        for fault in faults {
            let mut setting = kept
                .iter()
                .position(|setting| setting.pin == fault.pin)
                .map_or_else(|| PinSetting::sound(fault.pin), |i| kept.remove(i));
            setting.kind = Some(fault.kind);
            if let Some(with) = fault.with {
                setting.with = with;
            }
            setting.marginal = fault.on_frac.is_some() || fault.switch_hz.is_some();
            setting.on_frac = fault.on_frac.unwrap_or(setting.on_frac);
            setting.switch_hz = fault.switch_hz.unwrap_or(setting.switch_hz);
            self.pins.push(setting);
        }
        self.pins.extend(kept);
    }

    /// The settings of `pin`; a pin never set is sound.
    #[must_use]
    pub fn setting(&self, pin: u8) -> PinSetting {
        self.pins
            .iter()
            .find(|setting| setting.pin == pin)
            .copied()
            .unwrap_or_else(|| PinSetting::sound(pin))
    }

    /// The settings of `pin` to change, added after the others if it was never set.
    fn setting_mut(&mut self, pin: u8) -> &mut PinSetting {
        let index = self.pins.iter().position(|setting| setting.pin == pin);
        let index = index.unwrap_or_else(|| {
            self.pins.push(PinSetting::sound(pin));
            self.pins.len() - 1
        });
        &mut self.pins[index]
    }

    /// The pin whose bridge names `pin` as its other end, if any.
    #[must_use]
    pub fn bridged_by(&self, pin: u8) -> Option<u8> {
        self.pins
            .iter()
            .find(|setting| {
                setting.kind == Some(FaultKind::Bridge) && setting.with == pin && setting.pin != pin
            })
            .map(|setting| setting.pin)
    }

    /// Whether `pin` carries no fault, neither its own nor as a bridge's other end.
    #[must_use]
    pub fn is_free(&self, pin: u8) -> bool {
        self.setting(pin).kind.is_none() && self.bridged_by(pin).is_none()
    }

    /// The pins below `count` that `pin`'s bridge may join: the free ones, and its present partner.
    #[must_use]
    pub fn partners(&self, pin: u8, count: u8) -> Vec<u8> {
        let setting = self.setting(pin);
        let current = (setting.kind == Some(FaultKind::Bridge)).then_some(setting.with);
        (0..count)
            .filter(|&other| other != pin && (Some(other) == current || self.is_free(other)))
            .collect()
    }

    /// Sets the fault of `pin` among `count` pins, `None` for sound; returns whether it changed.
    ///
    /// Refused for the other end of a bridge, which carries that bridge, and for a bridge
    /// when no pin is free to join. A new bridge joins the pin it last joined if that is
    /// free, else the nearest free pin, the higher first.
    pub fn set_kind(&mut self, pin: u8, kind: Option<FaultKind>, count: u8) -> bool {
        let setting = self.setting(pin);
        if self.bridged_by(pin).is_some() || setting.kind == kind {
            return false;
        }
        if kind == Some(FaultKind::Bridge) {
            let free = |other: u8| other != pin && other < count && self.is_free(other);
            let with = if free(setting.with) {
                Some(setting.with)
            } else {
                (1..count)
                    .flat_map(|distance| [pin.checked_add(distance), pin.checked_sub(distance)])
                    .flatten()
                    .find(|&other| free(other))
            };
            let Some(with) = with else {
                return false;
            };
            self.setting_mut(pin).with = with;
        }
        self.setting_mut(pin).kind = kind;
        true
    }

    /// Joins `pin`'s bridge to `with` among `count` pins; returns whether it changed.
    ///
    /// Refused unless `pin` is bridged, and for a bridge to itself, to a pin beyond the
    /// SDRAM's or to a pin that carries a fault.
    pub fn set_partner(&mut self, pin: u8, with: u8, count: u8) -> bool {
        let setting = self.setting(pin);
        if setting.kind != Some(FaultKind::Bridge)
            || with == setting.with
            || with == pin
            || with >= count
            || !self.is_free(with)
        {
            return false;
        }
        self.setting_mut(pin).with = with;
        true
    }

    /// Makes the contact of `pin` marginal or solid; returns whether it changed.
    pub fn set_marginal(&mut self, pin: u8, marginal: bool) -> bool {
        if self.setting(pin).marginal == marginal {
            return false;
        }
        self.setting_mut(pin).marginal = marginal;
        true
    }

    /// The faulted pins at or beyond `count`, which the camera's SDRAM lacks, by name.
    fn missing(&self, bank: Bank, count: u8) -> Vec<String> {
        self.pins
            .iter()
            .filter(|setting| setting.kind.is_some() && setting.pin >= count)
            .map(|setting| bank.name(setting.pin))
            .collect()
    }

    /// The pin grid: one row per pin with its fault, a bridge's partner and the contact.
    ///
    /// `about` gives a pin's tooltip; the rows of pins `inert` names are dimmed.
    fn show(
        &mut self,
        ui: &mut Ui,
        bank: Bank,
        count: u8,
        about: impl Fn(u8) -> String,
        inert: impl Fn(u8) -> bool,
    ) -> Edit {
        let mut edit = Edit::default();
        egui::Grid::new(bank.salt("pins"))
            .num_columns(4)
            .spacing([theme::GRID, theme::GRID * 0.5])
            .show(ui, |ui| {
                ui.spacing_mut().button_padding = egui::vec2(theme::GRID * 0.5, theme::GRID * 0.25);
                ui.spacing_mut().item_spacing.x = theme::GRID * 0.5;
                for heading in ["Pin", "Fault", "With", "Contact"] {
                    ui.label(RichText::new(heading).small().color(theme::TEXT_WEAK));
                }
                ui.end_row();
                let opacity = ui.opacity();
                for pin in 0..count {
                    if inert(pin) {
                        ui.multiply_opacity(panels::OFF_OPACITY);
                    }
                    ui.label(bank.name(pin)).on_hover_text(about(pin));
                    if let Some(owner) = self.bridged_by(pin) {
                        let owner = bank.name(owner);
                        let hover = format!(
                            "Bridged to {owner}: both read the OR of the two; set {owner} to change it"
                        );
                        let weak = |text: String| RichText::new(text).color(theme::TEXT_WEAK);
                        ui.label(weak("bridged".to_owned())).on_hover_text(&hover);
                        ui.label(weak(owner)).on_hover_text(&hover);
                        ui.label("");
                    } else {
                        edit |= self.pin_row(ui, bank, pin, count);
                    }
                    ui.end_row();
                    ui.set_opacity(opacity);
                }
            });
        edit
    }

    /// The fault, partner and contact cells of a pin that is not a bridge's other end.
    fn pin_row(&mut self, ui: &mut Ui, bank: Bank, pin: u8, count: u8) -> Edit {
        let mut edit = Edit::default();
        let mut kind = self.setting(pin).kind;
        let picked = panels::dropdown(ui, (bank.salt("kind"), pin), &mut kind, &KINDS, KIND_WIDTH)
            .on_hover_text(KIND_HOVER);
        edit.changed |= picked.changed() && self.set_kind(pin, kind, count);
        let setting = self.setting(pin);
        if setting.kind == Some(FaultKind::Bridge) {
            let options: Vec<(u8, String)> = self
                .partners(pin, count)
                .into_iter()
                .map(|other| (other, bank.name(other)))
                .collect();
            let mut with = setting.with;
            let picked = panels::dropdown(
                ui,
                (bank.salt("with"), pin),
                &mut with,
                &options,
                PARTNER_WIDTH,
            )
            .on_hover_text(PARTNER_HOVER);
            edit.changed |= picked.changed() && self.set_partner(pin, with, count);
        } else {
            ui.label("");
        }
        if setting.kind.is_some() {
            let mut marginal = setting.marginal;
            let picked = panels::choice(ui, &mut marginal, &CONTACTS).on_hover_text(CONTACT_HOVER);
            edit.changed |= picked.changed() && self.set_marginal(pin, marginal);
        } else {
            ui.label("");
        }
        edit
    }

    /// The knobs of the grid's marginal contacts, pin by pin, below `count`.
    fn show_marginal(&mut self, ui: &mut Ui, bank: Bank, count: u8) -> Edit {
        let mut edit = Edit::default();
        let mut marginal: Vec<u8> = self
            .pins
            .iter()
            .filter(|setting| setting.kind.is_some() && setting.marginal && setting.pin < count)
            .map(|setting| setting.pin)
            .collect();
        if marginal.is_empty() {
            return edit;
        }
        marginal.sort_unstable();
        panels::section(ui, bank.marginal_title());
        panels::grid(ui, &bank.salt("marginal"), |ui| {
            for pin in marginal {
                let name = bank.name(pin);
                let setting = self.setting_mut(pin);
                panels::knob_row(
                    ui,
                    &mut edit,
                    &format!("{name} faulted"),
                    "Fraction of the time the loose contact is faulted; 100 % is solid",
                    &mut setting.on_frac,
                    &FAULTED,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    &format!("{name} switching"),
                    "Mean on-off cycles a second, at random in camera time; 0 freezes the contact as it powered up",
                    &mut setting.switch_hz,
                    &SWITCHING,
                );
            }
        });
        edit
    }
}

/// The two grids of pins, for their names and widget ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bank {
    /// The multiplexed address pins, `a0` upward.
    Address,
    /// The data lines, `d0` upward.
    Data,
}

impl Bank {
    /// The name of `pin` in this bank, such as `a10` or `d9`.
    fn name(self, pin: u8) -> String {
        match self {
            Self::Address => format!("a{pin}"),
            Self::Data => format!("d{pin}"),
        }
    }

    /// A widget id for `what` in this bank's grid.
    fn salt(self, what: &str) -> String {
        match self {
            Self::Address => format!("memory-{what}"),
            Self::Data => format!("memory-data-{what}"),
        }
    }

    /// The title of the bank's marginal contacts.
    fn marginal_title(self) -> &'static str {
        match self {
            Self::Address => "Marginal address contacts",
            Self::Data => "Marginal data contacts",
        }
    }
}

/// The memory bend's switch, the settings of every pin set so far, and the temperature.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryPanel {
    /// Whether the SDRAM is bent: the section is written.
    pub enabled: bool,
    /// The address pins, `a0` upward.
    pub address: PinGrid,
    /// The data pins, `d0` upward.
    pub data: PinGrid,
    /// The SDRAM's temperature in degrees Celsius; `None` is the camera's `reference_temp_c`.
    pub temperature_c: Option<f64>,
}

impl Default for MemoryPanel {
    /// [`DEFAULT_PIN`] stuck high with a solid contact, the other pins sound, the chip at the
    /// camera's temperature; off.
    fn default() -> Self {
        Self {
            enabled: false,
            address: PinGrid {
                pins: vec![PinSetting {
                    kind: Some(FaultKind::High),
                    ..PinSetting::sound(DEFAULT_PIN)
                }],
            },
            data: PinGrid::default(),
            temperature_c: None,
        }
    }
}

impl MemoryPanel {
    /// The panel showing `preset`'s memory bend, or the default settings, off.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the panel from `preset`: its faults and temperature, every other pin sound with
    /// its settings kept.
    pub fn load(&mut self, preset: &Preset) {
        let Some(memory) = &preset.bend.memory else {
            self.enabled = false;
            return;
        };
        self.enabled = true;
        self.address.load(memory.faults.iter().map(Stored::from));
        self.data.load(memory.data_faults.iter().map(Stored::from));
        self.temperature_c = memory.temperature_c;
    }

    /// The preset's `[bend.memory]` section; `None` while the bend is off.
    #[must_use]
    pub fn bend(&self) -> Option<MemoryBend> {
        self.enabled.then(|| MemoryBend {
            faults: self
                .address
                .pins
                .iter()
                .filter_map(PinSetting::fault)
                .collect(),
            data_faults: self
                .data
                .pins
                .iter()
                .filter_map(PinSetting::data_fault)
                .collect(),
            temperature_c: self.temperature_c,
        })
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.memory = self.bend();
    }

    /// Draws the panel for `camera`, whose `[memory]` section names the pins and holds the
    /// cells' retention, and whose `[adc]` section the sample's width.
    pub fn show(&mut self, ui: &mut Ui, camera: &Camera) -> Edit {
        let mut edit = Edit::default();
        edit.record(&panels::bend_header(
            ui,
            PanelId::Memory.title(),
            &mut self.enabled,
            "SDRAM",
            ["stock", "bent"],
        ));
        let memory = &camera.memory;
        let sample_bits = camera.adc.dsp_bits;
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            edit |= self.show_retention(ui, camera);

            panels::section(ui, "Address pins");
            panels::hint(ui, &organisation(memory));
            let count = pin_count(memory);
            edit |= self.address.show(
                ui,
                Bank::Address,
                count,
                |pin| pin_bits(memory, pin),
                |_| false,
            );
            missing_hint(ui, &self.address.missing(Bank::Address, count));
            edit |= self.address.show_marginal(ui, Bank::Address, count);

            panels::section(ui, "Data pins");
            panels::hint(ui, &data_organisation(memory, sample_bits));
            let count = memory.data_pins;
            edit |= self.data.show(
                ui,
                Bank::Data,
                count,
                |pin| data_bit(sample_bits, pin),
                |pin| pin >= sample_bits,
            );
            missing_hint(ui, &self.data.missing(Bank::Data, count));
            edit |= self.data.show_marginal(ui, Bank::Data, count);
        });
        edit
    }

    /// The temperature knob and the share of cells that leak before the refresh.
    fn show_retention(&mut self, ui: &mut Ui, camera: &Camera) -> Edit {
        let mut edit = Edit::default();
        let memory = &camera.memory;
        panels::section(ui, "Retention");
        panels::grid(ui, "memory-retention", |ui| {
            panels::override_row(
                ui,
                &mut edit,
                "Temperature",
                &format!(
                    "A heat gun on the board or freeze spray on the SDRAM: a cell's retention halves every {} °C, and a cell that leaks before the refresh, every {} ms, loses its bit, so the weakest show as single bright and dark sites first, then a frame eaten by them; the camera's {} °C shown dimmed",
                    panels::significant(memory.retention_halving_c, 2),
                    panels::significant(memory.refresh_ms, 2),
                    panels::significant(memory.reference_temp_c, 2),
                ),
                &mut self.temperature_c,
                memory.reference_temp_c,
                &TEMPERATURE,
            );
        });
        let temperature_c = self.temperature_c.unwrap_or(memory.reference_temp_c);
        panels::hint(ui, &retention_readout(camera, temperature_c));
        edit
    }
}

/// A hint naming the faulted pins the camera's SDRAM lacks, if there are any.
fn missing_hint(ui: &mut Ui, missing: &[String]) {
    if !missing.is_empty() {
        panels::hint(
            ui,
            &format!(
                "The preset faults {}, which this SDRAM lacks.",
                missing.join(", ")
            ),
        );
    }
}

/// How many address pins of `memory` are bendable: `a0` to `a(max(column_bits, row_bits) - 1)`.
#[must_use]
pub fn pin_count(memory: &Memory) -> u8 {
    memory.column_bits.max(memory.row_bits)
}

/// The bits of the word address that `pin` carries, for its tooltip.
#[must_use]
pub fn pin_bits(memory: &Memory, pin: u8) -> String {
    let column = (pin < memory.column_bits).then_some(pin);
    let row = (pin < memory.row_bits).then(|| memory.column_bits + memory.bank_bits + pin);
    let bits = match (column, row) {
        (Some(column), Some(row)) => format!("column bit {column} and row bit {row}"),
        (Some(column), None) => format!("column bit {column}"),
        (None, Some(row)) => format!("row bit {row}"),
        (None, None) => "no bit".to_owned(),
    };
    format!("a{pin} carries {bits} of the word address")
}

/// The bit of the stored sample that data pin `pin` carries, for its tooltip.
///
/// `sample_bits` is the processor's sample width, `[adc] dsp_bits`, whose code a word
/// holds in its low bits.
#[must_use]
pub fn data_bit(sample_bits: u8, pin: u8) -> String {
    if pin < sample_bits {
        let weight = 1_u32.checked_shl(u32::from(pin)).unwrap_or(0);
        let plural = if weight == 1 { "" } else { "s" };
        format!(
            "d{pin} carries bit {pin} of the {sample_bits}-bit sample, {weight} code{plural}: stuck high it adds them to every sample without the bit, stuck low it takes them from every sample with it"
        )
    } else {
        format!(
            "d{pin} carries no bit of the {sample_bits}-bit sample on this camera, so its fault changes nothing"
        )
    }
}

/// Which data pins carry the sample's bits, and what their faults do.
fn data_organisation(memory: &Memory, sample_bits: u8) -> String {
    let pins = memory.data_pins;
    let carried = sample_bits.min(pins);
    let top = carried.saturating_sub(1);
    let mut text = format!(
        "Pins d0 to d{top} hold the {sample_bits}-bit sample, d{top} its top bit; a stuck pin shifts black with the signal."
    );
    if pins > carried {
        let span = if pins - 1 == carried {
            format!("Pin d{carried} carries")
        } else {
            format!("Pins d{carried} to d{} carry", pins - 1)
        };
        text = format!(
            "{text} {span} no sample bit on this camera: dimmed, their faults change nothing."
        );
    }
    text
}

/// Which address pins carry which bits, and what their faults do.
fn organisation(memory: &Memory) -> String {
    let (columns, rows) = (memory.column_bits, memory.row_bits);
    let both = columns.min(rows).max(1);
    let span = |from: u8, to: u8| {
        if from == to {
            format!("a{from}")
        } else {
            format!("a{from} to a{to}")
        }
    };
    let mut text = format!("Pins {} carry a column and a row bit", span(0, both - 1));
    if rows > columns {
        text = format!("{text}, {} a row bit", span(columns, rows - 1));
    } else if columns > rows {
        text = format!("{text}, {} a column bit", span(rows, columns - 1));
    }
    format!(
        "{text}. A solid fault folds the frame in powers of two; a marginal one tears it and lets old frames through."
    )
}

/// The standard normal distribution's cumulative probability at `x`, `Φ(x)`.
///
/// Through the complementary error function, `Φ(x) = erfc(-x / √2) / 2`, to a relative
/// error below 1.2e-7 also deep in either tail.
#[must_use]
pub fn normal_cdf(x: f64) -> f64 {
    0.5 * erfc(-x / SQRT_2)
}

/// The complementary error function, `erfc(x) = 1 - erf(x)`, from its Chebyshev fit [`ERFC`].
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let series = ERFC
        .iter()
        .rev()
        .fold(0.0_f64, |sum, &coefficient| sum.mul_add(t, coefficient));
    let tail = t * (-z * z + series).exp();
    if x >= 0.0 { tail } else { 2.0 - tail }
}

/// The median retention of the SDRAM's cells at `temperature_c`, in seconds.
///
/// `retention_s * 2^(-(T - reference_temp_c) / retention_halving_c)`: the camera's
/// median at its reference, halving every `retention_halving_c` degrees above it.
#[must_use]
pub fn median_retention_s(memory: &Memory, temperature_c: f64) -> f64 {
    memory.retention_s
        * (-(temperature_c - memory.reference_temp_c) / memory.retention_halving_c).exp2()
}

/// The fraction of the SDRAM's cells that leak before the refresh at `temperature_c`.
///
/// The retention is log-normal over the cells around the median
/// [`median_retention_s`] with the spread `retention_spread` of its natural log, so
/// the fraction below the refresh period is `Φ(ln(refresh / median) / spread)`; with
/// no spread every cell has the median, and all or none leak.
#[must_use]
pub fn weak_fraction(memory: &Memory, temperature_c: f64) -> f64 {
    let refresh_s = memory.refresh_ms * 1e-3;
    let log_ratio = (refresh_s / median_retention_s(memory, temperature_c)).ln();
    if memory.retention_spread > 0.0 {
        normal_cdf(log_ratio / memory.retention_spread)
    } else if log_ratio > 0.0 {
        1.0
    } else {
        0.0
    }
}

/// The cells of the SDRAM: one per data pin of every word.
#[must_use]
pub fn cell_count(memory: &Memory) -> f64 {
    f64::from(memory.address_bits()).exp2() * f64::from(memory.data_pins)
}

/// The fraction of stored samples that hold at least one leaking cell at `temperature_c`.
///
/// `1 - (1 - f)^dsp_bits`, `f` the [`weak_fraction`]: a sample occupies the low
/// `[adc] dsp_bits` cells of its word.
#[must_use]
pub fn weak_sample_fraction(camera: &Camera, temperature_c: f64) -> f64 {
    let fraction = weak_fraction(&camera.memory, temperature_c);
    1.0 - (1.0 - fraction).powi(i32::from(camera.adc.dsp_bits))
}

/// The readout under the temperature knob: the median retention and what leaks before the refresh.
#[must_use]
pub fn retention_readout(camera: &Camera, temperature_c: f64) -> String {
    let memory = &camera.memory;
    let median_s = median_retention_s(memory, temperature_c);
    let median = if median_s >= 1.0 {
        format!("{} s", panels::significant(median_s, 3))
    } else {
        format!("{} ms", panels::significant(median_s * 1e3, 3))
    };
    let lead = format!(
        "Median retention {median} against the {} ms refresh",
        panels::significant(memory.refresh_ms, 2)
    );
    let cells = cell_count(memory);
    let millions = panels::significant(cells / 1e6, 3);
    let fraction = weak_fraction(memory, temperature_c);
    let weak = fraction * cells;
    if weak < NO_CELL {
        format!("{lead}: no cell of the {millions} million leaks before it comes.")
    } else if weak < FEW_CELLS {
        format!("{lead}: about {weak:.0} of the {millions} million cells leak before it comes.")
    } else {
        format!(
            "{lead}: {} % of the cells leak before it comes, in {} % of the samples.",
            panels::significant(fraction * 100.0, 3),
            panels::significant(weak_sample_fraction(camera, temperature_c) * 100.0, 3),
        )
    }
}
