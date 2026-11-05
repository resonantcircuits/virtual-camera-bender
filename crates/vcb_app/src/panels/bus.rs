//! The bus bend drawn as a front panel, in the schematic's signal order.
//!
//! From left to right in the schematic and top to bottom here: the source DIP bank
//! (`D11` to `D2`), the variable high-pass (capacitor, potentiometer and the node it
//! loads, the patch resistor), the "Bypass / Function" and "Invert / Divide" switches,
//! and the target selector (`D11` to `D4` or ground) with the common-bus switch.
//! [`BusPanel`] holds the switch positions, including those the preset cannot store
//! (the "Invert / Divide" position while bypassed, every value while the bend is
//! switched off), and builds the preset's `[bend.bus]` section from them.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Align, CornerRadius, Grid, Layout, Rect, Response, Sense, Slider, SliderClamping, Stroke,
    StrokeKind, Ui, WidgetInfo,
};
use vcb_core::Preset;
use vcb_core::bus::{Function, PotNode, Target};
use vcb_core::preset::BusBend;

use crate::panels::{self, Edit};
use crate::theme;

/// Data lines on the source DIP bank, in panel order.
pub const SOURCE_PINS: RangeInclusive<u8> = 2..=11;

/// Data lines on the target selector, in panel order; ground follows them.
pub const TARGET_PINS: RangeInclusive<u8> = 4..=11;

/// Travel of the potentiometer in kilohms: the schematic's 1 kΩ part.
pub const POT_RANGE_KOHM: RangeInclusive<f64> = 0.0..=1.0;

/// Smallest non-zero pot setting the logarithmic slider reaches, in kilohms (1 Ω).
const POT_SMALLEST_KOHM: f64 = 0.001;

/// Capacitor values the slider spans, in microfarads.
pub const CAP_RANGE_UF: RangeInclusive<f64> = 0.001..=100.0;

/// Patch resistor values the slider spans, in ohms; 0 is a direct wire.
pub const SERIES_RANGE_OHM: RangeInclusive<f64> = 0.0..=100_000.0;

/// Width of one switch position on the DIP bank and the selector, in points.
const SLOT_WIDTH: f32 = 24.0;

/// Height of a DIP switch's slide, in points.
const SLIDE_HEIGHT: f32 = 32.0;

/// Gap between switch positions, in points.
const SLOT_GAP: f32 = 4.0;

/// Width of the sliders in the panel, in points.
const SLIDER_WIDTH: f32 = 136.0;

/// The "Bypass / Function" switch: whether node F reaches the target through a gate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Route {
    /// F wired straight to the target.
    #[default]
    Bypass,
    /// F through the gate the "Invert / Divide" switch picks.
    Function,
}

/// The "Invert / Divide" switch: which gate the function route uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Gate {
    /// The 74HC00 inverter.
    #[default]
    Invert,
    /// The 74HC74 divide-by-two flip-flop.
    Divide,
}

/// The front panel's switch positions and component values.
///
/// `sources` has bit `k` set when line `Dk` is switched into the source bank. `gate`
/// keeps its position while `route` bypasses the gates, as the hardware switch does.
#[derive(Debug, Clone, PartialEq)]
pub struct BusPanel {
    /// Whether the bend is patched in at all.
    pub enabled: bool,
    /// Source lines, bit `k` for `Dk`.
    pub sources: u16,
    /// The target selector's position.
    pub target: Target,
    /// The "Bypass / Function" switch.
    pub route: Route,
    /// The "Invert / Divide" switch.
    pub gate: Gate,
    /// Coupling capacitor in microfarads.
    pub cap_uf: f64,
    /// Potentiometer setting in kilohms.
    pub pot_kohm: f64,
    /// The node the potentiometer loads.
    pub pot_node: PotNode,
    /// Patch resistor in ohms.
    pub series_ohm: f64,
    /// Whether source and target share one bus wire.
    pub common_bus: bool,
}

impl Default for BusPanel {
    /// The wiring of the shipped high-pass preset, switched off: `D10` and `D9`
    /// through 1 µF and 50 Ω on the source bus, bypassed onto `D11`.
    fn default() -> Self {
        Self {
            enabled: false,
            sources: (1 << 10) | (1 << 9),
            target: Target::Pin(11),
            route: Route::Bypass,
            gate: Gate::Invert,
            cap_uf: 1.0,
            pot_kohm: 0.05,
            pot_node: PotNode::S,
            series_ohm: 0.0,
            common_bus: false,
        }
    }
}

