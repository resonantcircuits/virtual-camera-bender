//! The app's look: neutral dark greys, one warm amber accent and an 8-pixel grid.
//!
//! Every colour the app paints comes from this module. Greys carry no tint, the amber
//! accent marks what is switched on or selected, and a muted red marks errors. Text
//! uses egui's proportional font in three sizes (panel titles, body, small labels);
//! numbers appear with their unit after them.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Theme, Visuals,
};

/// The base spacing unit in points; margins and gaps are multiples of it.
pub const GRID: f32 = 8.0;

/// The accent: a warm amber for switched-on and selected controls.
pub const ACCENT: Color32 = Color32::from_rgb(0xE0, 0xA4, 0x58);

/// The accent at low opacity, behind selected text and slider fills.
pub const ACCENT_FAINT: Color32 = Color32::from_rgba_premultiplied(0x5A, 0x42, 0x23, 0x66);

/// Error messages: a muted red that stays readable on the dark greys.
pub const ERROR: Color32 = Color32::from_rgb(0xD8, 0x76, 0x66);

/// Behind the image in the viewport.
pub const CANVAS: Color32 = Color32::from_gray(0x16);

/// Panels: inspector, top bar and status line.
pub const PANEL: Color32 = Color32::from_gray(0x1F);

/// Front-panel housings such as the DIP bank and the target selector.
pub const HOUSING: Color32 = Color32::from_gray(0x2A);

/// A switch's track or slot when off.
pub const SLOT: Color32 = Color32::from_gray(0x14);

/// A widget's resting fill.
pub const WIDGET: Color32 = Color32::from_gray(0x30);

/// A widget's fill under the pointer.
pub const WIDGET_HOVER: Color32 = Color32::from_gray(0x3A);

/// A widget's fill while pressed.
pub const WIDGET_ACTIVE: Color32 = Color32::from_gray(0x45);

/// Thin outlines and separators.
pub const LINE: Color32 = Color32::from_gray(0x3C);

/// Body text.
pub const TEXT: Color32 = Color32::from_gray(0xD8);

/// Secondary text: labels, units, hints.
pub const TEXT_WEAK: Color32 = Color32::from_gray(0x8E);

/// Font size of panel titles in points.
pub const TITLE_SIZE: f32 = 16.0;

/// Font size of body text and controls in points.
pub const BODY_SIZE: f32 = 13.0;

/// Font size of small labels (pin names, units) in points.
pub const SMALL_SIZE: f32 = 11.0;

/// Corner radius of widgets in points.
const RADIUS: u8 = 4;

/// The bundled monospace font, a fallback of the proportional one for symbols.
///
/// egui's proportional font lacks arrows and the transport symbols (→ ↺ ▶ ■ ◁ ▷);
/// its bundled monospace font has them, so text in the proportional font falls back
/// to it for those glyphs only.
const SYMBOL_FALLBACK: &str = "Hack";

/// Applies the theme to every style of the context and selects dark mode.
pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional)
        && !family.iter().any(|name| name == SYMBOL_FALLBACK)
    {
        family.push(SYMBOL_FALLBACK.to_owned());
    }
    ctx.set_fonts(fonts);
    ctx.set_theme(Theme::Dark);
    ctx.style_mut_of(Theme::Dark, |style| {
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(TITLE_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(BODY_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(BODY_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(SMALL_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
            ),
        ]
        .into();
        let spacing = &mut style.spacing;
        spacing.item_spacing = egui::vec2(GRID, GRID * 0.75);
        spacing.button_padding = egui::vec2(GRID, GRID * 0.5);
        spacing.window_margin = Margin::same(GRID as i8);
        spacing.menu_margin = Margin::same(GRID as i8);
        spacing.indent = GRID * 2.0;
        spacing.interact_size = egui::vec2(GRID * 5.0, GRID * 3.0);
        spacing.slider_width = GRID * 20.0;
        spacing.combo_width = GRID * 16.0;
        style.visuals = visuals();
    });
}

/// The dark visuals with the app's greys and accent.
fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    let radius = CornerRadius::same(RADIUS);
    visuals.override_text_color = None;
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.window_stroke = Stroke::new(1.0, LINE);
    visuals.window_corner_radius = CornerRadius::same(RADIUS * 2);
    visuals.menu_corner_radius = radius;
    visuals.extreme_bg_color = SLOT;
    visuals.faint_bg_color = HOUSING;
    visuals.code_bg_color = HOUSING;
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = ACCENT;
    visuals.error_fg_color = ERROR;
    visuals.selection.bg_fill = ACCENT_FAINT;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.slider_trailing_fill = true;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = PANEL;
    widgets.noninteractive.weak_bg_fill = PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.noninteractive.corner_radius = radius;
    for (state, fill) in [
        (&mut widgets.inactive, WIDGET),
        (&mut widgets.hovered, WIDGET_HOVER),
        (&mut widgets.active, WIDGET_ACTIVE),
        (&mut widgets.open, WIDGET_HOVER),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    widgets.inactive.bg_stroke = Stroke::NONE;
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_gray(0x55));
    widgets.hovered.fg_stroke = Stroke::new(1.5, Color32::from_gray(0xEE));
    widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    widgets.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
    widgets.open.bg_stroke = Stroke::new(1.0, LINE);
    widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    visuals
}

/// The colour of a control's label, weak when the control is off.
#[must_use]
pub fn label_colour(on: bool) -> Color32 {
    if on { TEXT } else { TEXT_WEAK }
}
