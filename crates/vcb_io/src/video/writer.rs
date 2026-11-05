//! Encoding frames into a video file through an `ffmpeg` pipe.

use std::ffi::OsString;
use std::fmt::{self, Display, Formatter};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Stdio};

use vcb_core::Frame;

use crate::Error;
use crate::video::process::{Process, Tool, path_arg};

/// Constant rate factor of H.264 output unless the caller chooses another.
///
/// libx264's own default: visually close to the source at moderate file sizes.
pub const DEFAULT_CRF: u8 = 23;

/// Largest constant rate factor libx264 accepts for 8-bit output.
const MAX_CRF: u8 = 51;

/// Video codec of a written file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Codec {
    /// H.264 through libx264, 8-bit 4:2:0, quality set by the CRF; for sharing.
    #[default]
    H264,
    /// Apple `ProRes` 422 HQ through `prores_ks`, 10-bit 4:2:2, in a `.mov` file; for archiving.
    ProRes,
}

impl Display for Codec {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::H264 => "h264",
            Self::ProRes => "prores",
        })
    }
}

/// Frame rate, codec and quality of a written video.
///
/// # Examples
///
/// ```
/// use vcb_io::video::{Codec, WriteOptions};
///
/// let archive = WriteOptions {
///     codec: Codec::ProRes,
///     ..WriteOptions::new(25.0)
/// };
/// assert_eq!(archive.fps, 25.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WriteOptions {
    /// Frame rate in frames per second, constant for the whole file.
    pub fps: f64,
    /// H.264 constant rate factor, 0 (lossless) to 51 (worst); ignored by `ProRes`.
    pub crf: u8,
    /// The codec.
    pub codec: Codec,
}

impl WriteOptions {
    /// Creates options for H.264 at `fps` frames per second and the default CRF.
    #[must_use]
    pub fn new(fps: f64) -> Self {
        Self {
            fps,
            crf: DEFAULT_CRF,
            codec: Codec::H264,
        }
    }
}

/// Writes frames of one size into a video file, through an `ffmpeg` process.
///
/// [`create`](Self::create) checks the settings and starts ffmpeg;
/// [`write_frame`](Self::write_frame) appends frames in order;
/// [`finish`](Self::finish) closes the stream and waits for ffmpeg to complete the
/// file. Dropping the writer without finishing stops ffmpeg and leaves an incomplete
/// file.
///
/// # Examples
///
/// ```no_run
/// use vcb_core::Frame;
/// use vcb_io::video::{VideoWriter, WriteOptions};
///
/// # fn main() -> Result<(), vcb_io::Error> {
/// let mut writer = VideoWriter::create("grey.mp4", 64, 48, WriteOptions::new(10.0))?;
/// for _ in 0..10 {
///     writer.write_frame(&Frame::from_rgb(64, 48, vec![128; 64 * 48 * 3]))?;
/// }
/// writer.finish()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct VideoWriter {
    path: PathBuf,
    width: u32,
    height: u32,
    process: Process,
    stdin: Option<ChildStdin>,
    frames_written: u64,
}

impl VideoWriter {
    /// Starts encoding `width` by `height` frames into `path`, replacing any existing file.
    ///
    /// # Errors
    ///
    /// Returns an invalid-argument error, before starting ffmpeg, if a dimension is
    /// zero or odd (crop or pad the frames first, for example with
    /// [`frame_to_even`](crate::video::frame_to_even)), if the frame rate is not a
    /// positive number, if the CRF exceeds 51, or if `ProRes` is asked for in a file
    /// whose extension is not `.mov`. Returns a missing-tool or tool error if `ffmpeg`
    /// cannot be found or started.
    pub fn create(
        path: impl AsRef<Path>,
        width: u32,
        height: u32,
        options: WriteOptions,
    ) -> Result<Self, Error> {
        let path = path.as_ref();
        validate(path, width, height, &options)?;
        let mut args: Vec<OsString> = ["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24"]
            .map(OsString::from)
            .into();
        args.extend([
            "-s".into(),
            format!("{width}x{height}").into(),
            "-r".into(),
            options.fps.to_string().into(),
            "-i".into(),
            "-".into(),
        ]);
        match options.codec {
            Codec::H264 => args.extend([
                "-c:v".into(),
                "libx264".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-crf".into(),
                options.crf.to_string().into(),
            ]),
            Codec::ProRes => args.extend(
                [
                    "-c:v",
                    "prores_ks",
                    "-profile:v",
                    "3",
                    "-pix_fmt",
                    "yuv422p10le",
                ]
                .map(OsString::from),
            ),
        }
        args.extend(["-movflags".into(), "+faststart".into(), path_arg(path)]);
        let mut process = Process::spawn(Tool::Ffmpeg, path, &args, Stdio::piped(), Stdio::null())?;
        let stdin = process.take_stdin();
        tracing::debug!(
            name: "io.video.write.open",
            path = %path.display(),
            width,
            height,
            fps = options.fps,
            codec = %options.codec,
            "video writer started"
        );
        Ok(Self {
            path: path.to_path_buf(),
            width,
            height,
            process,
            stdin,
            frames_written: 0,
        })
    }

