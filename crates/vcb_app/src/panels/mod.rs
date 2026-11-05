//! The inspector's panels, one per stage of the signal chain, and their shared widgets.
//!
//! The panels follow the signal: [`scene`], [`sensor`], [`readout`], [`clocks`],
//! [`afe`], [`bus`] and [`isp`] (the processor and its JPEG encoder), with the
//! [`strip`] above them as a map. Each panel edits the preset and reports an
//! [`Edit`]: whether a value changed, whether a control is being dragged, and whether
//! a drag just ended. The app turns those into preview and full renders.
//!
//! Panels of bends ([`clocks`], [`afe`], [`bus`]) hold their switch positions and
//! values even while the bend is switched off, which the preset cannot store, and
//! write their section of the preset with `apply`; [`Panels`] holds the three. The
//! other panels edit the preset directly. Every control shows a physical value with
//! its unit; [`Knob`] describes how a stored value is shown.

pub mod afe;
pub mod bus;
pub mod clocks;
pub mod isp;
pub mod readout;
pub mod scene;
pub mod sensor;
pub mod strip;

use std::ops::{BitOrAssign, RangeInclusive};

use eframe::egui::{
    self, Align, Align2, Button, Color32, CornerRadius, Layout, Response, RichText, Sense, Slider,
    SliderClamping, Stroke, StrokeKind, TextStyle, Ui, WidgetInfo,
};
use vcb_core::Preset;

use crate::theme;

/// Width of the label column of every panel, in points, so the controls line up.
pub const LABEL_WIDTH: f32 = theme::GRID * 14.0;

/// Width of the sliders in the panels, in points.
pub const SLIDER_WIDTH: f32 = theme::GRID * 17.0;

/// Smallest width of a slider's numeric field, in points, so most fields line up.
const VALUE_WIDTH: f32 = theme::GRID * 10.0;

/// Opacity of a control that shows the camera's own value.
const DEFAULT_OPACITY: f32 = 0.5;

/// Opacity of the controls of a bend that is switched off.
pub const OFF_OPACITY: f32 = 0.45;

/// What the user did to a panel during one UI pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about one UI pass, any combination of which can be true"
)]
pub struct Edit {
    /// A value changed.
    pub changed: bool,
    /// A control is being dragged.
    pub dragging: bool,
    /// A drag ended.
    pub released: bool,
    /// The user asked to power-cycle the camera.
    pub power_cycle: bool,
}

impl Edit {
    /// Records what a widget's response reports.
    pub fn record(&mut self, response: &Response) {
        self.changed |= response.changed();
        self.dragging |= response.dragged();
        self.released |= response.drag_stopped();
    }
}

impl BitOrAssign for Edit {
    fn bitor_assign(&mut self, rhs: Self) {
        self.changed |= rhs.changed;
        self.dragging |= rhs.dragging;
        self.released |= rhs.released;
        self.power_cycle |= rhs.power_cycle;
    }
}

/// The inspector's panels, the strip's stages point at them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelId {
    /// Scene input.
    Scene,
    /// Sensor.
    Sensor,
    /// Readout mode and resolution.
    Readout,
    /// The clock bend.
    Clocks,
    /// The analog front end bend.
    Afe,
    /// The bus bend.
    Bus,
    /// The processor overrides.
    Processor,
    /// The JPEG encoder, at the end of the processor panel.
    Jpeg,
}

/// The panels that hold state the preset cannot: the three bends.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Panels {
    /// The clock bend.
    pub clocks: clocks::ClocksPanel,
    /// The analog front end bend.
    pub afe: afe::AfePanel,
    /// The bus bend.
    pub bus: bus::BusPanel,
}

impl Panels {
    /// The panels showing `preset`'s bends; bends it lacks keep their default settings, off.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panels = Self::default();
        panels.load(preset);
        panels
    }

    /// Sets every bend panel from `preset`.
    pub fn load(&mut self, preset: &Preset) {
        self.clocks.load(preset);
        self.afe.load(preset);
        self.bus.load(preset);
    }

    /// Writes every bend panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        self.clocks.apply(preset);
        self.afe.apply(preset);
        self.bus.apply(preset);
    }
}

