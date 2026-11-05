//! Video export: a clip through a fresh full-size simulator into a file, off the UI thread.
//!
//! [`Export::start`] runs, on its own thread, what `vcb render-video` runs: a
//! [`VideoReader`] over the chosen window of the clip, a [`Simulator`] built for the
//! source size and powered on at the first frame, and a [`VideoWriter`] at the
//! source's frame rate, with frames cropped to the even size the encoders need. The
//! UI reads the progress (frames done, the estimated total, the rate and the time
//! left) whenever it draws, and can cancel; a cancelled export removes its unfinished
//! file. [`Dialog`] is the form that collects the settings.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use eframe::egui::{self, DragValue, Grid, RichText, TextEdit, Ui};
use vcb_core::{Camera, Preset, Simulator, Timing};
use vcb_io::video::{Codec, ReadOptions, VideoReader, VideoWriter, WriteOptions, frame_to_even};

use crate::panels;
use crate::theme;
use crate::video::Clip;

/// Shortest interval between two progress notifications to the UI.
///
/// Ten updates a second keep the progress bar moving without waking the UI for every
/// frame of a fast render.
const NOTIFY_INTERVAL: Duration = Duration::from_millis(100);

/// Size against fidelity of H.264 output, as `vcb render-video --quality` offers it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Quality {
    /// CRF 28: small files, visible compression.
    Small,
    /// CRF 23: libx264's default balance.
    #[default]
    Balanced,
    /// CRF 18: close to visually lossless.
    High,
}

impl Quality {
    /// The libx264 constant rate factor.
    #[must_use]
    pub fn crf(self) -> u8 {
        match self {
            Self::Small => 28,
            Self::Balanced => 23,
            Self::High => 18,
        }
    }
}

/// Everything an export needs; the camera and preset are a snapshot taken at the start.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The source video.
    pub input: PathBuf,
    /// The file to write; `.mp4` or `.mov` for H.264, `.mov` for `ProRes`.
    pub output: PathBuf,
    /// The camera.
    pub camera: Camera,
    /// The preset, rendered from power-on.
    pub preset: Preset,
    /// The output codec.
    pub codec: Codec,
    /// The H.264 quality; `ProRes` is always 422 HQ.
    pub quality: Quality,
    /// Where to start in the source, in seconds.
    pub start_s: f64,
    /// How much of the source to render, in seconds; `None` renders to the end.
    pub duration_s: Option<f64>,
}

/// How far an export has come.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    /// Frames rendered and written.
    pub done: u64,
    /// The estimated number of frames, if the source reports its length.
    pub total: Option<u64>,
    /// Time since the export started.
    pub elapsed: Duration,
}

impl Progress {
    /// Frames rendered per second so far.
    #[must_use]
    pub fn rate(&self) -> f64 {
        self.done as f64 / self.elapsed.as_secs_f64().max(f64::EPSILON)
    }

    /// The fraction done, if the total is known.
    #[must_use]
    pub fn fraction(&self) -> Option<f32> {
        self.total
            .filter(|&total| total > 0)
            .map(|total| (self.done as f64 / total as f64).min(1.0) as f32)
    }

    /// The time left at the rate so far, once a frame is done and the total is known.
    #[must_use]
    pub fn eta(&self) -> Option<Duration> {
        let total = self.total?;
        (self.done > 0).then(|| {
            let left = total.saturating_sub(self.done) as f64;
            Duration::from_secs_f64(left / self.rate())
        })
    }
}

/// How an export ended without an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Every frame was written.
    Finished {
        /// The written file.
        path: PathBuf,
        /// Frames written.
        frames: u64,
        /// Size of the written frames.
        size: [u32; 2],
    },
    /// The user cancelled; the unfinished file was removed.
    Cancelled,
}

/// Progress shared between the export thread and the UI.
#[derive(Debug)]
struct Shared {
    done: AtomicU64,
    /// The estimated total, or `u64::MAX` while unknown.
    total: AtomicU64,
    cancel: AtomicBool,
    started: Instant,
}

/// A running or finished export.
#[derive(Debug)]
pub struct Export {
    shared: Arc<Shared>,
    result: Receiver<Result<Outcome>>,
    output: PathBuf,
}