    /// Appends `frame` to the video.
    ///
    /// # Errors
    ///
    /// Returns an invalid-argument error if the frame's size differs from the size
    /// given to [`create`](Self::create). Returns a tool error, quoting ffmpeg's error
    /// output, if ffmpeg has stopped accepting frames, and an I/O error if the pipe
    /// fails for another reason. After an error the writer accepts no more frames.
    pub fn write_frame(&mut self, frame: &Frame) -> Result<(), Error> {
        if (frame.width(), frame.height()) != (self.width, self.height) {
            return Err(Error::invalid_argument(
                &self.path,
                format!(
                    "frame {} is {} x {} but the video is {} x {}",
                    self.frames_written,
                    frame.width(),
                    frame.height(),
                    self.width,
                    self.height
                ),
            ));
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(Error::tool_failed(
                &self.path,
                "ffmpeg has already stopped after an earlier error".to_owned(),
                None,
            ));
        };
        if let Err(source) = stdin.write_all(frame.rgb()) {
            drop(self.stdin.take());
            self.process.finish(&self.path)?;
            return Err(Error::io(&self.path, source));
        }
        self.frames_written += 1;
        Ok(())
    }

    /// Closes the stream, waits for ffmpeg to complete the file and reports its outcome.
    ///
    /// # Errors
    ///
    /// Returns a tool error, quoting ffmpeg's error output, if ffmpeg exits with a
    /// failure.
    pub fn finish(mut self) -> Result<(), Error> {
        drop(self.stdin.take());
        self.process.finish(&self.path)?;
        tracing::debug!(
            name: "io.video.write.done",
            path = %self.path.display(),
            frames = self.frames_written,
            "video written"
        );
        Ok(())
    }

    /// Returns the frame width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the frame height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the number of frames written so far.
    #[must_use]
    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }
}

/// Rejects settings that the encoders cannot honour.
fn validate(path: &Path, width: u32, height: u32, options: &WriteOptions) -> Result<(), Error> {
    if width == 0 || height == 0 || width % 2 == 1 || height % 2 == 1 {
        return Err(Error::invalid_argument(
            path,
            format!(
                "{width} x {height} frames cannot be encoded: chroma subsampling needs a \
                 non-zero, even width and height, so crop or pad the frames first (for \
                 example with vcb_io::video::frame_to_even)"
            ),
        ));
    }
    if !(options.fps.is_finite() && options.fps > 0.0) {
        return Err(Error::invalid_argument(
            path,
            format!(
                "frame rate must be a positive number of frames per second, got {}",
                options.fps
            ),
        ));
    }
    match options.codec {
        Codec::H264 if options.crf > MAX_CRF => Err(Error::invalid_argument(
            path,
            format!("H.264 CRF must be 0 to {MAX_CRF}, got {}", options.crf),
        )),
        Codec::ProRes
            if !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("mov")) =>
        {
            Err(Error::invalid_argument(
                path,
                "ProRes is written into QuickTime files only; use a .mov extension",
            ))
        }
        Codec::H264 | Codec::ProRes => Ok(()),
    }
}

/// Returns `frame` cropped to an even width and height, as video encoders require.
///
/// An odd dimension loses its last column or row, the closest a one-pixel crop comes
/// to a centre crop. Even frames are returned as an unchanged copy; a dimension of 1
/// becomes 0.
///
/// # Examples
///
/// ```
/// use vcb_core::Frame;
/// use vcb_io::video::frame_to_even;
///
/// let even = frame_to_even(&Frame::new(321, 241));
/// assert_eq!((even.width(), even.height()), (320, 240));
/// ```
#[must_use]
pub fn frame_to_even(frame: &Frame) -> Frame {
    let (width, height) = (frame.width(), frame.height());
    let (even_w, even_h) = (width & !1, height & !1);
    if (even_w, even_h) == (width, height) {
        return frame.clone();
    }
    if even_w == 0 || even_h == 0 {
        return Frame::new(even_w, even_h);
    }
    let (x0, y0) = ((width - even_w) / 2, (height - even_h) / 2);
    let (src_stride, dst_stride) = (width as usize * 3, even_w as usize * 3);
    let mut rgb = Vec::with_capacity(dst_stride * even_h as usize);
    for row in frame
        .rgb()
        .chunks_exact(src_stride)
        .skip(y0 as usize)
        .take(even_h as usize)
    {
        let start = x0 as usize * 3;
        rgb.extend_from_slice(&row[start..start + dst_stride]);
    }
    Frame::from_rgb(even_w, even_h, rgb)
}