impl BusPanel {
    /// The panel showing `preset`'s bus bend, or the default wiring switched off.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the switches from `preset`; without a bus bend only the power switch moves.
    ///
    /// Source pins outside the DIP bank cannot be shown and are left out.
    pub fn load(&mut self, preset: &Preset) {
        let Some(bus) = &preset.bend.bus else {
            self.enabled = false;
            return;
        };
        self.enabled = true;
        self.sources = bus
            .source_pins
            .iter()
            .filter(|pin| SOURCE_PINS.contains(pin))
            .fold(0, |mask, pin| mask | (1 << pin));
        self.target = bus.target;
        match bus.function {
            Function::Bypass => self.route = Route::Bypass,
            Function::Invert => (self.route, self.gate) = (Route::Function, Gate::Invert),
            Function::Divide => (self.route, self.gate) = (Route::Function, Gate::Divide),
        }
        self.cap_uf = bus.cap_uf;
        self.pot_kohm = bus.pot_kohm;
        self.pot_node = bus.pot_node;
        self.series_ohm = bus.series_ohm;
        self.common_bus = bus.common_bus;
    }

    /// The function the two switches select.
    #[must_use]
    pub fn function(&self) -> Function {
        match (self.route, self.gate) {
            (Route::Bypass, _) => Function::Bypass,
            (Route::Function, Gate::Invert) => Function::Invert,
            (Route::Function, Gate::Divide) => Function::Divide,
        }
    }

    /// The source lines switched on, highest first, as presets list them.
    #[must_use]
    pub fn source_pins(&self) -> Vec<u8> {
        SOURCE_PINS
            .rev()
            .filter(|pin| self.sources & (1 << pin) != 0)
            .collect()
    }

    /// Whether line `Dk` is switched into the source bank.
    #[must_use]
    pub fn source(&self, pin: u8) -> bool {
        SOURCE_PINS.contains(&pin) && self.sources & (1 << pin) != 0
    }

    /// Switches line `Dk` into or out of the source bank.
    ///
    /// # Panics
    ///
    /// Panics if `pin` is not on the DIP bank, `D2` to `D11`.
    pub fn set_source(&mut self, pin: u8, on: bool) {
        assert!(
            SOURCE_PINS.contains(&pin),
            "D{pin} is not on the source DIP bank (D2 to D11)"
        );
        if on {
            self.sources |= 1 << pin;
        } else {
            self.sources &= !(1 << pin);
        }
    }

    /// The preset's `[bend.bus]` section; `None` while the bend is switched off.
    #[must_use]
    pub fn bend(&self) -> Option<BusBend> {
        self.enabled.then(|| BusBend {
            source_pins: self.source_pins(),
            target: self.target,
            function: self.function(),
            cap_uf: self.cap_uf,
            pot_kohm: self.pot_kohm,
            pot_node: self.pot_node,
            series_ohm: self.series_ohm,
            common_bus: self.common_bus,
        })
    }

