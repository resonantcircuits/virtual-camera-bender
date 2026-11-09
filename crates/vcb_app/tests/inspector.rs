//! The inspector draws the panels of the four bends of milestone 3, the sensor and
//! reset blocks of milestone 4, the light leak, serial link, data pins and retention
//! of stage 19 and the compressed buffer's switch of stage 20 in a headless ui pass, a click on a lit stage of the strip scrolls their
//! panel into view, the wheel scrolls on to the rest, and a wrapped strip breaks
//! before an arrow, never after one.
//!
//! The passes run the app's real `ui` on an `egui::Context` without a renderer; the
//! painted text shapes tell what is on screen and where.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::App as _;
use eframe::egui::{
    self, DroppedFile, Event, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2,
    RawInput, Rect, Shape, TouchPhase, ViewportCommand,
};
use vcb_app::app::{App, Setup};
use vcb_app::panels::Panels;
use vcb_app::panels::memory::MemoryPanel;
use vcb_app::panels::optics::OpticsPanel;
use vcb_app::panels::rail::RailPanel;
use vcb_app::panels::reclock::ReclockPanel;
use vcb_app::panels::strip::{self, Stage};
use vcb_app::{presets, theme};
use vcb_core::preset::{Edge, FaultKind, RailBend};
use vcb_core::{Frame, Preset};

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
    assert!(
        panels
            .memory
            .address
            .set_kind(4, Some(FaultKind::Bridge), 12)
    );
    assert!(panels.memory.address.set_marginal(4, true));
    panels.rail = RailPanel {
        enabled: true,
        ..RailPanel::default()
    };
    panels.apply(&mut preset);
    let path = scratch().join("every-bend.toml");
    std::fs::write(&path, presets::to_toml(&preset).expect("serialises")).expect("written");
    path
}

/// The clean camera with the sensor bend and a weak reset gate on, saved as `sensor-reset.toml`.
fn sensor_and_reset() -> PathBuf {
    let mut preset =
        presets::load_preset(&root().join("presets/a520-clean.toml")).expect("shipped");
    let mut panels = Panels::from_preset(&preset);
    panels.sensor.enabled = true;
    panels.reset.enabled = true;
    panels.reset.bend.reset_frac = 0.25;
    panels.apply(&mut preset);
    let path = scratch().join("sensor-reset.toml");
    std::fs::write(&path, presets::to_toml(&preset).expect("serialises")).expect("written");
    path
}