/// How a control shows a stored value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scale {
    /// As stored.
    Plain,
    /// The stored value times a factor: 100 for percent, the frame width for pixels.
    Times(f64),
    /// A transfer efficiency shown as the loss per transfer in parts per million.
    LossPpm,
}

impl Scale {
    /// The shown value of `stored`.
    #[must_use]
    pub fn show(self, stored: f64) -> f64 {
        match self {
            Self::Plain => stored,
            Self::Times(factor) => stored * factor,
            Self::LossPpm => (1.0 - stored) * 1e6,
        }
    }

    /// The stored value of `shown`.
    #[must_use]
    pub fn store(self, shown: f64) -> f64 {
        match self {
            Self::Plain => shown,
            Self::Times(factor) => shown / factor,
            Self::LossPpm => 1.0 - shown * 1e-6,
        }
    }
}

/// A slider's range in shown units, its scale, unit and precision.
#[derive(Debug, Clone, PartialEq)]
pub struct Knob {
    /// The slider's range, in shown units.
    pub range: RangeInclusive<f64>,
    /// Whether the slider is logarithmic.
    pub log: bool,
    /// The smallest positive value of a logarithmic slider whose range starts at zero.
    pub smallest: f64,
    /// How the stored value is shown.
    pub scale: Scale,
    /// The unit after the number, with its leading space.
    pub unit: &'static str,
    /// Decimals shown; `None` shows three significant digits.
    pub decimals: Option<usize>,
}

impl Knob {
    /// A linear knob over `range` in `unit`, shown with `decimals` decimals.
    #[must_use]
    pub const fn linear(range: RangeInclusive<f64>, unit: &'static str, decimals: usize) -> Self {
        Self {
            range,
            log: false,
            smallest: 0.0,
            scale: Scale::Plain,
            unit,
            decimals: Some(decimals),
        }
    }

    /// A logarithmic knob over `range` in `unit`, shown with three significant digits.
    ///
    /// A range that starts at zero reaches down to `smallest`, then zero.
    #[must_use]
    pub const fn log(range: RangeInclusive<f64>, smallest: f64, unit: &'static str) -> Self {
        Self {
            range,
            log: true,
            smallest,
            scale: Scale::Plain,
            unit,
            decimals: None,
        }
    }

    /// The same knob showing the stored value through `scale`.
    #[must_use]
    pub const fn scaled(mut self, scale: Scale) -> Self {
        self.scale = scale;
        self
    }

    /// The same knob with `decimals` decimals.
    #[must_use]
    pub const fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = Some(decimals);
        self
    }
}

/// A slider with a numeric field over `value`, shown through `knob`.
///
/// The stored value changes only when the user moves the slider or types a number,
/// so a value loaded from a preset survives the round trip through the display scale.
pub fn knob(ui: &mut Ui, value: &mut f64, knob: &Knob) -> Response {
    let scale = knob.scale;
    let mut slider = Slider::from_get_set(knob.range.clone(), move |shown: Option<f64>| {
        if let Some(shown) = shown {
            *value = scale.store(shown);
        }
        scale.show(*value)
    })
    .logarithmic(knob.log)
    .clamping(SliderClamping::Edits)
    .suffix(knob.unit);
    if knob.log && *knob.range.start() <= 0.0 {
        slider = slider.smallest_positive(knob.smallest);
    }
    slider = match knob.decimals {
        Some(decimals) => slider.fixed_decimals(decimals),
        None => slider.custom_formatter(|shown, _| significant(shown, 3)),
    };
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.x = VALUE_WIDTH;
        ui.add(slider)
    })
    .inner
}

/// The label cell of a panel row, [`LABEL_WIDTH`] wide.
pub fn row_label(ui: &mut Ui, text: &str) -> Response {
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(LABEL_WIDTH, height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(LABEL_WIDTH);
            ui.label(text)
        },
    )
    .inner
}

/// A grid of panel rows: label, control, and a column for reset buttons.
pub fn grid<R>(ui: &mut Ui, id: &str, rows: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Grid::new(id)
        .num_columns(3)
        .spacing([theme::GRID, theme::GRID * 0.75])
        .show(ui, rows)
        .inner
}

