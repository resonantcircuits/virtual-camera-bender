//! Decoding a video into scenes through an `ffmpeg` pipe.

use std::ffi::OsString;
use std::fmt::{self, Debug, Formatter};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{ChildStdout, Stdio};

use vcb_core::Scene;

use crate::Error;
use crate::video::process::{Process, Tool, path_arg};
use crate::video::{VideoInfo, probe_video};

/// Which part of a video to read, and at what size.
///
/// The default reads every frame at the stored size.
///
/// # Examples
///
/// ```
/// use vcb_io::video::ReadOptions;
///
/// let preview = ReadOptions {
///     duration_s: Some(5.0),
///     max_dimension: Some(640),
///     ..ReadOptions::default()
/// };
/// assert_eq!(preview.start_s, None);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReadOptions {
    /// Time in seconds to seek to before the first frame; `None` starts at the beginning.
    pub start_s: Option<f64>,
    /// Seconds of video to read; `None` reads to the end.
    pub duration_s: Option<f64>,
    /// Longest edge in pixels; larger videos are scaled down to it, keeping the aspect ratio.
    pub max_dimension: Option<u32>,
}

/// Reads a video frame by frame as linear-light scenes, through an `ffmpeg` process.
///
/// [`open`](Self::open) probes the file and starts ffmpeg; [`next_frame`](Self::next_frame)
/// returns the frames in order and `None` at the end. Every frame has the size given by
/// [`width`](Self::width) and [`height`](Self::height). Dropping the reader before the
/// end stops ffmpeg.
///
/// # Examples
///
/// ```no_run
/// use vcb_io::video::{ReadOptions, VideoReader};
///
/// # fn main() -> Result<(), vcb_io::Error> {
/// let mut reader = VideoReader::open("clip.mp4", ReadOptions::default())?;
/// let mut frames = 0;
/// while let Some(scene) = reader.next_frame()? {
///     assert_eq!(scene.width(), reader.width());
///     frames += 1;
/// }
/// println!("{frames} frames at {} fps", reader.info().fps);
/// # Ok(())
/// # }
/// ```
pub struct VideoReader {
    path: PathBuf,
    info: VideoInfo,
    options: ReadOptions,
    width: u32,
    height: u32,
    process: Process,
    stdout: ChildStdout,
    buffer: Vec<u8>,
    frames_read: u64,
    ended: bool,
}

