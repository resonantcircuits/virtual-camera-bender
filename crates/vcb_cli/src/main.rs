//! `vcb`, the command line of the virtual camera bender.
//!
//! - `vcb render <input> <output> --preset <file>` renders one image through a preset.
//! - `vcb render-video <input> <output> --preset <file>` renders a video through one
//!   stateful simulator, so analog state carries from frame to frame.
//! - `vcb sheet <output> --input <image> --preset <file>...` renders an image through
//!   several presets and tiles the results, with optional reference images, into one
//!   contact sheet.
//! - `vcb inspect --preset <file>` prints the derived readout timing.
//! - `vcb presets` lists the presets in a directory.
//!
//! Camera files are looked up as `<camera-dir>/<name>.toml`, where the directory is
//! `--camera-dir`, else `$VCB_CAMERAS`, else `cameras/`. Video needs `ffmpeg` and
//! `ffprobe`, found through `$VCB_FFMPEG` and `$VCB_FFPROBE`, else `PATH`. Logging goes
//! to stderr and is controlled by `RUST_LOG`.

mod render_video;
mod sheet;

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use tracing_subscriber::EnvFilter;
use vcb_core::preset::{Mode, Resolution};
use vcb_core::{Camera, Preset, Simulator, Timing};
use vcb_io::Format;

use crate::render_video::RenderVideoArgs;
use crate::sheet::SheetArgs;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Simulate a circuit-bent early-2000s CCD compact camera.
#[derive(Debug, Parser)]
#[command(name = "vcb", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Render an image through a preset.
    Render(RenderArgs),
    /// Render a video through a preset, carrying the camera's analog state across frames.
    RenderVideo(RenderVideoArgs),
    /// Render an image through several presets into one contact sheet.
    Sheet(SheetArgs),
    /// Print the derived readout timing of a preset.
    Inspect(InspectArgs),
    /// List the presets in a directory.
    Presets(PresetsArgs),
}

#[derive(Debug, Args)]
struct CameraDir {
    /// Directory holding the camera files, `<name>.toml`.
    #[arg(long, env = "VCB_CAMERAS", default_value = "cameras")]
    camera_dir: PathBuf,
}

/// Command-line replacements for preset values.
#[derive(Debug, Args)]
struct Overrides {
    /// Override the preset's readout mode: still or movie.
    #[arg(long)]
    mode: Option<Mode>,
    /// Override the preset's sensor resolution: match-input or native.
    #[arg(long)]
    resolution: Option<Resolution>,
    /// Override the preset's noise seed, an unsigned integer.
    #[arg(long)]
    seed: Option<u64>,
}

impl Overrides {
    /// Loads the preset at `path` and applies the overrides.
    fn preset(&self, path: &Path) -> Result<Preset> {
        let mut preset = load_preset(path)?;
        if let Some(mode) = self.mode {
            preset.readout.mode = mode;
        }
        if let Some(resolution) = self.resolution {
            preset.readout.resolution = resolution;
        }
        if let Some(seed) = self.seed {
            preset.seed = seed;
        }
        Ok(preset)
    }
}

#[derive(Debug, Args)]
struct RenderArgs {
    /// Input image (JPEG or PNG).
    input: PathBuf,
    /// Output image; the extension picks PNG or JPEG.
    output: PathBuf,
    /// Preset file.
    #[arg(long)]
    preset: PathBuf,
    #[command(flatten)]
    cameras: CameraDir,
    #[command(flatten)]
    overrides: Overrides,
    /// Frame index within a sequence; selects the temporal noise.
    #[arg(long, default_value_t = 0)]
    frame: u64,
}

#[derive(Debug, Args)]
struct InspectArgs {
    /// Preset file.
    #[arg(long)]
    preset: PathBuf,
    #[command(flatten)]
    cameras: CameraDir,
    /// Image whose size the sensor matches; defaults to the camera's native size.
    #[arg(long, conflicts_with_all = ["width", "height"])]
    input: Option<PathBuf>,
    /// Video whose upright frame size the sensor matches, instead of an image.
    #[arg(long, conflicts_with_all = ["input", "width", "height"])]
    video: Option<PathBuf>,
    /// Input width in pixels, together with --height.
    #[arg(long, requires = "height")]
    width: Option<u32>,
    /// Input height in pixels, together with --width.
    #[arg(long, requires = "width")]
    height: Option<u32>,
}

