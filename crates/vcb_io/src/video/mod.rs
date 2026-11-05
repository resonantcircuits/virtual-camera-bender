//! Video input and output through `ffmpeg` and `ffprobe` pipes.
//!
//! The crate does not link ffmpeg. It runs the command-line tools as child processes
//! and exchanges raw frames with them over pipes, so any format the installed ffmpeg
//! reads can be simulated, and H.264 or `ProRes` files come out.
//!
//! - [`probe_video`] reads a file's size, frame rate, frame count and duration.
//! - [`VideoReader`] decodes a file frame by frame into linear-light
//!   [`Scene`](vcb_core::Scene)s for a stateful [`Simulator`](vcb_core::Simulator).
//! - [`VideoWriter`] encodes camera [`Frame`](vcb_core::Frame)s into a file.
//! - [`frame_to_even`] crops a frame to the even size the encoders need.
//!
//! # Locating the tools
//!
//! `ffmpeg` is the executable named by the `VCB_FFMPEG` environment variable if it is
//! set, else the first `ffmpeg` on `PATH`; `ffprobe` likewise with `VCB_FFPROBE`. The
//! tools are located again for every file, so a change of the environment takes
//! effect at once.
//!
//! # Invocations
//!
//! [`probe_video`] runs, and parses the JSON report:
//!
//! ```text
//! ffprobe -v error -select_streams v:0
//!     -show_entries stream=width,height,r_frame_rate,avg_frame_rate,nb_frames,duration:stream_side_data=rotation:format=duration
//!     -of json <path>
//! ```
//!
//! The frame rate is `r_frame_rate`, unless it exceeds 210 fps while `avg_frame_rate`
//! is below 70 fps, as ffmpeg itself decides. A display rotation of a quarter turn
//! exchanges width and height. The duration is the stream's, else the container's.
//!
//! [`VideoReader::open`] probes the file, then runs
//!
//! ```text
//! ffmpeg -v error [-ss <start_s>] -i <path> [-t <duration_s>]
//!     [-vf scale=<width>:<height>:flags=area] -f rawvideo -pix_fmt rgb24 -
//! ```
//!
//! with `-ss` only for a start after zero, `-t` only for a requested duration, and the
//! scale filter only when the picture's longer edge exceeds
//! [`ReadOptions::max_dimension`]; the scaled size keeps the aspect ratio and has even
//! edges. ffmpeg's default autorotation stays on, so frames arrive upright at the
//! probed size, and its default frame-rate conversion delivers a constant rate: a
//! variable-rate source has frames repeated or dropped to the probed rate.
//!
//! [`VideoWriter::create`] runs, for H.264,
//!
//! ```text
//! ffmpeg -v error -y -f rawvideo -pix_fmt rgb24 -s <width>x<height> -r <fps> -i -
//!     -c:v libx264 -pix_fmt yuv420p -crf <crf> -movflags +faststart <path>
//! ```
//!
//! and for `ProRes` the same with `-c:v prores_ks -profile:v 3 -pix_fmt yuv422p10le` in
//! place of the three H.264 options. A path that begins with `-` is passed as
//! `./<path>`. While a reader or writer runs, a helper thread collects up to 16 KiB of
//! ffmpeg's error output so that ffmpeg never blocks on it.
//!
//! # Frame format
//!
//! Frames cross the pipes as `rgb24`: 8-bit sRGB-encoded `R, G, B` triplets, rows top
//! to bottom, no padding, `width * height * 3` bytes per frame. The reader decodes
//! each frame into linear light with [`Scene::from_srgb8`](vcb_core::Scene::from_srgb8);
//! the writer sends a [`Frame`](vcb_core::Frame)'s bytes unchanged. The conversion
//! between YUV and RGB is ffmpeg's default for the stream.
//!
//! # Failure modes
//!
//! Every failure is a [`crate::Error`] naming the media file:
//!
//! - [`is_tool_missing`](crate::Error::is_tool_missing): a tool is not on `PATH`, or
//!   its environment variable names something that is not an executable file. The
//!   message says how to install ffmpeg or set the variable.
//! - [`is_io`](crate::Error::is_io): the input file does not exist (checked before
//!   starting any tool), or a pipe failed.
//! - [`is_tool_failed`](crate::Error::is_tool_failed): a tool could not start or exited
//!   with a failure, for example on an unreadable file, an unwritable output or an
//!   encoder the installed ffmpeg lacks. The message quotes the exit code and the
//!   first lines of the tool's error output. Error output of a tool that succeeds is
//!   logged as an `io.video.stderr` warning instead.
//! - [`is_stream`](crate::Error::is_stream): the file has no video stream, ffprobe
//!   reports no size or frame rate, or ffmpeg's output ends in the middle of a frame.
//! - [`is_invalid_argument`](crate::Error::is_invalid_argument): a negative start, a
//!   non-positive duration or frame rate, a zero or odd frame size, a CRF above 51,
//!   `ProRes` outside a `.mov` file, or a frame whose size differs from the writer's.
//!   These are checked before any tool starts, except the last.
//!
//! A reader dropped before its last frame, and a writer dropped without
//! [`VideoWriter::finish`], kill their ffmpeg process; the writer's file is then
//! incomplete.
//!
//! # Examples
//!
//! ```no_run
//! use vcb_core::Frame;
//! use vcb_io::video::{ReadOptions, VideoReader, VideoWriter, WriteOptions, frame_to_even};
//!
//! # fn main() -> Result<(), vcb_io::Error> {
//! let options = ReadOptions { max_dimension: Some(640), ..ReadOptions::default() };
//! let mut reader = VideoReader::open("in.mp4", options)?;
//! let (width, height) = (reader.width() & !1, reader.height() & !1);
//! let mut writer = VideoWriter::create("out.mp4", width, height, WriteOptions::new(reader.info().fps))?;
//! while let Some(scene) = reader.next_frame()? {
//!     let frame = Frame::from_rgb(scene.width(), scene.height(), scene.to_srgb8());
//!     writer.write_frame(&frame_to_even(&frame))?;
//! }
//! writer.finish()?;
//! # Ok(())
//! # }
//! ```

mod probe;
mod process;
mod reader;
mod writer;

#[doc(inline)]
pub use probe::{VideoInfo, probe_video};
#[doc(inline)]
pub use reader::{ReadOptions, VideoReader};
#[doc(inline)]
pub use writer::{Codec, DEFAULT_CRF, VideoWriter, WriteOptions, frame_to_even};
