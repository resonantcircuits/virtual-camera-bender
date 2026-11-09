//! The app takes in renders while its window is hidden, and the screenshot switch fires.
//!
//! eframe calls only `App::logic` while the window is occluded, minimised, or its
//! redraws are withheld; these tests drive the app the same way, with no ui pass.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::App as _;
use eframe::egui::{self, RawInput, ViewportCommand};
use vcb_app::app::{App, Setup};
use vcb_app::render::Generations;
use vcb_core::Frame;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// A small gradient photo on disk.
fn photo() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vcb_app_app");
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let path = dir.join("gradient.png");
    let (width, height) = (96, 72);
    let rgb = (0..width * height)
        .flat_map(|i| {
            let x = (i % width * 255 / width) as u8;
            [x, 120, 255 - x]
        })
        .collect();
    vcb_io::write_frame(
        &path,
        &Frame::from_rgb(width, height, rgb),
        vcb_io::Format::Png,
    )
    .expect("the photo is written");
    path
}

/// Whether a logic pass asked for a screenshot.
fn asks_for_screenshot(output: &egui::LogicOutput) -> bool {
    output.viewport_commands.values().any(|commands| {
        commands
            .iter()
            .any(|command| matches!(command, ViewportCommand::Screenshot(_)))
    })
}

#[test]
fn a_hidden_window_takes_in_the_full_render_and_asks_for_the_screenshot() {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app = App::with_setup(
        &cc,
        Setup {
            path: Some(photo()),
            camera_dir: root().join("cameras"),
            presets_dir: root().join("presets"),
            screenshot: Some(PathBuf::from("never-written.png")),
        },
    );
    let mut frame = eframe::Frame::_new_kittest();
    let deadline = Instant::now() + Duration::from_mins(2);
    let mut asked = false;
    while !asked {
        assert!(
            Instant::now() < deadline,
            "the full render never landed without a ui pass"
        );
        let output = ctx.run_logic(&RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        asked = asks_for_screenshot(&output);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        app.shows_full_render(),
        "the screenshot waits for the full render"
    );
    // Hand the uploaded textures to a pass, as a renderer would, before the context goes.
    let mut output = ctx.run_ui(RawInput::default(), |_| {});
    output.textures_delta.clear();
}

#[test]
fn the_full_render_after_a_preview_is_shown_and_is_the_latest() {
    let mut generations = Generations::default();
    let preview = generations.issue();
    assert!(generations.accept(preview));
    let full = generations.issue();
    assert!(generations.is_pending(), "the full render is awaited");
    assert!(
        generations.accept(full),
        "the full render replaces the preview"
    );
    assert!(
        generations.is_latest(full),
        "it answers the newest request, so the export and screenshot wait for it"
    );
    assert!(!generations.is_pending());
}
