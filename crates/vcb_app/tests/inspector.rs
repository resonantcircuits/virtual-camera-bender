//! The inspector draws the panels of the four bends of milestone 3 in a headless ui
//! pass, and a click on a lit stage of the strip scrolls their panel into view.
//!
//! The passes run the app's real `ui` on an `egui::Context` without a renderer; the
//! painted text shapes tell what is on screen and where.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::App as _;
use eframe::egui::{
    self, DroppedFile, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape,
    ViewportCommand,
};
use vcb_app::app::{App, Setup};
use vcb_app::panels::Panels;
use vcb_app::panels::memory::MemoryPanel;
use vcb_app::panels::optics::OpticsPanel;
use vcb_app::panels::rail::RailPanel;
use vcb_app::panels::reclock::ReclockPanel;
use vcb_app::presets;
use vcb_core::Frame;
use vcb_core::preset::FaultKind;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// The scratch folder of this test binary.
fn scratch() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vcb_app_inspector");
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

/// A small photo on disk.
fn photo() -> PathBuf {
    let path = scratch().join("flat.png");
    let (width, height) = (64, 48);
    vcb_io::write_frame(
        &path,
        &Frame::from_rgb(width, height, vec![110; (width * height * 3) as usize]),
        vcb_io::Format::Png,
    )
    .expect("the photo is written");
    path
}

/// The shipped high-pass preset with the four new bends on, saved as `every-bend.toml`.
///
/// The AFE and the processor have no bend of their own, so the rail alone lights them.
fn every_bend() -> PathBuf {
    let mut preset = presets::load_preset(&root().join("presets/a520-hpf.toml")).expect("shipped");
    let mut panels = Panels::from_preset(&preset);
    panels.optics = OpticsPanel {
        enabled: true,
        ..OpticsPanel::default()
    };
    panels.reclock = ReclockPanel {
        enabled: true,
        ..ReclockPanel::default()
    };
    panels.reclock.bend.osc_mhz = 26.7;
    panels.memory = MemoryPanel {
        enabled: true,
        ..MemoryPanel::default()
    };
    assert!(panels.memory.set_kind(4, Some(FaultKind::Bridge), 12));
    assert!(panels.memory.set_marginal(4, true));
    panels.rail = RailPanel {
        enabled: true,
        ..RailPanel::default()
    };
    panels.apply(&mut preset);
    let path = scratch().join("every-bend.toml");
    std::fs::write(&path, presets::to_toml(&preset).expect("serialises")).expect("written");
    path
}

/// A file dropped on the window.
#[derive(Debug)]
struct Dropped(PathBuf);

impl DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|error| error.to_string())
    }
}

/// The app and the context it draws into, as the window would hold them.
struct Window {
    ctx: egui::Context,
    app: App,
    frame: eframe::Frame,
    time: f64,
}

/// Text painted in one pass: the text, where it starts, and whether it is visible.
#[derive(Debug, Clone)]
struct Painted {
    text: String,
    pos: Pos2,
    visible: bool,
}

impl Window {
    fn new() -> Self {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let app = App::with_setup(
            &cc,
            Setup {
                path: Some(photo()),
                camera_dir: root().join("cameras"),
                screenshot: None,
            },
        );
        Self {
            ctx,
            app,
            frame: eframe::Frame::_new_kittest(),
            time: 0.0,
        }
    }

