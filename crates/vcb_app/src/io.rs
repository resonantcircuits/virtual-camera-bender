//! Opening source files, exporting renders, and running both off the UI thread.
//!
//! JPEG and PNG files are decoded by [`vcb_io::read_image`] with their EXIF
//! orientation. Any other file is treated as a video: its first frame, read through
//! [`vcb_io::video::VideoReader`] (which needs `ffmpeg`), becomes the working scene.
//! Exports go through [`vcb_io::write_frame`] as PNG or JPEG, chosen by extension.
//! A [`Task`] runs a file operation on its own thread and wakes the UI when done.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use anyhow::{Context as _, Result, anyhow, bail};
use eframe::egui::{self, ColorImage};
use vcb_core::{Frame, Scene};
use vcb_io::Format;
use vcb_io::video::{ReadOptions, VideoReader};

use crate::viewport;

/// File extensions opened as still images.
pub const IMAGE_EXTENSIONS: [&str; 3] = ["jpg", "jpeg", "png"];

/// File extensions offered as video in the open dialog; others are tried as video too.
pub const VIDEO_EXTENSIONS: [&str; 6] = ["mp4", "mov", "m4v", "mkv", "avi", "webm"];

/// What kind of file a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A still image.
    Image,
    /// A video, of which the first frame is used.
    Video,
}

impl Kind {
    /// Classifies a path by its extension: images by [`IMAGE_EXTENSIONS`], the rest as video.
    #[must_use]
    pub fn of(path: &Path) -> Self {
        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|extension| {
                IMAGE_EXTENSIONS
                    .iter()
                    .any(|known| extension.eq_ignore_ascii_case(known))
            });
        if is_image { Self::Image } else { Self::Video }
    }
}

/// An opened source file: the scene and what the UI shows about it.
#[derive(Clone)]
pub struct Source {
    /// The file.
    pub path: PathBuf,
    /// Whether it is an image or a video.
    pub kind: Kind,
    /// The linear-light scene, upright.
    pub scene: Arc<Scene>,
    /// The scene as an egui image, for showing the original.
    pub original: ColorImage,
    /// The file's size in bytes.
    pub bytes: u64,
}

impl std::fmt::Debug for Source {
    /// Prints the file and the scene's shape; the image holds millions of pixels.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("path", &self.path)
            .field("kind", &self.kind)
            .field("scene", &self.scene)
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl Source {
    /// The file name for display.
    #[must_use]
    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

/// Opens an image, or the first frame of a video, as a scene.
///
/// # Errors
///
/// Fails if the file cannot be read or decoded, if a video has no frame, or if
/// `ffmpeg` is missing for a video.
pub fn open(path: &Path) -> Result<Source> {
    let kind = Kind::of(path);
    let bytes = std::fs::metadata(path)
        .with_context(|| format!("cannot open {}", path.display()))?
        .len();
    let scene = match kind {
        Kind::Image => vcb_io::read_image(path)?,
        Kind::Video => {
            let mut reader = VideoReader::open(path, ReadOptions::default())?;
            reader
                .next_frame()?
                .ok_or_else(|| anyhow!("{} has no video frames", path.display()))?
        }
    };
    let original = viewport::texture_image(scene.width(), scene.height(), &scene.to_srgb8());
    tracing::info!(
        name: "app.open.done",
        path = %path.display(),
        width = scene.width(),
        height = scene.height(),
        "source opened"
    );
    Ok(Source {
        path: path.to_path_buf(),
        kind,
        scene: Arc::new(scene),
        original,
        bytes,
    })
}

/// Writes `frame` to `path`, as PNG or JPEG by the extension.
///
/// # Errors
///
/// Fails for an extension other than `png`, `jpg` or `jpeg`, or if the file cannot be
/// written.
pub fn export(path: &Path, frame: &Frame) -> Result<()> {
    let Some(format) = Format::from_path(path) else {
        bail!(
            "cannot tell the format of {}; use .png, .jpg or .jpeg",
            path.display()
        );
    };
    vcb_io::write_frame(path, frame, format)?;
    tracing::info!(
        name: "app.export.done",
        path = %path.display(),
        width = frame.width(),
        height = frame.height(),
        "frame exported"
    );
    Ok(())
}

/// Asks for an image or video to open.
#[must_use]
pub fn pick_source() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open image or video")
        .add_filter(
            "Image or video",
            &[IMAGE_EXTENSIONS.as_slice(), VIDEO_EXTENSIONS.as_slice()].concat(),
        )
        .add_filter("Image", &IMAGE_EXTENSIONS)
        .add_filter("Video", &VIDEO_EXTENSIONS)
        .pick_file()
}

/// Asks where to export the render, suggesting `name`.
#[must_use]
pub fn pick_export(name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Export image")
        .add_filter("PNG", &["png"])
        .add_filter("JPEG", &["jpg", "jpeg"])
        .set_file_name(name)
        .save_file()
}

/// A file operation running on its own thread.
#[derive(Debug)]
pub struct Task<T> {
    result: Receiver<Result<T>>,
}

impl<T: Send + 'static> Task<T> {
    /// Runs `work` on a new thread and repaints `ctx` when it finishes.
    ///
    /// # Errors
    ///
    /// Returns the operating system's error if the thread cannot be started.
    pub fn spawn(
        ctx: &egui::Context,
        work: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> std::io::Result<Self> {
        let (sender, result) = mpsc::channel();
        let ctx = ctx.clone();
        thread::Builder::new()
            .name("vcb-file".to_owned())
            .spawn(move || {
                // The receiver is gone only if the app dropped the task; nothing to do then.
                let _ = sender.send(work());
                ctx.request_repaint();
            })?;
        Ok(Self { result })
    }

    /// Returns the result once the work has finished, or an error if its thread died.
    #[must_use]
    pub fn poll(&self) -> Option<Result<T>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(anyhow!("the file operation stopped unexpectedly")))
            }
        }
    }
}
