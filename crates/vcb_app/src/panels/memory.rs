//! The memory bend: faulted address pins on the frame-buffer SDRAM.
//!
//! The panel sets the preset's `[bend.memory]` section. The processor writes the raw
//! frame into its SDRAM and reads it back a frame later through the address pins
//! `a0` upward, which carry the row address and then the column address, so one bad
//! pin corrupts a row bit and a column bit of the word address. The panel lists the
//! camera's bendable pins in a grid, one row each: the pin is sound, stuck low, stuck
//! high or bridged to a second pin (both then read the OR of the two), and a faulted
//! pin's contact is solid or marginal. A marginal contact comes and goes as a random
//! telegraph process: faulted for a percentage of the time, switching at a mean rate
//! in hertz; those two values sit in their own grid below the pins.
//!
//! [`MemoryPanel`] keeps every pin's settings, also of a pin set back to sound and of
//! the whole bend while it is off. A pin may carry one fault, also as the other end of
//! a bridge, so a bridge's partner is chosen among the sound pins and the partner's row
//! shows the bridge instead of a choice of its own; [`MemoryPanel::set_kind`] and
//! [`MemoryPanel::set_partner`] refuse what the preset could not hold.

use eframe::egui::{self, RichText, Ui};
use vcb_core::camera::Memory;
use vcb_core::preset::{AddressFault, FaultKind, MemoryBend};
use vcb_core::{Camera, Preset};

use crate::panels::{self, Edit, Knob, PanelId, Scale};
use crate::theme;

/// The knob of a marginal contact's faulted time: 1 to 100 % of the time.
pub const FAULTED: Knob = Knob::linear(1.0..=100.0, " % of time", 0).scaled(Scale::Times(100.0));

/// The knob of a marginal contact's switching rate: up to 1 kHz, logarithmic, with zero.
pub const SWITCHING: Knob = Knob::log(0.0..=1000.0, 0.01, " Hz");

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

/// The settings of one address pin, kept while the pin is sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinSetting {
    /// The address pin, `a0` upward.
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

    /// The preset's fault for the pin; `None` while it is sound.
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
}

/// The memory bend's switch and the settings of every pin set so far.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryPanel {
    /// Whether the address pins are faulted: the bend is patched in.
    pub enabled: bool,
    /// Every pin set so far, the preset's faults first in its order, then in order of first use.
    ///
    /// The preset's faults apply in their order, so the panel writes them back in it.
    pub pins: Vec<PinSetting>,
}