impl VideoReader {
    /// Probes `path` and starts decoding it with `options`.
    ///
    /// # Errors
    ///
    /// Returns an invalid-argument error for a negative or non-finite start, a
    /// non-positive duration or a maximum dimension below 2, before any process is
    /// started. Otherwise returns the errors of [`probe_video`], and a missing-tool or
    /// tool error if `ffmpeg` cannot be found or started.
    pub fn open(path: impl AsRef<Path>, options: ReadOptions) -> Result<Self, Error> {
        let path = path.as_ref();
        validate(path, &options)?;
        let info = probe_video(path)?;
        let (width, height) = match options.max_dimension {
            Some(max) => fit_within(info.width, info.height, max),
            None => (info.width, info.height),
        };
        let mut args: Vec<OsString> = vec!["-v".into(), "error".into()];
        if let Some(start_s) = options.start_s.filter(|&start_s| start_s > 0.0) {
            args.extend(["-ss".into(), start_s.to_string().into()]);
        }
        args.extend(["-i".into(), path_arg(path)]);
        if let Some(duration_s) = options.duration_s {
            args.extend(["-t".into(), duration_s.to_string().into()]);
        }
        if (width, height) != (info.width, info.height) {
            args.extend([
                "-vf".into(),
                format!("scale={width}:{height}:flags=area").into(),
            ]);
        }
        args.extend(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"].map(OsString::from));
        let mut process = Process::spawn(Tool::Ffmpeg, path, &args, Stdio::null(), Stdio::piped())?;
        let stdout = process.take_stdout().ok_or_else(|| {
            Error::io(
                path,
                std::io::Error::other("ffmpeg's standard output is not a pipe"),
            )
        })?;
        tracing::debug!(
            name: "io.video.read.open",
            path = %path.display(),
            width,
            height,
            fps = info.fps,
            "video reader started"
        );
        Ok(Self {
            path: path.to_path_buf(),
            info,
            options,
            width,
            height,
            process,
            stdout,
            buffer: vec![0; width as usize * height as usize * 3],
            frames_read: 0,
            ended: false,
        })
    }

    /// Returns the next frame as a linear-light scene, or `None` after the last frame.
    ///
    /// # Errors
    ///
    /// Returns a tool error if ffmpeg fails while decoding, quoting its error output,
    /// a stream error if its output ends in the middle of a frame, and an I/O error if
    /// the pipe cannot be read. After the end or an error, every call returns `None`.
    pub fn next_frame(&mut self) -> Result<Option<Scene>, Error> {
        if self.ended {
            return Ok(None);
        }
        let filled = match fill(&mut self.stdout, &mut self.buffer) {
            Ok(filled) => filled,
            Err(source) => {
                self.ended = true;
                self.process.finish(&self.path)?;
                return Err(Error::io(&self.path, source));
            }
        };
        if filled == self.buffer.len() {
            self.frames_read += 1;
            return Ok(Some(Scene::from_srgb8(
                self.width,
                self.height,
                &self.buffer,
            )));
        }
        self.ended = true;
        self.process.finish(&self.path)?;
        if filled > 0 {
            return Err(Error::stream(
                &self.path,
                format!(
                    "ffmpeg's output ended {filled} bytes into frame {}, which needs {} bytes \
                     at {} x {}",
                    self.frames_read,
                    self.buffer.len(),
                    self.width,
                    self.height
                ),
            ));
        }
        tracing::debug!(
            name: "io.video.read.done",
            path = %self.path.display(),
            frames = self.frames_read,
            "video reader finished"
        );
        Ok(None)
    }

    /// Returns the stream parameters of the source file.
    #[must_use]
    pub fn info(&self) -> &VideoInfo {
        &self.info
    }

    /// Returns the width of the delivered frames in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the height of the delivered frames in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Estimates how many frames the reader delivers, for progress reports.
    ///
    /// Derived from the probed duration, frame count and rate and the requested time
    /// window; `None` if the source reports neither a duration nor a frame count and
    /// no duration was requested. The actual count may differ by a frame or two.
    #[must_use]
    pub fn estimated_frames(&self) -> Option<u64> {
        let fps = self.info.fps;
        let start_s = self.options.start_s.unwrap_or(0.0);
        if start_s <= 0.0 && self.options.duration_s.is_none() && self.info.frames.is_some() {
            return self.info.frames;
        }
        let length_s = self
            .info
            .duration_s
            .or_else(|| self.info.frames.map(|frames| frames as f64 / fps));
        let remaining_s = length_s.map(|length_s| (length_s - start_s).max(0.0));
        let window_s = match (remaining_s, self.options.duration_s) {
            (Some(remaining_s), Some(duration_s)) => Some(remaining_s.min(duration_s)),
            (remaining_s, duration_s) => remaining_s.or(duration_s),
        };
        window_s.map(|window_s| (window_s * fps).round() as u64)
    }
}

impl Debug for VideoReader {
    /// Prints the source and progress; the frame buffer holds megabytes.
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("VideoReader")
            .field("path", &self.path)
            .field("info", &self.info)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("frames_read", &self.frames_read)
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

/// Rejects read options that ffmpeg would misinterpret.
fn validate(path: &Path, options: &ReadOptions) -> Result<(), Error> {
    if let Some(start_s) = options.start_s
        && !(start_s.is_finite() && start_s >= 0.0)
    {
        return Err(Error::invalid_argument(
            path,
            format!("start time must be zero or more seconds, got {start_s}"),
        ));
    }
    if let Some(duration_s) = options.duration_s
        && !(duration_s.is_finite() && duration_s > 0.0)
    {
        return Err(Error::invalid_argument(
            path,
            format!("duration must be more than zero seconds, got {duration_s}"),
        ));
    }
    if let Some(max) = options.max_dimension
        && max < 2
    {
        return Err(Error::invalid_argument(
            path,
            format!("maximum dimension must be at least 2 pixels, got {max}"),
        ));
    }
    Ok(())
}

/// Scales `width` by `height` down so the longer edge is at most `max`, both edges even.
///
/// Sizes that already fit are returned unchanged, even if odd.
fn fit_within(width: u32, height: u32, max: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= max {
        return (width, height);
    }
    let long_out = max & !1;
    let scale = f64::from(long_out) / f64::from(long);
    let short = width.min(height);
    let short_out = (((f64::from(short) * scale) / 2.0).round() as u32 * 2).clamp(2, long_out);
    if width >= height {
        (long_out, short_out)
    } else {
        (short_out, long_out)
    }
}

/// Reads until `buffer` is full or the stream ends; returns the bytes read.
fn fill(reader: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_keeps_the_aspect_ratio_with_even_edges() {
        assert_eq!(fit_within(320, 240, 160), (160, 120));
        assert_eq!(fit_within(240, 320, 160), (120, 160));
        assert_eq!(fit_within(1920, 1080, 641), (640, 360));
        assert_eq!(fit_within(1001, 1001, 161), (160, 160));
        assert_eq!(fit_within(4000, 10, 100), (100, 2));
        assert_eq!(fit_within(321, 241, 400), (321, 241));
    }

    #[test]
    fn fill_stops_at_the_end_of_the_stream() {
        let mut buffer = [0_u8; 8];
        assert_eq!(fill(&mut &[1_u8, 2, 3][..], &mut buffer).unwrap(), 3);
        assert_eq!(fill(&mut &[0_u8; 20][..], &mut buffer).unwrap(), 8);
    }
}