/// A grid row with a label and a knob over `value`.
pub fn knob_row(
    ui: &mut Ui,
    edit: &mut Edit,
    label: &str,
    hover: &str,
    value: &mut f64,
    spec: &Knob,
) {
    row_label(ui, label).on_hover_text(hover);
    edit.record(&knob(ui, value, spec).on_hover_text(hover));
    ui.label("");
    ui.end_row();
}

/// A grid row whose value may be the camera's own, as `None`.
///
/// The camera's value `default` is shown dimmed; moving the knob sets the value, and
/// the reset button returns it to `None`.
pub fn override_row(
    ui: &mut Ui,
    edit: &mut Edit,
    label: &str,
    hover: &str,
    value: &mut Option<f64>,
    default: f64,
    spec: &Knob,
) {
    row_label(ui, label).on_hover_text(hover);
    let touched = value.is_some();
    let mut shown = value.unwrap_or(default);
    let response = ui
        .scope(|ui| {
            if !touched {
                ui.multiply_opacity(DEFAULT_OPACITY);
            }
            knob(ui, &mut shown, spec)
        })
        .inner
        .on_hover_text(hover);
    if response.changed() {
        *value = Some(shown);
    }
    edit.record(&response);
    if reset_button(ui, touched).clicked() {
        *value = None;
        edit.changed = true;
    }
    ui.end_row();
}

/// A grid row with a choice whose value may be the camera's own, as `None`.
pub fn override_choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    edit: &mut Edit,
    label: &str,
    value: &mut Option<T>,
    default: T,
    options: &[(T, &str)],
) {
    row_label(ui, label);
    let touched = value.is_some();
    let mut shown = value.unwrap_or(default);
    let response = ui
        .scope(|ui| {
            if !touched {
                ui.multiply_opacity(DEFAULT_OPACITY);
            }
            choice(ui, &mut shown, options)
        })
        .inner;
    if response.changed() {
        *value = Some(shown);
    }
    edit.record(&response);
    if reset_button(ui, touched).clicked() {
        *value = None;
        edit.changed = true;
    }
    ui.end_row();
}

/// A small button that returns a value to the camera's own; disabled while it is.
pub fn reset_button(ui: &mut Ui, enabled: bool) -> Response {
    ui.add_enabled(enabled, Button::new("↺").small())
        .on_hover_text("Back to the camera's value")
        .on_disabled_hover_text("The camera's own value; move the control to override it")
}

/// A read-only grid row: a label and a value.
pub fn value_row(ui: &mut Ui, label: &str, value: &str, hover: &str) {
    row_label(ui, label).on_hover_text(hover);
    ui.label(RichText::new(value).color(theme::TEXT_WEAK))
        .on_hover_text(hover);
    ui.label("");
    ui.end_row();
}

/// A grid row with a hint in the control column.
pub fn hint_row(ui: &mut Ui, text: &str) {
    ui.label("");
    hint(ui, text);
    ui.label("");
    ui.end_row();
}

/// A panel title with an optional power switch on the right; returns the switch's response.
pub fn header(
    ui: &mut Ui,
    text: &str,
    power_switch: Option<(&mut bool, &str)>,
) -> Option<Response> {
    ui.horizontal(|ui| {
        title(ui, text);
        power_switch.map(|(on, label)| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                power(ui, on, label)
            })
            .inner
        })
    })
    .inner
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

/// A panel title.
pub fn title(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .text_style(TextStyle::Heading)
            .color(theme::TEXT),
    );
}

/// Space above a subsection label, in points.
pub const SECTION_GAP: f32 = theme::GRID * 0.5;

/// A subsection label inside a panel, named after the part of the circuit.
pub fn section(ui: &mut Ui, text: &str) {
    ui.add_space(SECTION_GAP);
    section_label(ui, text);
}

/// The text of a subsection label, without the space above it.
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).strong().color(theme::TEXT));
}

/// A weak explanatory line.
pub fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(theme::TEXT_WEAK));
}