    /// Writes the panel into `preset`'s bus bend.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.bus = self.bend();
    }

    /// Draws the front panel and returns what the user did.
    pub fn show(&mut self, ui: &mut Ui) -> Edit {
        let mut edit = Edit::default();
        ui.horizontal(|ui| {
            panels::title(ui, "Bus bend");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                edit.record(&panels::power(
                    ui,
                    &mut self.enabled,
                    "Patch the bend into the bus",
                ));
            });
        });
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(0.45);
            }
            ui.spacing_mut().slider_width = SLIDER_WIDTH;
            edit |= self.show_source(ui);
            edit |= self.show_filter(ui);
            edit |= self.show_function(ui);
            edit |= self.show_target(ui);
        });
        edit
    }

    fn show_source(&mut self, ui: &mut Ui) -> Edit {
        let mut edit = Edit::default();
        panels::section(ui, "Source selector");
        edit.record(&dip_bank(ui, &mut self.sources));
        if self.enabled && self.sources == 0 {
            panels::hint(ui, "Switch at least one source line on.");
        }
        edit
    }

    fn show_filter(&mut self, ui: &mut Ui) -> Edit {
        let mut edit = Edit::default();
        panels::section(ui, "Variable high-pass");
        Grid::new("bus-filter")
            .num_columns(2)
            .spacing([theme::GRID, theme::GRID])
            .show(ui, |ui| {
                ui.label("Capacitor");
                edit.record(
                    &ui.add(
                        Slider::new(&mut self.cap_uf, CAP_RANGE_UF)
                            .logarithmic(true)
                            .clamping(SliderClamping::Edits)
                            .custom_formatter(|value, _| significant(value, 3))
                            .suffix(" µF"),
                    )
                    .on_hover_text("Coupling capacitor from the source bus S to node F"),
                );
                ui.end_row();

                ui.label("Pot");
                edit.record(
                    &ui.add(
                        Slider::new(&mut self.pot_kohm, POT_RANGE_KOHM)
                            .logarithmic(true)
                            .smallest_positive(POT_SMALLEST_KOHM)
                            .clamping(SliderClamping::Edits)
                            .fixed_decimals(3)
                            .suffix(" kΩ"),
                    )
                    .on_hover_text("The 1 kΩ potentiometer to ground; 0 grounds its node"),
                );
                ui.end_row();

                ui.label("Pot node");
                let mut on_s = self.pot_node == PotNode::S;
                let response = panels::switch(ui, &mut on_s, "F", "S").on_hover_text(
                    "F: after the capacitor, a high-pass. S: on the source bus, as the schematic draws it",
                );
                if response.changed() {
                    self.pot_node = if on_s { PotNode::S } else { PotNode::F };
                }
                edit.record(&response);
                ui.end_row();

                ui.label("Patch resistor");
                edit.record(
                    &ui.add(
                        Slider::new(&mut self.series_ohm, SERIES_RANGE_OHM)
                            .logarithmic(true)
                            .smallest_positive(1.0)
                            .clamping(SliderClamping::Edits)
                            .fixed_decimals(0)
                            .suffix(" Ω"),
                    )
                    .on_hover_text("Resistor between each tapped pin and its bus; 0 is a direct wire"),
                );
                ui.end_row();
            });
        edit
    }

    fn show_function(&mut self, ui: &mut Ui) -> Edit {
        let mut edit = Edit::default();
        panels::section(ui, "Function");
        let mut function = self.route == Route::Function;
        let route = panels::switch(ui, &mut function, "Bypass", "Function")
            .on_hover_text("Bypass wires F to the target; Function routes it through a gate");
        if route.changed() {
            self.route = if function {
                Route::Function
            } else {
                Route::Bypass
            };
        }
        edit.record(&route);
        ui.scope(|ui| {
            if self.route == Route::Bypass {
                ui.multiply_opacity(0.5);
            }
            let mut divide = self.gate == Gate::Divide;
            let gate = panels::switch(ui, &mut divide, "Invert", "Divide").on_hover_text(
                "Invert: a 74HC00 NAND with tied inputs. Divide: a 74HC74 toggle clocked by F",
            );
            if gate.changed() {
                self.gate = if divide { Gate::Divide } else { Gate::Invert };
            }
            edit.record(&gate);
        });
        edit
    }

    fn show_target(&mut self, ui: &mut Ui) -> Edit {
        let mut edit = Edit::default();
        panels::section(ui, "Target selector");
        edit.record(&selector(ui, &mut self.target));
        ui.horizontal(|ui| {
            ui.label("Common bus");
            edit.record(&panels::power(
                ui,
                &mut self.common_bus,
                "Source and target banks share one bus wire",
            ));
        });
        edit
    }
}