#[derive(Debug, Args)]
struct PresetsArgs {
    /// Directory holding the preset files.
    #[arg(long, default_value = "presets")]
    dir: PathBuf,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Render(args) => render(&args),
        Command::RenderVideo(args) => render_video::run(&args),
        Command::Sheet(args) => sheet::run(&args),
        Command::Inspect(args) => inspect(&args),
        Command::Presets(args) => list_presets(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn render(args: &RenderArgs) -> Result<()> {
    let started = Instant::now();
    let preset = args.overrides.preset(&args.preset)?;
    let camera = load_camera(&args.cameras.camera_dir, &preset.camera)?;
    let format = output_format(&args.output)?;

    let scene = vcb_io::read_image(&args.input)?;
    let mut simulator = Simulator::builder((camera, preset))
        .input_size(scene.width(), scene.height())
        .build()
        .context("cannot build the simulator")?;
    let mode = simulator.timing().mode;
    let frame = simulator.frame(&scene, args.frame);
    vcb_io::write_frame(&args.output, frame, format)?;
    println!(
        "{} {}x{} {mode} {} ms",
        args.output.display(),
        frame.width(),
        frame.height(),
        started.elapsed().as_millis()
    );
    Ok(())
}

fn inspect(args: &InspectArgs) -> Result<()> {
    let preset = load_preset(&args.preset)?;
    let camera = load_camera(&args.cameras.camera_dir, &preset.camera)?;
    camera
        .validate()
        .with_context(|| format!("camera \"{}\" is invalid", preset.camera))?;
    let mut video = None;
    let (width, height) = match (&args.input, &args.video, args.width, args.height) {
        (Some(input), _, _, _) => {
            let probe = vcb_io::probe_image(input)?;
            (probe.width, probe.height)
        }
        (None, Some(path), _, _) => {
            let info = vcb_io::video::probe_video(path)?;
            video = Some(info);
            (info.width, info.height)
        }
        (None, None, Some(width), Some(height)) => (width, height),
        _ => (camera.sensor.native_width, camera.sensor.native_height),
    };
    let timing = Timing::derive(&camera, &preset, width, height)?;
    println!("preset        {}", args.preset.display());
    println!(
        "camera        {} {} ({})",
        camera.info.maker, camera.info.model, camera.info.name
    );
    if let Some(info) = video {
        println!("video         {info}");
    }
    println!("input         {width} x {height} px");
    println!("{timing}");
    if let Some(inject) = preset.bend.afe.as_ref().and_then(|afe| afe.inject.as_ref()) {
        println!(
            "injection     {:?} at {} Hz: {:.3} lines, {:.1} pixel periods per cycle",
            inject.wave,
            inject.freq_hz,
            timing.lines_per_cycle(inject.freq_hz),
            timing.samples_per_cycle(inject.freq_hz)
        );
    }
    Ok(())
}

fn list_presets(args: &PresetsArgs) -> Result<()> {
    let entries = fs::read_dir(&args.dir)
        .with_context(|| format!("cannot list presets in {}", args.dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry
            .with_context(|| format!("cannot list presets in {}", args.dir.display()))?
            .path();
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            paths.push(path);
        }
    }
    paths.sort();
    for path in paths {
        let name = path.file_stem().unwrap_or_default().to_string_lossy();
        match load_preset(&path) {
            Ok(preset) => {
                let bends: Vec<&str> = [
                    preset.bend.bus.as_ref().map(|_| "bus"),
                    preset.bend.clocks.as_ref().map(|_| "clocks"),
                    preset.bend.afe.as_ref().map(|_| "afe"),
                    preset.bend.optics.as_ref().map(|_| "optics"),
                    preset.bend.sensor.as_ref().map(|_| "sensor"),
                    preset.bend.rail.as_ref().map(|_| "rail"),
                    preset.bend.clock.as_ref().map(|_| "clock"),
                    preset.bend.memory.as_ref().map(|_| "memory"),
                    preset.bend.reset.as_ref().map(|_| "reset"),
                    preset.bend.serial.as_ref().map(|_| "serial"),
                    preset.bend.leak.as_ref().map(|_| "leak"),
                ]
                .into_iter()
                .flatten()
                .collect();
                let bends = if bends.is_empty() {
                    "none".to_owned()
                } else {
                    bends.join(",")
                };
                println!(
                    "{name:<24} camera={} mode={} resolution={} bends={bends}",
                    preset.camera, preset.readout.mode, preset.readout.resolution
                );
            }
            Err(error) => println!("{name:<24} invalid: {error:#}"),
        }
    }
    Ok(())
}

/// Chooses the image format of an output path from its extension.
fn output_format(path: &Path) -> Result<Format> {
    Format::from_path(path).with_context(|| {
        format!(
            "cannot tell the output format of {}; use .png, .jpg or .jpeg",
            path.display()
        )
    })
}

fn load_preset(path: &Path) -> Result<Preset> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read preset {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("cannot parse preset {}", path.display()))
}

fn load_camera(dir: &Path, name: &str) -> Result<Camera> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        bail!("camera name \"{name}\" is not a file name");
    }
    let path = dir.join(format!("{name}.toml"));
    let text = fs::read_to_string(&path).with_context(|| {
        format!(
            "cannot read camera \"{name}\" from {}; set --camera-dir or VCB_CAMERAS",
            path.display()
        )
    })?;
    toml::from_str(&text).with_context(|| format!("cannot parse camera {}", path.display()))
}
