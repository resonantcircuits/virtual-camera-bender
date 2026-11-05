//! The signal-path strip: the camera's stages in signal order, above the panels.
//!
//! Scene → Sensor → Readout → AFE → ADC → Bus → Processor → JPEG. A stage with a bend
//! patched in (the clocks on the readout, the AFE bend, the bus bend) or with
//! processor overrides is lit in the accent colour. Clicking a stage asks the
//! inspector to scroll to its panel; the ADC has no controls of its own and leads to
//! the bus bend on its output lines.

use eframe::egui::{self, Button, RichText, Ui};
use vcb_core::Preset;

use crate::panels::PanelId;
use crate::theme;

/// A stage of the signal chain, as the strip shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// The light in front of the lens.
    Scene,
    /// The photosites and their colour filters.
    Sensor,
    /// The vertical and horizontal transfer.
    Readout,
    /// The analog front end.
    Afe,
    /// The converter.
    Adc,
    /// The data bus between converter and processor.
    Bus,
    /// The image processor.
    Processor,
    /// The JPEG encoder.
    Jpeg,
}

impl Stage {
    /// Every stage, in signal order.
    pub const ALL: [Self; 8] = [
        Self::Scene,
        Self::Sensor,
        Self::Readout,
        Self::Afe,
        Self::Adc,
        Self::Bus,
        Self::Processor,
        Self::Jpeg,
    ];

    /// The stage's name on the strip.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Scene => "Scene",
            Self::Sensor => "Sensor",
            Self::Readout => "Readout",
            Self::Afe => "AFE",
            Self::Adc => "ADC",
            Self::Bus => "Bus",
            Self::Processor => "Processor",
            Self::Jpeg => "JPEG",
        }
    }

    /// The panel a click on the stage scrolls to.
    #[must_use]
    pub fn panel(self) -> PanelId {
        match self {
            Self::Scene => PanelId::Scene,
            Self::Sensor => PanelId::Sensor,
            Self::Readout => PanelId::Readout,
            Self::Afe => PanelId::Afe,
            Self::Adc | Self::Bus => PanelId::Bus,
            Self::Processor => PanelId::Processor,
            Self::Jpeg => PanelId::Jpeg,
        }
    }

    /// Whether `preset` bends or overrides the stage.
    #[must_use]
    pub fn is_bent(self, preset: &Preset) -> bool {
        match self {
            Self::Readout => preset.bend.clocks.is_some(),
            Self::Afe => preset.bend.afe.is_some(),
            Self::Bus => preset.bend.bus.is_some(),
            Self::Processor => preset.isp.is_some(),
            Self::Scene | Self::Sensor | Self::Adc | Self::Jpeg => false,
        }
    }
}

/// Draws the strip for `preset`; returns the panel to scroll to if a stage was clicked.
pub fn show(ui: &mut Ui, preset: &Preset) -> Option<PanelId> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme::GRID * 0.25;
        ui.spacing_mut().button_padding = egui::vec2(theme::GRID * 0.5, theme::GRID * 0.25);
        for (i, stage) in Stage::ALL.into_iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("→").small().color(theme::TEXT_WEAK));
            }
            let bent = stage.is_bent(preset);
            let text = RichText::new(stage.name()).small().color(if bent {
                theme::ACCENT
            } else {
                theme::TEXT
            });
            let mut button = Button::new(text).corner_radius(theme::GRID * 0.5);
            button = if bent {
                button
                    .fill(theme::ACCENT_FAINT)
                    .stroke(egui::Stroke::new(1.0, theme::ACCENT))
            } else {
                button.fill(theme::HOUSING)
            };
            let hover = if bent {
                format!("{}: bent or overridden; show its panel", stage.name())
            } else {
                format!("{}: show its panel", stage.name())
            };
            if ui.add(button).on_hover_text(hover).clicked() {
                clicked = Some(stage.panel());
            }
        }
    });
    clicked
}
