//! The scene panel: how bright the photographed scene was beyond the input's white.
//!
//! An 8-bit photo or video frame was tone-mapped by the camera that recorded it, its
//! highlights rolled off and clipped. The panel sets the preset's `[scene]` section,
//! which inverts that roll-off: the shoulder's start as a fraction of white, and the
//! headroom, the brightness of the input's clipped white in multiples of reference
//! white.

use eframe::egui::Ui;
use vcb_core::preset::SceneInput;

use crate::panels::{self, Edit, Knob, Scale};

/// The headroom knob: 1 to 16 times reference white, logarithmic.
///
/// 16 is four stops above white, as bright as a sunlit cloud against a mid-grey
/// exposure; the preset format accepts more.
pub const HEADROOM: Knob = Knob::log(1.0..=16.0, 1.0, " ×").decimals(2);

/// The shoulder knob: where the source camera's roll-off began, 1 to 99 % of white.
pub const SHOULDER: Knob = Knob::linear(1.0..=99.0, " % of white", 0).scaled(Scale::Times(100.0));

/// Draws the scene controls for `scene`.
pub fn show(ui: &mut Ui, scene: &mut SceneInput) -> Edit {
    let mut edit = Edit::default();
    panels::header(ui, "Scene", None);
    panels::grid(ui, "scene", |ui| {
        panels::knob_row(
            ui,
            &mut edit,
            "Headroom",
            "Brightness of the input's clipped white, in multiples of reference white; 1 takes the input as it is",
            &mut scene.headroom,
            &HEADROOM,
        );
        panels::knob_row(
            ui,
            &mut edit,
            "Shoulder start",
            "Input level, linear, at which the source camera began to roll off its highlights",
            &mut scene.shoulder_start,
            &SHOULDER,
        );
    });
    panels::hint(
        ui,
        &format!(
            "Clipped white reaches the lens {:.2} stops above reference white.",
            scene.headroom.max(1.0).log2()
        ),
    );
    edit
}