impl Export {
    /// Starts exporting with `settings`; `notify` is called on progress and at the end.
    ///
    /// # Errors
    ///
    /// Returns the operating system's error if the thread cannot be started.
    pub fn start(settings: Settings, notify: impl Fn() + Send + 'static) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            done: AtomicU64::new(0),
            total: AtomicU64::new(u64::MAX),
            cancel: AtomicBool::new(false),
            started: Instant::now(),
        });
        let (sender, result) = mpsc::channel();
        let output = settings.output.clone();
        let progress = Arc::clone(&shared);
        thread::Builder::new()
            .name("vcb-export".to_owned())
            .spawn(move || {
                let outcome = run(&settings, &progress, &notify);
                if let Err(error) = &outcome {
                    tracing::warn!(name: "app.export.failed", error = %format!("{error:#}"), "export failed");
                }
                // The receiver is gone only if the app dropped the export; nothing to do then.
                let _ = sender.send(outcome);
                notify();
            })?;
        Ok(Self {
            shared,
            result,
            output,
        })
    }

    /// The progress so far.
    #[must_use]
    pub fn progress(&self) -> Progress {
        let total = self.shared.total.load(Ordering::Relaxed);
        Progress {
            done: self.shared.done.load(Ordering::Relaxed),
            total: (total != u64::MAX).then_some(total),
            elapsed: self.shared.started.elapsed(),
        }
    }

    /// The file being written.
    #[must_use]
    pub fn output(&self) -> &Path {
        &self.output
    }

    /// Asks the export to stop before its next frame.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
    }

    /// Whether [`cancel`](Self::cancel) has been called.
    #[must_use]
    pub fn is_cancelling(&self) -> bool {
        self.shared.cancel.load(Ordering::Relaxed)
    }

    /// The result once the export has ended.
    ///
    /// # Errors
    ///
    /// The returned result holds the export's error: the source cannot be read, the
    /// preset is invalid for the source, the file cannot be written, or the export
    /// thread died.
    #[must_use]
    pub fn poll(&self) -> Option<Result<Outcome>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(anyhow!("the export stopped unexpectedly")))
            }
        }
    }
}

/// The export thread's work.
fn run(settings: &Settings, shared: &Shared, notify: &impl Fn()) -> Result<Outcome> {
    refuse_overwrite(&settings.input, &settings.output)?;
    let mut reader = VideoReader::open(
        &settings.input,
        ReadOptions {
            start_s: Some(settings.start_s).filter(|&start_s| start_s > 0.0),
            duration_s: settings.duration_s,
            max_dimension: None,
        },
    )?;
    let mut simulator = Simulator::builder((settings.camera.clone(), settings.preset.clone()))
        .input_size(reader.width(), reader.height())
        .build()
        .context("cannot set up the camera for this video")?;
    let timing = simulator.timing();
    let (width, height) = (timing.output_width & !1, timing.output_height & !1);
    let crop = (width, height) != (timing.output_width, timing.output_height);
    let options = WriteOptions {
        fps: reader.info().fps,
        crf: settings.quality.crf(),
        codec: settings.codec,
    };
    let mut writer = VideoWriter::create(&settings.output, width, height, options)?;
    if let Some(total) = reader.estimated_frames() {
        shared.total.store(total, Ordering::Relaxed);
    }
    tracing::info!(
        name: "app.export.start",
        input = %settings.input.display(),
        output = %settings.output.display(),
        width,
        height,
        "export started"
    );
    let mut index = 0;
    let mut notified = Instant::now();
    loop {
        if shared.cancel.load(Ordering::Relaxed) {
            drop(writer);
            remove_unfinished(&settings.output);
            return Ok(Outcome::Cancelled);
        }
        let Some(scene) = reader.next_frame()? else {
            break;
        };
        let frame = simulator.frame(&scene, index);
        if crop {
            writer.write_frame(&frame_to_even(frame))?;
        } else {
            writer.write_frame(frame)?;
        }
        index += 1;
        shared.done.store(index, Ordering::Relaxed);
        if notified.elapsed() >= NOTIFY_INTERVAL {
            notified = Instant::now();
            notify();
        }
    }
    if index == 0 {
        drop(writer);
        remove_unfinished(&settings.output);
        bail!(
            "{} has no frames between {:.2} s and the end of the window",
            settings.input.display(),
            settings.start_s
        );
    }
    writer.finish()?;
    shared.total.store(index, Ordering::Relaxed);
    Ok(Outcome::Finished {
        path: settings.output.clone(),
        frames: index,
        size: [width, height],
    })
}

