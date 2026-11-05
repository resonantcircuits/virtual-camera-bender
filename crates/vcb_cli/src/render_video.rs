//! `vcb render-video`: a video through one stateful simulator.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use vcb_core::Simulator;
use vcb_io::video::{Codec, ReadOptions, VideoReader, VideoWriter, WriteOptions, frame_to_even};

use crate::{CameraDir, Overrides, load_camera};

/// Frames between two progress lines on stderr.
///
/// About a second of video at common frame rates: frequent enough to show the
/// render is alive, rare enough not to flood a log.
const PROGRESS_INTERVAL_FRAMES: u64 = 30;

#[derive(Debug, Args)]
pub(crate) struct RenderVideoArgs {
    /// Input video, in any format ffmpeg reads.
    input: PathBuf,
    /// Output video: .mp4 or .mov for h264, .mov for prores.
    output: PathBuf,
    /// Preset file.
    #[arg(long)]
    preset: PathBuf,
    #[command(flatten)]
    cameras: CameraDir,
    #[command(flatten)]
    overrides: Overrides,
    /// Start time in the input, in seconds.
    #[arg(long, value_name = "SECONDS")]
    start: Option<f64>,
    /// Length of input to render, in seconds; defaults to the rest of the video.
    #[arg(long, value_name = "SECONDS")]
    duration: Option<f64>,
    /// Scale the input down so its longer edge is at most this many pixels.
    #[arg(long, value_name = "PIXELS")]
    max_dimension: Option<u32>,
    /// H.264 quality: small (CRF 28), balanced (CRF 23, the default) or high (CRF 18).
    #[arg(long, value_enum)]
    quality: Option<Quality>,
    #[arg(
        long,
        value_enum,
        default_value_t = CodecArg::H264,
        help = "Output codec: h264, or prores for a ProRes 422 HQ archive in a .mov file"
    )]
    codec: CodecArg,
    /// Output frame rate in frames per second; defaults to the input's.
    #[arg(long, value_name = "FPS")]
    fps: Option<f64>,
}

/// Size against fidelity of H.264 output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Quality {
    /// CRF 28: small files, visible compression.
    Small,
    /// CRF 23: libx264's default balance.
    Balanced,
    /// CRF 18: close to visually lossless.
    High,
}

impl Quality {
    /// Returns the libx264 constant rate factor.
    fn crf(self) -> u8 {
        match self {
            Self::Small => 28,
            Self::Balanced => 23,
            Self::High => 18,
        }
    }
}

/// The codec choices of the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CodecArg {
    /// H.264, 8-bit 4:2:0.
    H264,
    #[value(help = "Apple ProRes 422 HQ, 10-bit 4:2:2")]
    Prores,
}

impl From<CodecArg> for Codec {
    fn from(value: CodecArg) -> Self {
        match value {
            CodecArg::H264 => Self::H264,
            CodecArg::Prores => Self::ProRes,
        }
    }
}

/// Renders the input video frame by frame and encodes the result.
pub(crate) fn run(args: &RenderVideoArgs) -> Result<()> {
    let started = Instant::now();
    let preset = args.overrides.preset(&args.preset)?;
    let camera = load_camera(&args.cameras.camera_dir, &preset.camera)?;
    refuse_overwrite(&args.input, &args.output)?;
    let codec = Codec::from(args.codec);
    if codec == Codec::ProRes && args.quality.is_some() {
        eprintln!("note: --quality applies to h264 only; ProRes is always 422 HQ");
    }

    let mut reader = VideoReader::open(
        &args.input,
        ReadOptions {
            start_s: args.start,
            duration_s: args.duration,
            max_dimension: args.max_dimension,
        },
    )?;
    let mut simulator = Simulator::builder((camera, preset))
        .input_size(reader.width(), reader.height())
        .build()
        .context("cannot build the simulator")?;
    let timing = simulator.timing();
    let mode = timing.mode;
    let (width, height) = (timing.output_width, timing.output_height);
    let (even_w, even_h) = (width & !1, height & !1);
    let crop = (even_w, even_h) != (width, height);
    if crop {
        eprintln!("note: cropping {width}x{height} frames to {even_w}x{even_h} for the encoder");
    }
    let options = WriteOptions {
        fps: args.fps.unwrap_or(reader.info().fps),
        crf: args.quality.unwrap_or(Quality::Balanced).crf(),
        codec,
    };
    let mut writer = VideoWriter::create(&args.output, even_w, even_h, options)?;

    let progress = Progress::new(reader.estimated_frames());
    let mut index = 0;
    while let Some(scene) = reader.next_frame()? {
        let frame = simulator.frame(&scene, index);
        if crop {
            writer.write_frame(&frame_to_even(frame))?;
        } else {
            writer.write_frame(frame)?;
        }
        index += 1;
        progress.update(index);
    }
    if index == 0 {
        drop(writer);
        bail!(
            "{} has no frames in the requested window; check --start and --duration",
            args.input.display()
        );
    }
    writer.finish()?;
    let wall = started.elapsed();
    println!(
        "{} {index} frames {even_w}x{even_h} {mode} {:.1} s ({:.1} fps)",
        args.output.display(),
        wall.as_secs_f64(),
        index as f64 / wall.as_secs_f64()
    );
    Ok(())
}

/// Refuses to write the output over the input, which ffmpeg would truncate while reading.
fn refuse_overwrite(input: &Path, output: &Path) -> Result<()> {
    if let (Ok(input_path), Ok(output_path)) = (fs::canonicalize(input), fs::canonicalize(output))
        && input_path == output_path
    {
        bail!(
            "the output {} is the input video; choose another output path",
            output.display()
        );
    }
    Ok(())
}

/// Progress lines on stderr with the rendering rate and the time left.
struct Progress {
    started: Instant,
    total: Option<u64>,
}

impl Progress {
    fn new(total: Option<u64>) -> Self {
        Self {
            started: Instant::now(),
            total,
        }
    }

    /// Reports after every [`PROGRESS_INTERVAL_FRAMES`] frames.
    fn update(&self, done: u64) {
        if !done.is_multiple_of(PROGRESS_INTERVAL_FRAMES) {
            return;
        }
        let elapsed_s = self.started.elapsed().as_secs_f64();
        let rate = done as f64 / elapsed_s.max(f64::EPSILON);
        match self.total {
            Some(total) if total > done => {
                let left = Duration::from_secs_f64((total - done) as f64 / rate);
                eprintln!("frame {done}/{total}  {rate:.1} fps  ETA {}", clock(left));
            }
            Some(total) => eprintln!("frame {done}/{total}  {rate:.1} fps"),
            None => eprintln!("frame {done}  {rate:.1} fps"),
        }
    }
}

/// Formats a duration as `h:mm:ss`, or `m:ss` under an hour.
fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}