    /// Runs one logic and ui pass with `events` and `dropped` files; returns what it painted.
    fn pass(&mut self, events: Vec<Event>, dropped: Vec<PathBuf>) -> (Vec<Painted>, Vec<String>) {
        self.time += 0.1;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            time: Some(self.time),
            events,
            dropped_files: dropped
                .into_iter()
                .map(|path| Arc::new(Dropped(path)) as egui::DroppedFileHandle)
                .collect(),
            ..RawInput::default()
        };
        let (app, frame) = (&mut self.app, &mut self.frame);
        // As eframe's native pass does: the logic, then the ui, in one pass.
        let mut output = self.ctx.run_ui(input, |ui| {
            app.logic(ui.ctx(), frame);
            app.ui(ui, frame);
        });
        output.textures_delta.clear();
        let mut painted = Vec::new();
        for clipped in &output.shapes {
            collect(&clipped.shape, clipped.clip_rect, &mut painted);
        }
        let titles = output
            .viewport_output
            .values()
            .flat_map(|viewport| &viewport.commands)
            .filter_map(|command| match command {
                ViewportCommand::Title(title) => Some(title.clone()),
                _ => None,
            })
            .collect();
        (painted, titles)
    }

    /// Runs passes until the scroll animation has settled; returns what the last one painted.
    fn settle(&mut self) -> Vec<Painted> {
        let mut painted = Vec::new();
        for _ in 0..12 {
            painted = self.pass(Vec::new(), Vec::new()).0;
        }
        painted
    }

    /// Clicks the visible text `text` that lies highest on screen, as a strip button.
    fn click(&mut self, painted: &[Painted], text: &str) {
        let target = painted
            .iter()
            .filter(|shape| shape.visible && shape.text == text)
            .min_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
            .unwrap_or_else(|| panic!("\"{text}\" is on screen"));
        let pos = target.pos + egui::vec2(4.0, 4.0);
        let press = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.pass(vec![Event::PointerMoved(pos), press(true)], Vec::new());
        self.pass(vec![press(false)], Vec::new());
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // Hand the uploaded textures to a pass, as a renderer would, before the context goes.
        let mut output = self.ctx.run_ui(RawInput::default(), |_| {});
        output.textures_delta.clear();
    }
}

/// Collects the text shapes in `shape`, nested ones included.
fn collect(shape: &Shape, clip: Rect, out: &mut Vec<Painted>) {
    match shape {
        Shape::Text(text) => {
            // A right-aligned galley lies left of its position.
            let rect = text.galley.rect.translate(text.pos.to_vec2());
            out.push(Painted {
                text: text.galley.text().to_owned(),
                pos: rect.min,
                visible: clip.contains(rect.center()),
            });
        }
        Shape::Vec(shapes) => {
            for shape in shapes {
                collect(shape, clip, out);
            }
        }
        _ => {}
    }
}

/// Whether `text` is painted where the user sees it.
fn shows(painted: &[Painted], text: &str) -> bool {
    painted
        .iter()
        .any(|shape| shape.visible && shape.text == text)
}

#[test]
fn the_new_panels_draw_and_the_strip_scrolls_to_them() {
    let mut window = Window::new();
    let path = every_bend();
    let (_, titles) = window.pass(Vec::new(), vec![path]);
    let mut titled = titles.iter().any(|title| title.contains("every-bend"));
    for _ in 0..4 {
        titled |= window
            .pass(Vec::new(), Vec::new())
            .1
            .iter()
            .any(|title| title.contains("every-bend"));
    }
    assert!(titled, "the dropped preset is adopted: {titles:?}");

    let painted = window.settle();
    assert!(
        shows(&painted, "IR-cut filter pulled"),
        "the optics panel sits near the top with the filter pulled"
    );
    assert!(
        !shows(&painted, "SDRAM address pins faulted"),
        "the memory panel starts below the fold"
    );

    for (stage, header) in [
        ("Clock", "Master clock replaced"),
        ("Memory", "SDRAM address pins faulted"),
        ("AFE", "Supply rail loaded"),
        ("Scene", "IR-cut filter pulled"),
    ] {
        let painted = window.settle();
        window.click(&painted, stage);
        let painted = window.settle();
        assert!(
            shows(&painted, header),
            "a click on the lit {stage} stage shows \"{header}\": {:?}",
            painted
                .iter()
                .filter(|shape| shape.visible)
                .map(|shape| format!("{} @ {:.0},{:.0}", shape.text, shape.pos.x, shape.pos.y))
                .collect::<Vec<_>>()
        );
    }

    let painted = window.settle();
    window.click(&painted, "Memory");
    let painted = window.settle();
    for text in [
        "a0",
        "a11",
        "bridge",
        "marginal",
        "a4 faulted",
        "a4 switching",
    ] {
        assert!(shows(&painted, text), "the pin grid shows \"{text}\"");
    }
    assert!(
        shows(&painted, "a5") && shows(&painted, "bridged"),
        "a4's partner a5 shows the bridge"
    );
}