/// Refuses to write the output over the input, which ffmpeg would truncate while reading.
fn refuse_overwrite(input: &Path, output: &Path) -> Result<()> {
    if let (Ok(input_path), Ok(output_path)) = (fs::canonicalize(input), fs::canonicalize(output))
        && input_path == output_path
    {
        bail!(
            "the output {} is the source video; choose another file",
            output.display()
        );
    }
    Ok(())
}

/// Removes the file an interrupted writer left behind.
fn remove_unfinished(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(name: "app.export.cleanup", path = %path.display(), %error, "cannot remove the unfinished file");
    }
}

/// Formats a duration as `m:ss`, or `h:mm:ss` from an hour on.
#[must_use]
pub fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Returns `path` with the extension the codec needs: `.mov` for `ProRes`.
#[must_use]
pub fn with_codec_extension(path: &Path, codec: Codec) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match (codec, extension.as_deref()) {
        (Codec::ProRes, Some("mov")) | (Codec::H264, Some("mp4" | "mov" | "m4v")) => {
            path.to_path_buf()
        }
        (Codec::ProRes, _) => path.with_extension("mov"),
        (Codec::H264, _) => path.with_extension("mp4"),
    }
}

/// The export form: output file, codec, quality and the part of the clip to render.
#[derive(Debug, Clone, PartialEq)]
pub struct Dialog {
    /// The file to write, as typed or chosen.
    pub output: String,
    /// The codec.
    pub codec: Codec,
    /// The H.264 quality.
    pub quality: Quality,
    /// Start in the clip, in seconds.
    pub start_s: f64,
    /// Length to render in seconds, used unless `to_end` is set.
    pub duration_s: f64,
    /// Whether to render from the start to the end of the clip.
    pub to_end: bool,
}

/// What the user did in the export form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Still filling in the form.
    Open,
    /// Start the export.
    Export,
    /// Close the form without exporting.
    Close,
}

impl Dialog {
    /// A form for `clip` suggesting `output`, starting at `start_s`.
    #[must_use]
    pub fn new(clip: &Clip, output: &Path, start_s: f64) -> Self {
        Self {
            output: output.display().to_string(),
            codec: Codec::H264,
            quality: Quality::Balanced,
            start_s: start_s.clamp(0.0, clip.duration_s()),
            duration_s: (clip.duration_s() - start_s).max(0.0),
            to_end: true,
        }
    }

    /// The settings of an export of `clip` through `camera` and `preset`.
    ///
    /// # Errors
    ///
    /// Fails if no output file is given.
    pub fn settings(&self, clip: &Clip, camera: &Camera, preset: &Preset) -> Result<Settings> {
        let output = self.output.trim();
        if output.is_empty() {
            bail!("choose a file to export to");
        }
        Ok(Settings {
            input: clip.path.clone(),
            output: with_codec_extension(Path::new(output), self.codec),
            camera: camera.clone(),
            preset: preset.clone(),
            codec: self.codec,
            quality: self.quality,
            start_s: self.start_s,
            duration_s: (!self.to_end).then_some(self.duration_s),
        })
    }

    /// The number of source frames the form selects.
    #[must_use]
    pub fn frames(&self, clip: &Clip) -> u64 {
        let available = (clip.duration_s() - self.start_s).max(0.0);
        let length = if self.to_end {
            available
        } else {
            self.duration_s.min(available)
        };
        (length * clip.info.fps).round() as u64
    }