impl Default for MemoryPanel {
    /// [`DEFAULT_PIN`] stuck high with a solid contact, the pins sound.
    fn default() -> Self {
        Self {
            enabled: false,
            pins: vec![PinSetting {
                kind: Some(FaultKind::High),
                ..PinSetting::sound(DEFAULT_PIN)
            }],
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

    /// Sets the panel from `preset`: its faults, and every other pin sound with its settings kept.
    pub fn load(&mut self, preset: &Preset) {
        let Some(memory) = &preset.bend.memory else {
            self.enabled = false;
            return;
        };
        self.enabled = true;
        let mut kept = std::mem::take(&mut self.pins);
        for setting in &mut kept {
            setting.kind = None;
        }
        for fault in &memory.faults {
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

    /// The preset's `[bend.memory]` section; `None` while the bend is off.
    #[must_use]
    pub fn bend(&self) -> Option<MemoryBend> {
        self.enabled.then(|| MemoryBend {
            faults: self.pins.iter().filter_map(PinSetting::fault).collect(),
        })
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.memory = self.bend();
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

    /// Draws the panel for `camera`, whose `[memory]` section names the pins.
    pub fn show(&mut self, ui: &mut Ui, camera: &Camera) -> Edit {
        let mut edit = Edit::default();
        edit.record(&panels::bend_header(
            ui,
            PanelId::Memory.title(),
            &mut self.enabled,
            "SDRAM address pins",
            ["sound", "faulted"],
        ));
        let memory = &camera.memory;
        let count = pin_count(memory);
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            panels::section(ui, "Address pins");
            panels::hint(ui, &organisation(memory));
            edit |= self.show_pins(ui, memory, count);
            let missing: Vec<String> = self
                .pins
                .iter()
                .filter(|setting| setting.kind.is_some() && setting.pin >= count)
                .map(|setting| format!("a{}", setting.pin))
                .collect();
            if !missing.is_empty() {
                panels::hint(
                    ui,
                    &format!(
                        "The preset faults {}, which this SDRAM lacks.",
                        missing.join(", ")
                    ),
                );
            }
            edit |= self.show_marginal(ui, count);
        });
        edit
    }

    /// The pin grid: one row per pin with its fault, a bridge's partner and the contact.
    fn show_pins(&mut self, ui: &mut Ui, memory: &Memory, count: u8) -> Edit {
        let mut edit = Edit::default();
        egui::Grid::new("memory-pins")
            .num_columns(4)
            .spacing([theme::GRID, theme::GRID * 0.5])
            .show(ui, |ui| {
                ui.spacing_mut().button_padding = egui::vec2(theme::GRID * 0.5, theme::GRID * 0.25);
                ui.spacing_mut().item_spacing.x = theme::GRID * 0.5;
                for heading in ["Pin", "Fault", "With", "Contact"] {
                    ui.label(RichText::new(heading).small().color(theme::TEXT_WEAK));
                }
                ui.end_row();
                for pin in 0..count {
                    ui.label(format!("a{pin}"))
                        .on_hover_text(pin_bits(memory, pin));
                    if let Some(owner) = self.bridged_by(pin) {
                        let hover = format!(
                            "Bridged to a{owner}: both read the OR of the two; set a{owner} to change it"
                        );
                        let weak = |text: String| RichText::new(text).color(theme::TEXT_WEAK);
                        ui.label(weak("bridged".to_owned())).on_hover_text(&hover);
                        ui.label(weak(format!("a{owner}"))).on_hover_text(&hover);
                        ui.label("");
                        ui.end_row();
                        continue;
                    }
                    edit |= self.pin_row(ui, pin, count);
                    ui.end_row();
                }
            });
        edit
    }

    /// The fault, partner and contact cells of a pin that is not a bridge's other end.
    fn pin_row(&mut self, ui: &mut Ui, pin: u8, count: u8) -> Edit {
        let mut edit = Edit::default();
        let mut kind = self.setting(pin).kind;
        let picked = panels::dropdown(ui, ("memory-kind", pin), &mut kind, &KINDS, KIND_WIDTH)
            .on_hover_text(KIND_HOVER);
        edit.changed |= picked.changed() && self.set_kind(pin, kind, count);
        let setting = self.setting(pin);
        if setting.kind == Some(FaultKind::Bridge) {
            let options: Vec<(u8, String)> = self
                .partners(pin, count)
                .into_iter()
                .map(|other| (other, format!("a{other}")))
                .collect();
            let mut with = setting.with;
            let picked =
                panels::dropdown(ui, ("memory-with", pin), &mut with, &options, PARTNER_WIDTH)
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

    /// The knobs of the marginal contacts, pin by pin.
    fn show_marginal(&mut self, ui: &mut Ui, count: u8) -> Edit {
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
        panels::section(ui, "Marginal contacts");
        panels::grid(ui, "memory-marginal", |ui| {
            for pin in marginal {
                let setting = self.setting_mut(pin);
                panels::knob_row(
                    ui,
                    &mut edit,
                    &format!("a{pin} faulted"),
                    "Fraction of the time the loose contact is faulted; 100 % is solid",
                    &mut setting.on_frac,
                    &FAULTED,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    &format!("a{pin} switching"),
                    "Mean on-off cycles a second, at random in camera time; 0 freezes the contact as it powered up",
                    &mut setting.switch_hz,
                    &SWITCHING,
                );
            }
        });
        edit
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

/// Which pins carry which bits, and what their faults do.
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
