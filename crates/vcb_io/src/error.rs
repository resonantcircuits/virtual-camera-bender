//! The crate's canonical error type.

use std::backtrace::{Backtrace, BacktraceStatus};
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

/// Error returned when an image or video cannot be read, decoded, encoded or written.
///
/// [`path`](Self::path) names the media file involved; the upstream cause, if any, is
/// available through [`std::error::Error::source`]. `Display` prints a one-line
/// summary, followed by the backtrace when one was captured (set `RUST_BACKTRACE=1`).
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    path: PathBuf,
    detail: Option<String>,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    backtrace: Backtrace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorKind {
    /// The file could not be opened, read, created or written.
    Io,
    /// The file's content is not a supported image.
    Decode,
    /// The frame could not be encoded.
    Encode,
    /// `ffmpeg` or `ffprobe` is not installed or not where the environment points.
    ToolMissing,
    /// `ffmpeg` or `ffprobe` could not be started or exited with an error.
    ToolFailed,
    /// The file has no usable video stream, or its frames do not match the probe.
    Stream,
    /// The caller asked for settings the video pipeline cannot honour.
    InvalidArgument,
}

impl Error {
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        Self::new(ErrorKind::Io, path, None, Some(Box::new(source)))
    }

    pub(crate) fn decode(
        path: &Path,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::new(ErrorKind::Decode, path, None, Some(Box::new(source)))
    }

    pub(crate) fn encode(
        path: &Path,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::new(ErrorKind::Encode, path, None, Some(Box::new(source)))
    }

    /// An external tool is missing; `message` tells the user how to provide it.
    pub(crate) fn tool_missing(path: &Path, message: String) -> Self {
        Self::new(ErrorKind::ToolMissing, path, Some(message), None)
    }

    /// An external tool failed; `message` names the tool and carries its diagnostics.
    pub(crate) fn tool_failed(
        path: &Path,
        message: String,
        source: Option<std::io::Error>,
    ) -> Self {
        let source = source.map(|source| Box::new(source) as _);
        Self::new(ErrorKind::ToolFailed, path, Some(message), source)
    }

    /// The video stream is missing or inconsistent.
    pub(crate) fn stream(path: &Path, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Stream, path, Some(message.into()), None)
    }

    /// The video stream description could not be parsed.
    pub(crate) fn stream_parse(path: &Path, source: serde_json::Error) -> Self {
        Self::new(
            ErrorKind::Stream,
            path,
            Some("ffprobe's report could not be parsed".to_owned()),
            Some(Box::new(source)),
        )
    }

    /// The caller's settings are out of range; `message` says which and why.
    pub(crate) fn invalid_argument(path: &Path, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidArgument, path, Some(message.into()), None)
    }

    fn new(
        kind: ErrorKind,
        path: &Path,
        detail: Option<String>,
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind,
            path: path.to_path_buf(),
            detail,
            source,
            backtrace: Backtrace::capture(),
        }
    }

    /// Returns `true` if the file could not be opened, read, created or written.
    #[must_use]
    pub fn is_io(&self) -> bool {
        self.kind == ErrorKind::Io
    }

    /// Returns `true` if the file is not a decodable JPEG or PNG image.
    #[must_use]
    pub fn is_decode(&self) -> bool {
        self.kind == ErrorKind::Decode
    }

    /// Returns `true` if the frame could not be encoded.
    #[must_use]
    pub fn is_encode(&self) -> bool {
        self.kind == ErrorKind::Encode
    }

    /// Returns `true` if `ffmpeg` or `ffprobe` could not be found.
    ///
    /// The tools are looked up through `VCB_FFMPEG` and `VCB_FFPROBE`, then `PATH`.
    #[must_use]
    pub fn is_tool_missing(&self) -> bool {
        self.kind == ErrorKind::ToolMissing
    }

    /// Returns `true` if `ffmpeg` or `ffprobe` could not start or reported a failure.
    ///
    /// The message includes the tool's exit status and its error output.
    #[must_use]
    pub fn is_tool_failed(&self) -> bool {
        self.kind == ErrorKind::ToolFailed
    }

    /// Returns `true` if the file has no usable video stream or delivered malformed frames.
    #[must_use]
    pub fn is_stream(&self) -> bool {
        self.kind == ErrorKind::Stream
    }

    /// Returns `true` if a size, frame rate, time or quality setting is unusable.
    #[must_use]
    pub fn is_invalid_argument(&self) -> bool {
        self.kind == ErrorKind::InvalidArgument
    }

    /// Returns the path of the file involved.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the backtrace captured when the error was created.
    pub fn backtrace(&self) -> &Backtrace {
        &self.backtrace
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let action = match self.kind {
            ErrorKind::Io => "could not access",
            ErrorKind::Decode => "could not decode image",
            ErrorKind::Encode => "could not encode image",
            ErrorKind::ToolMissing => "cannot process video",
            ErrorKind::ToolFailed => "video tool failed on",
            ErrorKind::Stream => "unusable video stream in",
            ErrorKind::InvalidArgument => "invalid video settings for",
        };
        write!(f, "{action} {}", self.path.display())?;
        if let Some(detail) = &self.detail {
            write!(f, ": {detail}")?;
        }
        if self.backtrace.status() == BacktraceStatus::Captured {
            write!(f, "\n\nbacktrace:\n{}", self.backtrace)?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}
