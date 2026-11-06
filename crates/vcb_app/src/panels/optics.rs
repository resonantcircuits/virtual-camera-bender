//! The optics bend: the IR-cut filter slid off the sensor, and the scene's estimated infrared.
//!
//! The panel sets the preset's `[bend.optics]` section. The power switch seats or pulls
//! the filter; the knob says how far it has slid off, from the top, as a percentage of
//! the frame height, its shadow edge as soft as the camera's `[lens]
//! filter_edge_blur_frac` makes it. Below, under "Scene infrared, estimated", the
//! estimator of the scene's near infrared: an RGB photo records nothing beyond 700 nm,
//! so the infrared is guessed from each pixel's linear colour, a weighted sum of red,
//! green and blue scaled by an overall level, and raised on foliage only on the
//! explicit assumption that green means chlorophyll. Weights and factors are
//! dimensionless, shown as factors with two decimals. [`OpticsPanel`] keeps every value
//! while the filter is seated.

use eframe::egui::Ui;
use vcb_core::preset::OpticsBend;
use vcb_core::{Camera, Preset};

use crate::panels::{self, Edit, Knob, PanelId, Scale};

/// The filter knob: 0 to 100 % of the frame height slid off, from the top.
pub const PULLED: Knob = Knob::linear(0.0..=100.0, " % of height", 0).scaled(Scale::Times(100.0));

/// The overall infrared level knob: 0 to 4 times the visible level of a grey surface.
pub const LEVEL: Knob = Knob::linear(0.0..=4.0, " ×", 2);

/// The foliage knob: chlorophyll's infrared up to 8 times its visible green.
pub const FOLIAGE: Knob = Knob::linear(0.0..=8.0, " ×", 2);

/// The weight knobs: each channel's share of the infrared, 0 to 1.
pub const WEIGHT: Knob = Knob::linear(0.0..=1.0, " ×", 2);

/// The weight rows: label, channel index and tooltip.
const WEIGHTS: [(&str, usize, &str); 3] = [
    (
        "Red weight",
        0,
        "Share of the scene's linear red in its infrared: most surfaces reflect infrared about as they reflect red",
    ),
    (
        "Green weight",
        1,
        "Share of the scene's linear green in its infrared",
    ),
    (
        "Blue weight",
        2,
        "Share of the scene's linear blue in its infrared: low, so a clear sky, blue by Rayleigh scattering, stays dark",
    ),
];

/// The optics bend's switch and values.
///
/// The default holds the core's values, the filter fully off the sensor and the
/// infrared from the weighted sum alone, with the filter seated.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpticsPanel {
    /// Whether the filter is pulled: the bend is patched in.
    pub enabled: bool,
    /// The values, kept while the filter is seated.
    pub bend: OpticsBend,
}

impl OpticsPanel {
    /// The panel showing `preset`'s optics bend, or the default values with the filter seated.
    #[must_use]
    pub fn from_preset(preset: &Preset) -> Self {
        let mut panel = Self::default();
        panel.load(preset);
        panel
    }

    /// Sets the panel from `preset`; without an optics bend only the switch moves.
    pub fn load(&mut self, preset: &Preset) {
        match preset.bend.optics {
            Some(bend) => {
                self.enabled = true;
                self.bend = bend;
            }
            None => self.enabled = false,
        }
    }

    /// The preset's `[bend.optics]` section; `None` while the filter is seated.
    #[must_use]
    pub fn bend(&self) -> Option<OpticsBend> {
        self.enabled.then_some(self.bend)
    }

    /// Writes the panel into `preset`.
    pub fn apply(&self, preset: &mut Preset) {
        preset.bend.optics = self.bend();
    }

    /// Draws the panel for `camera`, whose lens sets the softness of the filter's edge.
    pub fn show(&mut self, ui: &mut Ui, camera: &Camera) -> Edit {
        let mut edit = Edit::default();
        edit.record(&panels::bend_header(
            ui,
            PanelId::Optics.title(),
            &mut self.enabled,
            "IR-cut filter",
            ["seated", "pulled"],
        ));
        ui.scope(|ui| {
            if !self.enabled {
                ui.multiply_opacity(panels::OFF_OPACITY);
            }
            let bend = &mut self.bend;
            let edge = camera.lens.filter_edge_blur_frac * 100.0;
            panels::grid(ui, "optics-filter", |ui| {
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Pulled",
                    "How far the hot mirror has slid off the sensor, from the top: 0 covers every row, 100 % none",
                    &mut bend.ir_cut_pulled_frac,
                    &PULLED,
                );
                panels::hint_row(
                    ui,
                    &format!(
                        "Its shadow edge spans {} % of the height",
                        panels::significant(edge, 2)
                    ),
                );
            });

            panels::section(ui, "Scene infrared, estimated");
            panels::hint(
                ui,
                "A photo records no infrared, so it is guessed from each pixel's colour.",
            );
            panels::grid(ui, "optics-nir", |ui| {
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Level",
                    "Infrared radiance relative to visible for a scene of equal energy: a grey surface reflects this times the weights' sum of its visible level",
                    &mut bend.nir_scale,
                    &LEVEL,
                );
                panels::knob_row(
                    ui,
                    &mut edit,
                    "Foliage",
                    "Infrared of chlorophyll relative to its visible green, the Wood effect, on the assumption that green means foliage: 0 treats foliage as any surface, 4 is a typical leaf",
                    &mut bend.foliage_nir,
                    &FOLIAGE,
                );
                panels::hint_row(
                    ui,
                    "An assumption: green means chlorophyll",
                );
                for (label, channel, hover) in WEIGHTS {
                    panels::knob_row(
                        ui,
                        &mut edit,
                        label,
                        hover,
                        &mut bend.nir_weights[channel],
                        &WEIGHT,
                    );
                }
            });
            panels::hint(
                ui,
                &format!(
                    "Infrared of grey {:.2} ×, of pure blue {:.2} ×, relative to visible.",
                    grey_nir(bend),
                    bend.nir_scale * bend.nir_weights[2],
                ),
            );
        });
        edit
    }
}

/// The estimated infrared of a grey surface relative to its visible level.
///
/// `nir_scale` times the sum of the weights; the foliage term is zero on grey.
#[must_use]
pub fn grey_nir(bend: &OpticsBend) -> f64 {
    bend.nir_scale * bend.nir_weights.iter().sum::<f64>()
}