    /// Draws the form; `timing` is the readout for the clip's frames, `current_s` the frame on screen.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        clip: &Clip,
        timing: Option<&Timing>,
        current_s: f64,
    ) -> Choice {
        let mut choice = Choice::Open;
        panels::title(ui, "Export video");
        ui.add_space(theme::GRID);
        Grid::new("export")
            .num_columns(2)
            .spacing([theme::GRID, theme::GRID])
            .show(ui, |ui| {
                self.file_rows(ui);
                self.window_rows(ui, clip.duration_s(), current_s);
            });
        ui.add_space(theme::GRID);
        if let Some(timing) = timing {
            panels::hint(
                ui,
                &format!(
                    "{} × {} px, {} readout, about {} frames at {:.3} fps",
                    timing.output_width & !1,
                    timing.output_height & !1,
                    timing.mode,
                    self.frames(clip),
                    clip.info.fps
                ),
            );
        }
        panels::hint(
            ui,
            "Renders from power-on at full size, as vcb render-video does; the app stays usable.",
        );
        ui.add_space(theme::GRID);
        ui.horizontal(|ui| {
            let ready = !self.output.trim().is_empty() && self.frames(clip) > 0;
            let export = egui::Button::new(RichText::new("Export").color(theme::ACCENT));
            if ui.add_enabled(ready, export).clicked() {
                choice = Choice::Export;
            }
            if ui.button("Cancel").clicked() {
                choice = Choice::Close;
            }
        });
        choice
    }

    /// The rows for the output file, the codec and the quality.
    fn file_rows(&mut self, ui: &mut Ui) {
        ui.label("File");
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut self.output).desired_width(theme::GRID * 32.0));
            if ui.button("Choose…").clicked()
                && let Some(path) = pick_output(Path::new(&self.output), self.codec)
            {
                self.output = path.display().to_string();
            }
        });
        ui.end_row();

        ui.label("Codec");
        let codec = panels::choice(
            ui,
            &mut self.codec,
            &[
                (Codec::H264, "H.264"),
                (Codec::ProRes, "ProRes 422 HQ (.mov)"),
            ],
        );
        if codec.changed() && !self.output.trim().is_empty() {
            self.output = with_codec_extension(Path::new(self.output.trim()), self.codec)
                .display()
                .to_string();
        }
        ui.end_row();

        ui.label("Quality");
        let quality = self.codec == Codec::H264;
        ui.add_enabled_ui(quality, |ui| {
            panels::choice(
                ui,
                &mut self.quality,
                &[
                    (Quality::Small, "Small"),
                    (Quality::Balanced, "Balanced"),
                    (Quality::High, "High"),
                ],
            )
            .on_hover_text("H.264 constant rate factor 28, 23 or 18; ProRes is always 422 HQ")
        });
        ui.end_row();
    }

    /// The rows for the start and duration within a clip of `length` seconds.
    fn window_rows(&mut self, ui: &mut Ui, length: f64, current_s: f64) {
        ui.label("Start");
        ui.horizontal(|ui| {
            ui.add(
                DragValue::new(&mut self.start_s)
                    .range(0.0..=length)
                    .speed(0.05)
                    .fixed_decimals(2)
                    .suffix(" s"),
            );
            if ui.button("Current frame").clicked() {
                self.start_s = current_s.clamp(0.0, length);
            }
        });
        ui.end_row();

        ui.label("Duration");
        ui.horizontal(|ui| {
            ui.add_enabled(
                !self.to_end,
                DragValue::new(&mut self.duration_s)
                    .range(0.0..=length)
                    .speed(0.05)
                    .fixed_decimals(2)
                    .suffix(" s"),
            );
            ui.checkbox(&mut self.to_end, "to the end");
        });
        ui.end_row();
    }
}

/// Asks where to write the video, starting from `current`.
fn pick_output(current: &Path, codec: Codec) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title("Export video");
    dialog = match codec {
        Codec::H264 => dialog
            .add_filter("MPEG-4", &["mp4"])
            .add_filter("QuickTime", &["mov"]),
        Codec::ProRes => dialog.add_filter("QuickTime", &["mov"]),
    };
    if let Some(directory) = current.parent().filter(|dir| dir.is_dir()) {
        dialog = dialog.set_directory(directory);
    }
    if let Some(name) = current.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    dialog
        .save_file()
        .map(|path| with_codec_extension(&path, codec))
}