/// A two-position slide switch between `left` (false) and `right` (true).
///
/// The label of the selected position is bright and the other weak, so the switch
/// reads like the front panel's engraving.
pub fn switch(ui: &mut Ui, value: &mut bool, left: &str, right: &str) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let left_text =
        ui.painter()
            .layout_no_wrap(left.to_owned(), font.clone(), theme::label_colour(!*value));
    let right_text =
        ui.painter()
            .layout_no_wrap(right.to_owned(), font, theme::label_colour(*value));
    let track = egui::vec2(theme::GRID * 4.0, theme::GRID * 2.0);
    let gap = theme::GRID;
    let height = track.y.max(left_text.size().y);
    let size = egui::vec2(
        left_text.size().x + gap + track.x + gap + right_text.size().x,
        height,
    );
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    let selected = *value;
    response.widget_info(|| {
        WidgetInfo::selected(
            egui::Role::Switch,
            true,
            selected,
            format!("{left} or {right}"),
        )
    });
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let position = ui.ctx().animate_bool_responsive(response.id, selected);
        let left_pos = egui::pos2(rect.left(), rect.center().y - left_text.size().y / 2.0);
        let left_width = left_text.size().x;
        painter.galley(left_pos, left_text, theme::TEXT);
        let track_rect = egui::Rect::from_min_size(
            egui::pos2(
                rect.left() + left_width + gap,
                rect.center().y - track.y / 2.0,
            ),
            track,
        );
        let visuals = ui.style().interact(&response);
        painter.rect(
            track_rect,
            CornerRadius::same((track.y / 2.0) as u8),
            theme::SLOT,
            Stroke::new(1.0, outline(&response)),
            StrokeKind::Inside,
        );
        let radius = track.y / 2.0 - 2.0;
        let knob_x = egui::lerp(
            (track_rect.left() + radius + 2.0)..=(track_rect.right() - radius - 2.0),
            position,
        );
        painter.circle_filled(
            egui::pos2(knob_x, track_rect.center().y),
            radius,
            visuals.fg_stroke.color,
        );
        let right_pos = egui::pos2(
            track_rect.right() + gap,
            rect.center().y - right_text.size().y / 2.0,
        );
        painter.galley(right_pos, right_text, theme::TEXT);
    }
    response
}

/// An on-off switch whose track lights in the accent colour when on.
pub fn power(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let size = egui::vec2(theme::GRID * 4.0, theme::GRID * 2.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let state = *on;
    response.widget_info(|| WidgetInfo::selected(egui::Role::Switch, true, state, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let position = ui.ctx().animate_bool_responsive(response.id, state);
        let fill = theme::SLOT.lerp_to_gamma(theme::ACCENT_FAINT, position);
        let visuals = ui.style().interact(&response);
        let stroke = if state {
            theme::ACCENT
        } else {
            outline(&response)
        };
        painter.rect(
            rect,
            CornerRadius::same((size.y / 2.0) as u8),
            fill,
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
        let radius = size.y / 2.0 - 2.0;
        let knob_x = egui::lerp(
            (rect.left() + radius + 2.0)..=(rect.right() - radius - 2.0),
            position,
        );
        let knob = if state {
            theme::ACCENT
        } else {
            visuals.fg_stroke.color
        };
        painter.circle_filled(egui::pos2(knob_x, rect.center().y), radius, knob);
    }
    response.on_hover_text(label)
}

/// A row of mutually exclusive choices; returns the union of the buttons' responses.
pub fn choice<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::GRID * 0.5;
        let mut response: Option<Response> = None;
        for &(option, label) in options {
            let button = ui.selectable_value(value, option, label);
            response = Some(match response {
                Some(previous) => previous | button,
                None => button,
            });
        }
        response.unwrap_or_else(|| ui.label(""))
    })
    .inner
}

/// A small label centred at `pos`, for pin names under the switches.
pub(crate) fn pin_label(ui: &Ui, pos: egui::Pos2, text: &str, colour: Color32) {
    ui.painter().text(
        pos,
        Align2::CENTER_TOP,
        text,
        TextStyle::Small.resolve(ui.style()),
        colour,
    );
}

/// The outline of a switch's track: brighter under the pointer.
pub(crate) fn outline(response: &Response) -> Color32 {
    if response.hovered() {
        theme::TEXT_WEAK
    } else {
        theme::LINE
    }
}
