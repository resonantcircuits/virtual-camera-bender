//! Camera discovery and preset files on a temporary directory.

use std::fs;
use std::path::{Path, PathBuf};

use vcb_app::presets::{self, Current};
use vcb_core::Preset;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// A fresh, empty directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("old scratch directory removed");
    }
    fs::create_dir_all(&dir).expect("scratch directory created");
    dir
}

#[test]
fn discovery_lists_toml_files_by_name() {
    let dir = scratch("discover");
    let camera = fs::read_to_string(root().join("cameras/a520.toml")).expect("camera file");
    fs::write(dir.join("zx81.toml"), &camera).expect("write");
    fs::write(dir.join("a520.toml"), &camera).expect("write");
    fs::write(dir.join("notes.txt"), "not a camera").expect("write");
    fs::create_dir(dir.join("folder.toml")).expect("a directory with a camera-like name");

    let names: Vec<String> = presets::discover(&dir)
        .expect("readable")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, vec!["a520", "zx81"]);

    let loaded = presets::load_camera(&dir, "a520").expect("a valid camera loads");
    assert_eq!(loaded.info.name, "a520");
}

#[test]
fn a_missing_folder_or_camera_is_an_error_not_a_panic() {
    let dir = scratch("missing");
    presets::discover(&dir.join("absent")).expect_err("no such folder");
    let error = presets::load_camera(&dir, "a520").expect_err("no camera file");
    assert!(
        format!("{error:#}").contains(presets::CAMERAS_ENV),
        "the message says how to point at the cameras: {error:#}"
    );
    let error = presets::load_camera(&dir, "../a520").expect_err("not a file name");
    assert!(format!("{error}").contains("not a file name"), "{error}");
}

#[test]
fn an_invalid_camera_file_is_reported() {
    let dir = scratch("invalid");
    fs::write(dir.join("broken.toml"), "[camera]\nname = 3\n").expect("write");
    presets::load_camera(&dir, "broken").expect_err("a camera name must be a string");
}

#[test]
fn presets_save_and_load_through_files() {
    let dir = scratch("save");
    let mut preset = presets::load_preset(&root().join("presets/a520-inv.toml")).expect("shipped");
    preset.seed = 42;
    let path = dir.join("mine.toml");
    presets::save_preset(&path, &preset).expect("written");
    assert_eq!(presets::load_preset(&path).expect("read back"), preset);

    fs::write(dir.join("bad.toml"), "camera = \"a520\"\nseed = -1\n").expect("write");
    presets::load_preset(&dir.join("bad.toml")).expect_err("a seed cannot be negative");
}

#[test]
fn the_current_preset_knows_when_it_was_edited() {
    let preset = Preset::new("a520");
    let current = Current::unsaved("untitled", &preset);
    assert!(!current.is_edited(&preset));
    let mut edited = preset;
    edited.seed = 3;
    assert!(current.is_edited(&edited));
    assert!(
        current.path.is_none(),
        "an unsaved preset asks where to save"
    );
}