/// The clean camera with a light leak from the top, a noisy serial link and stuck data pins
/// on a hot SDRAM, saved as `leak-serial-memory.toml`.
fn leak_serial_memory() -> PathBuf {
    let mut preset =
        presets::load_preset(&root().join("presets/a520-clean.toml")).expect("shipped");
    let mut panels = Panels::from_preset(&preset);
    panels.leak.enabled = true;
    panels.leak.bend.edge = Edge::Top;
    panels.serial.enabled = true;
    panels.memory = MemoryPanel {
        enabled: true,
        temperature_c: Some(120.0),
        ..MemoryPanel::default()
    };
    let data = &mut panels.memory.data;
    assert!(data.set_kind(9, Some(FaultKind::High), 16));
    assert!(data.set_kind(3, Some(FaultKind::Bridge), 16));
    assert!(data.set_marginal(3, true));
    assert!(data.set_kind(12, Some(FaultKind::Low), 16));
    panels.apply(&mut preset);
    let path = scratch().join("leak-serial-memory.toml");
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

/// Text painted in one pass: the text, where it starts, its centre, and whether it is visible.
#[derive(Debug, Clone)]
struct Painted {
    text: String,
    pos: Pos2,
    centre: Pos2,
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
                presets_dir: root().join("presets"),
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

    /// Turns the wheel `points` down over the visible text `text`; returns what then shows.
    fn scroll(&mut self, painted: &[Painted], text: &str, points: f32) -> Vec<Painted> {
        let target = painted
            .iter()
            .find(|shape| shape.visible && shape.text == text)
            .unwrap_or_else(|| panic!("\"{text}\" is on screen"));
        let wheel = Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -points),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        };
        self.pass(vec![Event::PointerMoved(target.centre), wheel], Vec::new());
        self.settle()
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
            // Where the glyphs are: a right-aligned galley lies left of its position, and
            // a label in a wrapping row starts at the row's edge with leading space.
            let rect = text
                .galley
                .rows
                .iter()
                .filter(|row| !row.glyphs.is_empty())
                .map(egui::epaint::text::PlacedRow::rect_without_leading_space)
                .reduce(Rect::union)
                .unwrap_or(text.galley.rect)
                .translate(text.pos.to_vec2());
            out.push(Painted {
                text: text.galley.text().to_owned(),
                pos: rect.min,
                centre: rect.center(),
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
        !shows(&painted, "SDRAM bent"),
        "the memory panel starts below the fold"
    );

    for (stage, header) in [
        ("Clock", "Master clock replaced"),
        ("Memory", "SDRAM bent"),
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

#[test]
fn the_sensor_and_reset_blocks_draw_and_the_strip_reaches_them() {
    let mut window = Window::new();
    window.pass(Vec::new(), vec![sensor_and_reset()]);
    for (stage, texts) in [
        (
            "Sensor",
            [
                "Sensor bent",
                "Dark current 181 × that at 25 °C; full well 17800 e.",
            ],
        ),
        (
            "AFE",
            [
                "Reset pulses faulted",
                "Swing 23077 e; a weak reset shows on packets above 5769 e.",
            ],
        ),
    ] {
        let painted = window.settle();
        window.click(&painted, stage);
        let painted = window.settle();
        for text in texts {
            assert!(
                shows(&painted, text),
                "a click on the lit {stage} stage shows \"{text}\": {:?}",
                painted
                    .iter()
                    .filter(|shape| shape.visible)
                    .map(|shape| format!("{} @ {:.0},{:.0}", shape.text, shape.pos.x, shape.pos.y))
                    .collect::<Vec<_>>()
            );
        }
    }
}

/// The visible texts of `painted` with their positions, for a failure message.
fn on_screen(painted: &[Painted]) -> Vec<String> {
    painted
        .iter()
        .filter(|shape| shape.visible)
        .map(|shape| format!("{} @ {:.0},{:.0}", shape.text, shape.pos.x, shape.pos.y))
        .collect()
}

#[test]
fn the_stage19_blocks_draw_and_every_new_control_is_reachable() {
    let mut window = Window::new();
    window.pass(Vec::new(), vec![leak_serial_memory()]);
    let pages: [(&str, &[&str]); 3] = [
        (
            "Sensor",
            &[
                "Light leak",
                "Body open",
                "Edge",
                "top",
                "Radiance",
                "Spread",
                "Red",
                "Green",
                "Blue",
                "Near infrared",
                "0.500 × white at the top edge, 1/e after 511 of the sensor's 1704 rows, 3.57 % at the bottom edge.",
            ],
        ),
        (
            "AFE",
            &[
                "Serial link",
                "SDATA contact noisy",
                "Bit error",
                "224 bits a frame in 7 writes of 8 + 24; on average 0.221 writes corrupted a frame, at least one in 20.1 % of the frames.",
            ],
        ),
        (
            "Memory",
            &[
                "SDRAM bent",
                "Retention",
                "Temperature",
                "Median retention 177 ms against the 64 ms refresh: 2.11 % of the cells leak before it comes, in 19.2 % of the samples.",
                "Compressed buffer",
                "untouched",
                "in the bent chip",
                "Address pins",
                "a0",
            ],
        ),
    ];
    let mut painted = Vec::new();
    for (stage, texts) in pages {
        let before = window.settle();
        window.click(&before, stage);
        painted = window.settle();
        for text in texts {
            assert!(
                shows(&painted, text),
                "a click on the lit {stage} stage shows \"{text}\": {:?}",
                on_screen(&painted)
            );
        }
    }

    // The data pins sit below the address pins: the wheel brings them up.
    assert!(
        !shows(&painted, "d15") && !shows(&painted, "d3 switching"),
        "the data pins start below the fold"
    );
    let painted = window.scroll(&painted, "Retention", 520.0);
    for text in [
        "Data pins",
        "Pins d0 to d9 hold the 10-bit sample, d9 its top bit; a stuck pin shifts black with the signal. Pins d10 to d15 carry no sample bit on this camera: dimmed, their faults change nothing.",
        "d0",
        "d9",
        "d12",
        "d15",
        "high",
        "low",
        "bridge",
        "bridged",
        "Marginal data contacts",
        "d3 faulted",
        "d3 switching",
    ] {
        assert!(
            shows(&painted, text),
            "the wheel scrolls the data pins into view with \"{text}\": {:?}",
            on_screen(&painted)
        );
    }
}

/// The strip's texts drawn `width` points wide for `preset`, row by row, left to right.
fn strip_rows(ctx: &egui::Context, preset: &Preset, width: f32) -> Vec<Vec<String>> {
    let mut output = None;
    // The first pass takes in the theme's fonts; the second lays the strip out with them.
    for _ in 0..2 {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            ..RawInput::default()
        };
        let mut pass = ctx.run_ui(input, |ui| {
            ui.allocate_ui(egui::vec2(width, 400.0), |ui| {
                ui.set_max_width(width);
                strip::show(ui, preset);
            });
        });
        // No renderer takes the font atlas; drop its updates.
        pass.textures_delta.clear();
        output = Some(pass);
    }
    let mut painted = Vec::new();
    for clipped in &output.expect("two passes ran").shapes {
        collect(&clipped.shape, clipped.clip_rect, &mut painted);
    }
    painted.sort_by(|a, b| a.centre.y.total_cmp(&b.centre.y));
    let mut rows: Vec<(f32, Vec<Painted>)> = Vec::new();
    for shape in painted {
        match rows.last_mut() {
            Some((y, row)) if (shape.centre.y - *y).abs() < 4.0 => row.push(shape),
            _ => rows.push((shape.centre.y, vec![shape])),
        }
    }
    rows.into_iter()
        .map(|(_, mut row)| {
            row.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
            row.into_iter().map(|shape| shape.text).collect()
        })
        .collect()
}

#[test]
fn a_wrapped_strip_breaks_before_an_arrow() {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let mut preset = Preset::new("a520");
    preset.bend.rail = Some(RailBend::default());
    let names: Vec<&str> = Stage::ALL.iter().map(|stage| stage.name()).collect();
    let mut wrapped = 0;
    for width in (160..=600).step_by(4) {
        let width = width as f32;
        let rows = strip_rows(&ctx, &preset, width);
        let stages: Vec<&str> = rows
            .iter()
            .flatten()
            .map(String::as_str)
            .filter(|text| *text != strip::ARROW)
            .collect();
        assert_eq!(stages, names, "at {width} pt every stage is drawn in order");
        assert_eq!(
            rows.iter()
                .flatten()
                .filter(|text| *text == strip::ARROW)
                .count(),
            names.len() - 1
        );
        assert_ne!(
            rows[0].first().map(String::as_str),
            Some(strip::ARROW),
            "at {width} pt the strip starts with a stage: {rows:?}"
        );
        for row in &rows {
            assert_ne!(
                row.last().map(String::as_str),
                Some(strip::ARROW),
                "at {width} pt a row ends with an arrow: {rows:?}"
            );
            for (i, text) in row.iter().enumerate() {
                if text == strip::ARROW {
                    assert!(
                        row.get(i + 1)
                            .is_some_and(|next| names.contains(&next.as_str())),
                        "at {width} pt an arrow shares its row with the stage after it: {rows:?}"
                    );
                }
            }
        }
        wrapped += usize::from(rows.len() > 1);
    }
    assert!(wrapped > 10, "the narrower widths wrap the strip");
}
