//! The inspector's panels, one per stage of the signal chain, and their shared widgets.
//!
//! Each panel edits the preset and reports an [`Edit`]: whether a value changed,
//! whether a control is being dragged, and whether a drag just ended. The app turns
//! those into preview and full renders.

pub mod bus;
pub mod readout;

use std::ops::BitOrAssign;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Response, RichText, Sense, Stroke, StrokeKind, TextStyle,
    Ui, WidgetInfo,
};

use crate::theme;

/// What the user did to a panel during one UI pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Edit {
    /// A value changed.
    pub changed: bool,
    /// A control is being dragged.
    pub dragging: bool,
    /// A drag ended.
    pub released: bool,
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
    }
}

/// A panel title.
pub fn title(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .text_style(TextStyle::Heading)
            .color(theme::TEXT),
    );
}

/// A subsection label inside a panel, named after the part of the circuit.
pub fn section(ui: &mut Ui, text: &str) {
    ui.add_space(theme::GRID * 0.5);
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
