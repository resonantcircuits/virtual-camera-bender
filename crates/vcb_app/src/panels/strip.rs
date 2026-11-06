//! The signal-path strip: the camera's stages in signal order, above the panels.
//!
//! Scene → Sensor → Readout → AFE → ADC → Bus → Clock → Processor → Memory → JPEG. A
//! stage with a bend patched in (the IR-cut filter pulled off the sensor, the clocks
//! on the readout, the AFE bend, the bus bend, the master-clock reclock, the memory's
//! address faults) or with processor overrides is lit in the accent colour. The
//! supply rail has no stage of its own: it feeds the AFE, the bus and the processor,
//! so a rail bend lights those three.
//!
//! Clicking a stage asks the inspector to scroll to the panel of what bends it: a lit
//! scene to the optics, a lit readout to the clocks, a stage lit by the rail alone to
//! the power panel. An unlit stage leads to its own panel; the ADC has no controls of
//! its own and leads to the bus bend on its output lines.

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
    /// The processor's capture of the bus on its own clock.
    Clock,
    /// The image processor.
    Processor,
    /// The frame-buffer SDRAM between the black clamp and development.
    Memory,
    /// The JPEG encoder.
    Jpeg,
}

impl Stage {
    /// Every stage, in signal order.
    pub const ALL: [Self; 10] = [
        Self::Scene,
        Self::Sensor,
        Self::Readout,
        Self::Afe,
        Self::Adc,
        Self::Bus,
        Self::Clock,
        Self::Processor,
        Self::Memory,
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
            Self::Clock => "Clock",
            Self::Processor => "Processor",
            Self::Memory => "Memory",
            Self::Jpeg => "JPEG",
        }
    }

    /// The stage's own panel, where a click leads while nothing bends the stage.
    #[must_use]
    pub fn home(self) -> PanelId {
        match self {
            Self::Scene => PanelId::Scene,
            Self::Sensor => PanelId::Sensor,
            Self::Readout => PanelId::Readout,
            Self::Afe => PanelId::Afe,
            Self::Adc | Self::Bus => PanelId::Bus,
            Self::Clock => PanelId::Clock,
            Self::Processor => PanelId::Processor,
            Self::Memory => PanelId::Memory,
            Self::Jpeg => PanelId::Jpeg,
        }
    }

    /// The panel of what bends or overrides the stage in `preset`, if anything does.
    ///
    /// The stage's own bend comes first; a rail bend (`[bend.rail]`) bends the AFE, the
    /// bus and the processor, which the rail feeds, and leads to the Power panel.
    #[must_use]
    pub fn bent_by(self, preset: &Preset) -> Option<PanelId> {
        let bend = &preset.bend;
        let rail = bend.rail.is_some().then_some(PanelId::Power);
        match self {
            Self::Scene => bend.optics.is_some().then_some(PanelId::Optics),
            Self::Readout => bend.clocks.is_some().then_some(PanelId::Clocks),
            Self::Afe => bend.afe.is_some().then_some(PanelId::Afe).or(rail),
            Self::Bus => bend.bus.is_some().then_some(PanelId::Bus).or(rail),
            Self::Clock => bend.clock.is_some().then_some(PanelId::Clock),
            Self::Processor => preset.isp.is_some().then_some(PanelId::Processor).or(rail),
            Self::Memory => bend.memory.is_some().then_some(PanelId::Memory),
            Self::Sensor | Self::Adc | Self::Jpeg => None,
        }
    }

    /// Whether `preset` bends or overrides the stage.
    #[must_use]
    pub fn is_bent(self, preset: &Preset) -> bool {
        self.bent_by(preset).is_some()
    }

    /// The panel a click on the stage scrolls to: what bends it, else its own.
    #[must_use]
    pub fn panel(self, preset: &Preset) -> PanelId {
        self.bent_by(preset).unwrap_or_else(|| self.home())
    }

    /// The stage's tooltip: whether it is bent and where a click leads.
    #[must_use]
    pub fn hover(self, preset: &Preset) -> String {
        match self.bent_by(preset) {
            Some(panel) if panel == self.home() => {
                format!("{}: bent or overridden; show its panel", self.name())
            }
            Some(panel) => format!(
                "{}: bent through the {} panel; show it",
                self.name(),
                panel.title()
            ),
            None => format!("{}: show its panel", self.name()),
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
            if ui.add(button).on_hover_text(stage.hover(preset)).clicked() {
                clicked = Some(stage.panel(preset));
            }
        }
    });
    clicked
}
