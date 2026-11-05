//! Reading a video's stream parameters with `ffprobe`.

use std::ffi::OsString;
use std::fmt::{self, Display, Formatter};
use std::path::Path;

use serde::Deserialize;

use crate::Error;
use crate::video::process::{Tool, path_arg};

/// Real base frame rates above this many frames per second are treated as timebase artefacts.
///
/// Mirrors `av_guess_frame_rate` in libavformat, which ffmpeg uses to choose the rate
/// at which it delivers frames; the probe must agree with the frames delivered.
const IMPLAUSIBLE_BASE_FPS: f64 = 210.0;

/// Average frame rates below this many frames per second are trusted over an implausible base rate.
///
/// The second threshold of libavformat's `av_guess_frame_rate`.
const PLAUSIBLE_AVERAGE_FPS: f64 = 70.0;

/// Largest deviation in degrees from a quarter turn that ffmpeg still autorotates as one.
///
/// The tolerance ffmpeg applies to the display matrix before inserting a transpose.
const ROTATION_TOLERANCE_DEG: f64 = 1.0;

/// Stream parameters of a video file, as a player displays it.
///
/// Read with [`probe_video`]. The size is the displayed size: a stream the container
/// marks as rotated by a quarter turn has width and height exchanged, as ffmpeg turns
/// its frames upright when decoding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoInfo {
    /// Width of the upright picture in pixels.
    pub width: u32,
    /// Height of the upright picture in pixels.
    pub height: u32,
    /// Frame rate in frames per second at which ffmpeg delivers the frames.
    pub fps: f64,
    /// Number of frames the container records, if it records one.
    pub frames: Option<u64>,
    /// Duration of the stream in seconds, else of the container, if either is known.
    pub duration_s: Option<f64>,
}

impl Display for VideoInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} x {} px, {:.3} fps",
            self.width, self.height, self.fps
        )?;
        if let Some(frames) = self.frames {
            write!(f, ", {frames} frames")?;
        }
        if let Some(duration_s) = self.duration_s {
            write!(f, ", {duration_s:.3} s")?;
        }
        Ok(())
    }
}

