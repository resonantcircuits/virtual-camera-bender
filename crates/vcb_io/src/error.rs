//! The crate's canonical error type.

use std::backtrace::{Backtrace, BacktraceStatus};
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

/// Error returned when an image file cannot be read, decoded or written.
///
/// [`path`](Self::path) names the file; the upstream cause is available through
/// [`std::error::Error::source`]. `Display` prints a one-line summary, followed by the
/// backtrace when one was captured (set `RUST_BACKTRACE=1`).
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    path: PathBuf,
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
}

impl Error {
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        Self::new(ErrorKind::Io, path, Box::new(source))
    }

    pub(crate) fn decode(
        path: &Path,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::new(ErrorKind::Decode, path, Box::new(source))
    }

    pub(crate) fn encode(
        path: &Path,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::new(ErrorKind::Encode, path, Box::new(source))
    }

    fn new(
        kind: ErrorKind,
        path: &Path,
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    ) -> Self {
        Self {
            kind,
            path: path.to_path_buf(),
            source: Some(source),
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
        };
        write!(f, "{action} {}", self.path.display())?;
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
