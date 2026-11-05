//! The crate's canonical error type.

use std::backtrace::{Backtrace, BacktraceStatus};
use std::fmt::{self, Display, Formatter};

/// Error returned when a camera, preset or input cannot be simulated.
///
/// Inspect the cause with [`is_config`](Self::is_config) and
/// [`is_input`](Self::is_input). `Display` prints a one-line summary, followed by the
/// backtrace when one was captured (set `RUST_BACKTRACE=1`).
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    backtrace: Backtrace,
}

#[derive(Debug)]
enum ErrorKind {
    /// A camera or preset value is missing, out of range or inconsistent.
    Config(String),
    /// The input image cannot be mapped onto the virtual sensor.
    Input(String),
}

impl Error {
    /// Creates a configuration error with a message naming the offending field.
    pub(crate) fn config(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Config(message.into()))
    }

    /// Creates an input error with a message describing the input.
    pub(crate) fn input(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Input(message.into()))
    }

    fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            source: None,
            backtrace: Backtrace::capture(),
        }
    }

    /// Returns `true` if a camera or preset value is invalid.
    #[must_use]
    pub fn is_config(&self) -> bool {
        matches!(self.kind, ErrorKind::Config(_))
    }

    /// Returns `true` if the input image cannot be simulated, for example when too small.
    #[must_use]
    pub fn is_input(&self) -> bool {
        matches!(self.kind, ErrorKind::Input(_))
    }

    /// Returns the backtrace captured when the error was created.
    pub fn backtrace(&self) -> &Backtrace {
        &self.backtrace
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Config(message) => write!(f, "invalid configuration: {message}")?,
            ErrorKind::Input(message) => write!(f, "invalid input: {message}")?,
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