/// The source DIP bank: one slide switch per line, `D11` on the left.
fn dip_bank(ui: &mut Ui, sources: &mut u16) -> Response {
    let pins: Vec<u8> = SOURCE_PINS.rev().collect();
    let (housing, mut response) = bank_rect(ui, pins.len(), SLIDE_HEIGHT + theme::GRID);
    let painter = ui.painter();
    painter.rect_filled(housing, CornerRadius::same(4), theme::HOUSING);
    let mut toggled = false;
    for (i, pin) in pins.iter().enumerate() {
        let slot = slot_rect(housing, i);
        let id = response.id.with(pin);
        let slide = ui.interact(slot, id, Sense::click());
        if slide.clicked() {
            *sources ^= 1 << pin;
            toggled = true;
        }
        let on_now = *sources & (1 << pin) != 0;
        slide.widget_info(|| {
            WidgetInfo::selected(egui::Role::CheckBox, true, on_now, format!("D{pin}"))
        });
        let track = Rect::from_center_size(slot.center(), egui::vec2(12.0, SLIDE_HEIGHT));
        painter.rect(
            track,
            CornerRadius::same(2),
            theme::SLOT,
            Stroke::new(1.0, panels::outline(&slide)),
            StrokeKind::Inside,
        );
        let travel = ui.ctx().animate_bool_responsive(id, on_now);
        let knob_y = egui::lerp((track.bottom() - 7.0)..=(track.top() + 7.0), travel);
        let knob =
            Rect::from_center_size(egui::pos2(track.center().x, knob_y), egui::vec2(10.0, 12.0));
        let colour = if on_now {
            theme::ACCENT
        } else {
            theme::WIDGET_ACTIVE
        };
        painter.rect_filled(knob, CornerRadius::same(2), colour);
        panels::pin_label(
            ui,
            egui::pos2(slot.center().x, housing.bottom() + 2.0),
            &format!("D{pin}"),
            theme::label_colour(on_now),
        );
        response = response.union(slide);
    }
    painter.text(
        egui::pos2(housing.right() + 4.0, housing.top() + 4.0),
        egui::Align2::LEFT_TOP,
        "on",
        egui::TextStyle::Small.resolve(ui.style()),
        theme::TEXT_WEAK,
    );
    if toggled {
        response.mark_changed();
    }
    response
}

/// The target selector: one contact per line, `D11` on the left, then ground.
fn selector(ui: &mut Ui, target: &mut Target) -> Response {
    let positions: Vec<Target> = TARGET_PINS
        .rev()
        .map(Target::Pin)
        .chain(std::iter::once(Target::Gnd))
        .collect();
    let (housing, mut response) = bank_rect(ui, positions.len(), theme::GRID * 3.0);
    let painter = ui.painter();
    painter.rect_filled(housing, CornerRadius::same(4), theme::HOUSING);
    let mut moved = false;
    for (i, position) in positions.iter().enumerate() {
        let slot = slot_rect(housing, i);
        let name = match position {
            Target::Pin(pin) => format!("D{pin}"),
            Target::Gnd => "GND".to_owned(),
        };
        let contact = ui.interact(slot, response.id.with(i), Sense::click());
        if contact.clicked() && *target != *position {
            *target = *position;
            moved = true;
        }
        let selected = *target == *position;
        contact
            .widget_info(|| WidgetInfo::selected(egui::Role::RadioButton, true, selected, &name));
        let centre = slot.center();
        painter.circle_stroke(centre, 6.0, Stroke::new(1.5, panels::outline(&contact)));
        if selected {
            painter.circle_filled(centre, 4.0, theme::ACCENT);
        }
        panels::pin_label(
            ui,
            egui::pos2(centre.x, housing.bottom() + 2.0),
            &name,
            theme::label_colour(selected),
        );
        response = response.union(contact);
    }
    if moved {
        response.mark_changed();
    }
    response
}

/// Allocates a housing for `count` switch positions of `height`, with room for labels.
fn bank_rect(ui: &mut Ui, count: usize, height: f32) -> (Rect, Response) {
    let width = count as f32 * SLOT_WIDTH + (count - 1) as f32 * SLOT_GAP + 2.0 * SLOT_GAP;
    let label_height = theme::SMALL_SIZE + 4.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height + label_height), Sense::hover());
    let housing = Rect::from_min_size(rect.min, egui::vec2(width, height));
    (housing, response)
}

/// The rectangle of switch position `i` inside a housing.
fn slot_rect(housing: Rect, i: usize) -> Rect {
    let left = housing.left() + SLOT_GAP + i as f32 * (SLOT_WIDTH + SLOT_GAP);
    Rect::from_min_size(
        egui::pos2(left, housing.top()),
        egui::vec2(SLOT_WIDTH, housing.height()),
    )
}

/// Formats `value` with `digits` significant digits, without an exponent.
#[must_use]
pub fn significant(value: f64, digits: u8) -> String {
    if value == 0.0 || !value.is_finite() {
        return format!("{value}");
    }
    let magnitude = value.abs().log10().floor() as i32;
    let decimals = usize::try_from(i32::from(digits) - 1 - magnitude).unwrap_or(0);
    format!("{value:.decimals$}")
}