/// Reads the size, frame rate, frame count and duration of a video's first video stream.
///
/// Runs `ffprobe`; see the [module documentation](crate::video) for the exact command.
///
/// # Errors
///
/// Returns an I/O error if the file does not exist, a missing-tool error if `ffprobe`
/// cannot be found, a tool error if `ffprobe` rejects the file, and a stream error if
/// the file has no video stream or no usable size or frame rate.
pub fn probe_video(path: impl AsRef<Path>) -> Result<VideoInfo, Error> {
    let path = path.as_ref();
    std::fs::metadata(path).map_err(|source| Error::io(path, source))?;
    let mut args: Vec<OsString> = [
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,r_frame_rate,avg_frame_rate,nb_frames,duration\
         :stream_side_data=rotation:format=duration",
        "-of",
        "json",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.push(path_arg(path));
    let report = Tool::Ffprobe.run(path, &args)?;
    let info = parse(path, &report)?;
    tracing::debug!(
        name: "io.video.probe",
        path = %path.display(),
        width = info.width,
        height = info.height,
        fps = info.fps,
        frames = info.frames,
        duration_s = info.duration_s,
        "video probed"
    );
    Ok(info)
}

/// The parts of ffprobe's JSON report that [`probe_video`] asks for.
#[derive(Debug, Deserialize)]
struct Report {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Container>,
}

#[derive(Debug, Deserialize)]
struct Stream {
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    nb_frames: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    side_data_list: Vec<SideData>,
}

#[derive(Debug, Deserialize)]
struct SideData {
    rotation: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct Container {
    duration: Option<String>,
}

/// Turns ffprobe's JSON report into a [`VideoInfo`].
fn parse(path: &Path, json: &[u8]) -> Result<VideoInfo, Error> {
    let report: Report =
        serde_json::from_slice(json).map_err(|source| Error::stream_parse(path, source))?;
    let stream = report
        .streams
        .into_iter()
        .next()
        .ok_or_else(|| Error::stream(path, "the file has no video stream"))?;
    let (width, height) = match (stream.width, stream.height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => (width, height),
        _ => return Err(Error::stream(path, "ffprobe reported no picture size")),
    };
    let fps = frame_rate(
        stream.r_frame_rate.as_deref(),
        stream.avg_frame_rate.as_deref(),
    )
    .ok_or_else(|| Error::stream(path, "ffprobe reported no frame rate"))?;
    let rotation_deg = stream
        .side_data_list
        .iter()
        .find_map(|side| side.rotation)
        .unwrap_or(0.0);
    let (width, height) = if is_quarter_turn(rotation_deg) {
        (height, width)
    } else {
        (width, height)
    };
    let frames = stream
        .nb_frames
        .as_deref()
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|&frames| frames > 0);
    let duration_s = stream
        .duration
        .as_deref()
        .and_then(seconds)
        .or_else(|| report.format?.duration.as_deref().and_then(seconds));
    Ok(VideoInfo {
        width,
        height,
        fps,
        frames,
        duration_s,
    })
}

/// Chooses the frame rate the way ffmpeg's `av_guess_frame_rate` does.
///
/// The real base rate, unless it is implausibly high while the average rate is an
/// ordinary video rate, as for streams with a fine timebase.
fn frame_rate(base: Option<&str>, average: Option<&str>) -> Option<f64> {
    let base = base.and_then(ratio);
    let average = average.and_then(ratio);
    match (base, average) {
        (Some(base), Some(average))
            if base > IMPLAUSIBLE_BASE_FPS && average < PLAUSIBLE_AVERAGE_FPS =>
        {
            Some(average)
        }
        (Some(base), _) => Some(base),
        (None, average) => average,
    }
}

/// Parses `"num/den"` or a plain number; `None` unless positive and finite.
fn ratio(text: &str) -> Option<f64> {
    let (num, den) = text.split_once('/').unwrap_or((text, "1"));
    let value = num.trim().parse::<f64>().ok()? / den.trim().parse::<f64>().ok()?;
    (value.is_finite() && value > 0.0).then_some(value)
}

/// Parses a duration in seconds; `None` unless positive and finite.
fn seconds(text: &str) -> Option<f64> {
    text.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
}

/// Returns `true` if a display rotation turns the picture by 90 or 270 degrees.
fn is_quarter_turn(rotation_deg: f64) -> bool {
    let turned = rotation_deg.rem_euclid(360.0);
    (turned - 90.0).abs() < ROTATION_TOLERANCE_DEG
        || (turned - 270.0).abs() < ROTATION_TOLERANCE_DEG
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(json: &str) -> Result<VideoInfo, Error> {
        parse(Path::new("clip.mp4"), json.as_bytes())
    }

    #[test]
    fn a_quarter_turn_exchanges_width_and_height() {
        let rotated = info(
            r#"{"streams": [{"width": 320, "height": 240, "r_frame_rate": "10/1",
                "avg_frame_rate": "10/1", "side_data_list": [{"rotation": -90}]}]}"#,
        )
        .unwrap();
        assert_eq!((rotated.width, rotated.height), (240, 320));
        let half = info(
            r#"{"streams": [{"width": 320, "height": 240, "r_frame_rate": "10/1",
                "side_data_list": [{}, {"rotation": 180}]}]}"#,
        )
        .unwrap();
        assert_eq!((half.width, half.height), (320, 240));
    }

    #[test]
    fn missing_counts_fall_back_to_the_container_duration() {
        let mkv = info(
            r#"{"streams": [{"width": 64, "height": 48, "r_frame_rate": "30000/1001",
                "avg_frame_rate": "30000/1001"}], "format": {"duration": "2.002000"}}"#,
        )
        .unwrap();
        assert_eq!(mkv.frames, None);
        assert_eq!(mkv.duration_s, Some(2.002));
        assert!((mkv.fps - 29.970_03).abs() < 1e-4, "{}", mkv.fps);
    }

    #[test]
    fn an_implausible_base_rate_yields_to_the_average() {
        assert_eq!(frame_rate(Some("90000/1"), Some("25/1")), Some(25.0));
        assert_eq!(frame_rate(Some("240/1"), Some("120/1")), Some(240.0));
        assert_eq!(frame_rate(Some("0/0"), Some("24/1")), Some(24.0));
        assert_eq!(frame_rate(Some("0/0"), Some("0/0")), None);
    }

    #[test]
    fn a_file_without_video_is_a_stream_error() {
        let error = info(r#"{"streams": []}"#).unwrap_err();
        assert!(error.is_stream(), "{error}");
        let error = info("not json").unwrap_err();
        assert!(error.is_stream(), "{error}");
    }
}
