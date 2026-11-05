//! Camera discovery, preset files and the app's resources.
//!
//! Camera files live in one directory as `<name>.toml`; a preset names its camera by
//! that stem, as the command line does. The directory is `$VCB_CAMERAS` if set, else
//! `cameras/` in the working directory, else `cameras/` in the resources of the app
//! bundle the binary runs from (`Contents/Resources`, where `scripts/bundle-macos.sh`
//! puts it). Presets are listed from `presets/` and the sample photo is found in
//! `samples/`, looked up the same way, and presets are opened or saved anywhere
//! through file dialogs. A preset written by the app holds the values only: TOML
//! comments of a hand-written file are not kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use vcb_core::{Camera, Preset};

/// Environment variable naming the camera directory.
pub const CAMERAS_ENV: &str = "VCB_CAMERAS";

/// Camera directory used when [`CAMERAS_ENV`] is not set, relative to the working directory.
pub const CAMERAS_DIR: &str = "cameras";

/// Directory of the preset list, relative to the working directory or the bundle's resources.
pub const PRESETS_DIR: &str = "presets";

/// The sample photo, relative to the working directory or the bundle's resources.
pub const SAMPLE_PHOTO: &str = "samples/sample-01.jpg";

/// Finds `relative` in the working directory, else in the app bundle's resources.
///
/// Returns `None` if neither exists.
#[must_use]
pub fn resource(relative: &str) -> Option<PathBuf> {
    let here = PathBuf::from(relative);
    if here.exists() {
        return Some(here);
    }
    let bundled = std::env::current_exe()
        .ok()?
        .parent()?
        .parent()?
        .join("Resources")
        .join(relative);
    bundled.exists().then_some(bundled)
}

/// Returns the directory of the preset list: `presets/` here or in the bundle.
#[must_use]
pub fn presets_dir() -> PathBuf {
    resource(PRESETS_DIR).unwrap_or_else(|| PathBuf::from(PRESETS_DIR))
}

/// Returns the sample photo, if the repository or the bundle holds it.
#[must_use]
pub fn sample_photo() -> Option<PathBuf> {
    resource(SAMPLE_PHOTO)
}

/// Whether `dir` lies inside an app bundle, where saved files would be lost on update.
fn in_bundle(dir: &Path) -> bool {
    absolute(dir)
        .components()
        .any(|part| part.as_os_str().to_string_lossy().ends_with(".app"))
}

/// A configuration file found in a directory: its stem and its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The file stem, which is how presets name a camera.
    pub name: String,
    /// The file's path.
    pub path: PathBuf,
}

/// The preset being edited, with where it came from.
///
/// `path` is the file that "Save" writes; it is set when the preset was opened through
/// a dialog or saved, and left empty for presets picked from the shipped list, so that
/// saving one of those asks for a new file instead of overwriting the original.
#[derive(Debug, Clone, PartialEq)]
pub struct Current {
    /// The file "Save" writes, if any.
    pub path: Option<PathBuf>,
    /// The name shown in the preset picker.
    pub name: String,
    /// The preset as loaded or last saved, to tell whether it has been edited.
    pub saved: Preset,
}

impl Current {
    /// Describes an unsaved preset called `name`.
    #[must_use]
    pub fn unsaved(name: impl Into<String>, preset: &Preset) -> Self {
        Self {
            path: None,
            name: name.into(),
            saved: preset.clone(),
        }
    }

    /// Whether `preset` differs from the preset as loaded or last saved.
    #[must_use]
    pub fn is_edited(&self, preset: &Preset) -> bool {
        self.saved != *preset
    }
}

/// Returns the camera directory: `$VCB_CAMERAS`, else `cameras/` here or in the bundle.
#[must_use]
pub fn camera_dir() -> PathBuf {
    std::env::var_os(CAMERAS_ENV).map_or_else(
        || resource(CAMERAS_DIR).unwrap_or_else(|| PathBuf::from(CAMERAS_DIR)),
        PathBuf::from,
    )
}

/// Lists the `.toml` files directly inside `dir`, sorted by name.
///
/// Subdirectories and files with other extensions are skipped.
///
/// # Errors
///
/// Returns the I/O error if the directory cannot be read.
pub fn discover(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let is_toml = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"));
        if !is_toml || !path.is_file() {
            continue;
        }
        if let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) {
            entries.push(Entry {
                name: name.to_owned(),
                path,
            });
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// Reads and validates the camera called `name` from `dir`.
///
/// # Errors
///
/// Fails if the name is not a plain file name, the file cannot be read or parsed, or
/// a camera value is out of range.
pub fn load_camera(dir: &Path, name: &str) -> Result<Camera> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        bail!("camera name \"{name}\" is not a file name");
    }
    let path = dir.join(format!("{name}.toml"));
    let text = fs::read_to_string(&path).with_context(|| {
        format!(
            "camera \"{name}\" not found at {}; set {CAMERAS_ENV} to the camera folder",
            path.display()
        )
    })?;
    let camera: Camera =
        toml::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))?;
    camera
        .validate()
        .with_context(|| format!("camera \"{name}\" is invalid"))?;
    Ok(camera)
}

/// Parses and validates a preset from TOML text.
///
/// # Errors
///
/// Fails on a TOML or schema error, or when a value is out of range.
pub fn from_toml(text: &str) -> Result<Preset> {
    let preset: Preset = toml::from_str(text)?;
    preset.validate()?;
    Ok(preset)
}

/// Writes a preset as TOML.
///
/// # Errors
///
/// Fails if a value cannot be represented in TOML, such as a seed above `i64::MAX`.
pub fn to_toml(preset: &Preset) -> Result<String> {
    toml::to_string(preset).context("cannot write the preset as TOML")
}

/// Reads and validates the preset file at `path`.
///
/// # Errors
///
/// Fails if the file cannot be read or holds an invalid preset.
pub fn load_preset(path: &Path) -> Result<Preset> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    from_toml(&text).with_context(|| format!("invalid preset {}", path.display()))
}

/// Writes `preset` to `path` as TOML, replacing the file.
///
/// # Errors
///
/// Fails if the preset cannot be serialised or the file cannot be written.
pub fn save_preset(path: &Path, preset: &Preset) -> Result<()> {
    let text = to_toml(preset)?;
    fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))
}

/// Asks for a preset file to open.
#[must_use]
pub fn pick_open() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Load preset")
        .add_filter("Preset", &["toml"])
        .set_directory(absolute(&presets_dir()))
        .pick_file()
}

/// Asks where to save a preset, suggesting `name.toml`.
///
/// The dialog opens in the preset's own folder, else the preset list's folder, unless
/// that lies inside the app bundle; then it opens in the home folder.
#[must_use]
pub fn pick_save(current: &Current) -> Option<PathBuf> {
    let directory = current
        .path
        .as_deref()
        .and_then(Path::parent)
        .map_or_else(presets_dir, Path::to_path_buf);
    let directory = if in_bundle(&directory) {
        std::env::var_os("HOME").map_or(directory, PathBuf::from)
    } else {
        directory
    };
    rfd::FileDialog::new()
        .set_title("Save preset")
        .add_filter("Preset", &["toml"])
        .set_directory(absolute(&directory))
        .set_file_name(format!("{}.toml", current.name))
        .save_file()
}

/// Makes `dir` absolute for the file dialogs, which do not resolve relative paths.
pub(crate) fn absolute(dir: &Path) -> PathBuf {
    std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf())
}
