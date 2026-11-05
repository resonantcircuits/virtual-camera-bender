//! End-to-end runs of the `vcb` binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn vcb(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vcb"))
        .args(args)
        .current_dir(root())
        .env_remove("VCB_CAMERAS")
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vcb_cli");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn path_arg(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn render_writes_the_upright_input_size() {
    let output = scratch("sample-clean.png");
    let _ = std::fs::remove_file(&output);
    let run = vcb(&[
        "render",
        "samples/sample-01.jpg",
        path_arg(&output),
        "--preset",
        "presets/a520-clean.toml",
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let input = vcb_io::probe_image(root().join("samples/sample-01.jpg")).unwrap();
    let written = vcb_io::probe_image(&output).unwrap();
    assert_eq!((written.width, written.height), (input.width, input.height));
    let stdout = text(&run.stdout);
    assert!(
        stdout.contains(&format!("{}x{}", input.width, input.height)) && stdout.contains("still"),
        "{stdout}"
    );
}

#[test]
fn render_honours_the_camera_directory_and_mode_override() {
    let output = scratch("sample-movie.jpg");
    let run = vcb(&[
        "render",
        "samples/sample-01.jpg",
        path_arg(&output),
        "--preset",
        "presets/a520-hpf.toml",
        "--camera-dir",
        path_arg(&root().join("cameras")),
        "--mode",
        "movie",
        "--seed",
        "9",
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    assert!(text(&run.stdout).contains("movie"));
    let written = vcb_io::probe_image(&output).unwrap();
    assert!(written.width < written.height);
}

#[test]
fn render_explains_a_missing_camera() {
    let run = vcb(&[
        "render",
        "samples/sample-01.jpg",
        path_arg(&scratch("never.png")),
        "--preset",
        "presets/a520-clean.toml",
        "--camera-dir",
        "no-such-dir",
    ]);
    assert!(!run.status.success());
    assert!(text(&run.stderr).contains("a520"), "{}", text(&run.stderr));
}

#[test]
fn inspect_prints_the_derived_timing() {
    let run = vcb(&[
        "inspect",
        "--preset",
        "presets/a520-clean.toml",
        "--width",
        "640",
        "--height",
        "480",
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let stdout = text(&run.stdout);
    for needle in ["pixel clock", "line period", "frame period", "640 x 480"] {
        assert!(stdout.contains(needle), "missing {needle:?} in\n{stdout}");
    }
}

#[test]
fn presets_lists_every_preset_file() {
    let run = vcb(&["presets", "--dir", "presets"]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let stdout = text(&run.stdout);
    for name in ["a520-clean", "a520-hpf", "a520-div", "a520-inv"] {
        assert!(stdout.contains(name), "missing {name} in\n{stdout}");
    }
    assert!(stdout.contains("bends=bus"), "{stdout}");
}
