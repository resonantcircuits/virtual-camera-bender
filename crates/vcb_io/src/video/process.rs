//! Locating and running `ffmpeg` and `ffprobe`.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};

use crate::Error;

/// Most bytes of a tool's error output kept for diagnostics.
///
/// With `-v error` ffmpeg prints a few lines per problem; a damaged file can print
/// one line per frame, and the first lines name the cause.
const STDERR_LIMIT_BYTES: usize = 16 * 1024;

/// Most lines of a tool's error output quoted in an error message.
const STDERR_QUOTED_LINES: usize = 6;

/// One of the two ffmpeg command-line tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tool {
    /// The transcoder, for decoding and encoding frames.
    Ffmpeg,
    /// The analyser, for reading stream parameters.
    Ffprobe,
}

impl Tool {
    /// Returns the executable's base name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
        }
    }

    /// Returns the environment variable that overrides the `PATH` search.
    fn env_var(self) -> &'static str {
        match self {
            Self::Ffmpeg => "VCB_FFMPEG",
            Self::Ffprobe => "VCB_FFPROBE",
        }
    }

    /// Finds the executable: the environment variable if set, else the first on `PATH`.
    ///
    /// `subject` is the media file the tool is needed for; it names the error.
    pub(crate) fn locate(self, subject: &Path) -> Result<PathBuf, Error> {
        let (name, var) = (self.name(), self.env_var());
        if let Some(value) = std::env::var_os(var).filter(|value| !value.is_empty()) {
            let path = PathBuf::from(value);
            if is_executable(&path) {
                return Ok(path);
            }
            return Err(Error::tool_missing(
                subject,
                format!(
                    "{var} is set to {} but that is not an executable file; point it at the \
                     {name} executable or unset it to search PATH",
                    path.display()
                ),
            ));
        }
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths)
            .map(|dir| dir.join(&file))
            .find(|candidate| is_executable(candidate))
            .ok_or_else(|| {
                Error::tool_missing(
                    subject,
                    format!(
                        "{name} was not found on PATH; install FFmpeg (macOS: `brew install \
                         ffmpeg`, Debian or Ubuntu: `sudo apt install ffmpeg`, others: \
                         https://ffmpeg.org/download.html) or set {var} to the {name} executable"
                    ),
                )
            })
    }

    /// Runs the tool to completion and returns its standard output.
    pub(crate) fn run(self, subject: &Path, args: &[OsString]) -> Result<Vec<u8>, Error> {
        let program = self.locate(subject)?;
        tracing::debug!(name: "io.video.run", tool = self.name(), ?args, "running video tool");
        let output = Command::new(&program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| self.start_failed(subject, &program, source))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        check_exit(self, subject, output.status, stderr.trim())?;
        Ok(output.stdout)
    }

    fn start_failed(self, subject: &Path, program: &Path, source: std::io::Error) -> Error {
        Error::tool_failed(
            subject,
            format!(
                "{} could not be started from {}",
                self.name(),
                program.display()
            ),
            Some(source),
        )
    }
}

/// A running tool whose error output is drained on a helper thread.
///
/// Draining keeps the tool from blocking on a full stderr pipe while the caller is
/// busy with stdin or stdout. Dropping a process that was not
/// [`finish`](Self::finish)ed kills it.
#[derive(Debug)]
pub(crate) struct Process {
    tool: Tool,
    child: Child,
    stderr: Option<JoinHandle<String>>,
    reaped: bool,
}

impl Process {
    /// Starts `tool` with `args`, standard input and output as given, stderr captured.
    pub(crate) fn spawn(
        tool: Tool,
        subject: &Path,
        args: &[OsString],
        stdin: Stdio,
        stdout: Stdio,
    ) -> Result<Self, Error> {
        let program = tool.locate(subject)?;
        tracing::debug!(name: "io.video.spawn", tool = tool.name(), ?args, "starting video tool");
        let mut child = Command::new(&program)
            .args(args)
            .stdin(stdin)
            .stdout(stdout)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| tool.start_failed(subject, &program, source))?;
        let stderr = match child.stderr.take().map(drain).transpose() {
            Ok(stderr) => stderr,
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::io(subject, source));
            }
        };
        Ok(Self {
            tool,
            child,
            stderr,
            reaped: false,
        })
    }

    /// Takes the tool's standard input, if it was piped and not taken before.
    pub(crate) fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    /// Takes the tool's standard output, if it was piped and not taken before.
    pub(crate) fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    /// Waits for the tool to exit; a failure becomes an error quoting its output.
    ///
    /// The caller closes standard input first, or the tool may wait for more data.
    pub(crate) fn finish(&mut self, subject: &Path) -> Result<(), Error> {
        let status = self.child.wait();
        self.reaped = true;
        let stderr = self
            .stderr
            .take()
            .and_then(|drained| drained.join().ok())
            .unwrap_or_default();
        let status = status.map_err(|source| {
            Error::tool_failed(
                subject,
                format!("{} could not be waited for", self.tool.name()),
                Some(source),
            )
        })?;
        check_exit(self.tool, subject, status, &stderr)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if !self.reaped {
            // An abandoned reader or writer: stop the tool instead of leaving it
            // blocked on a pipe. Failures mean it has exited already.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Collects up to [`STDERR_LIMIT_BYTES`] of `stderr` on a new thread until it closes.
fn drain(mut stderr: ChildStderr) -> std::io::Result<JoinHandle<String>> {
    thread::Builder::new()
        .name("vcb-ffmpeg-stderr".to_owned())
        .spawn(move || {
            let mut kept = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                match stderr.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let room = STDERR_LIMIT_BYTES.saturating_sub(kept.len());
                        kept.extend_from_slice(&chunk[..n.min(room)]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            String::from_utf8_lossy(&kept).trim().to_owned()
        })
}

/// Turns a tool's exit into a result; error output of a successful run is logged.
fn check_exit(tool: Tool, subject: &Path, status: ExitStatus, stderr: &str) -> Result<(), Error> {
    if status.success() {
        if !stderr.is_empty() {
            tracing::warn!(
                name: "io.video.stderr",
                tool = tool.name(),
                path = %subject.display(),
                stderr,
                "video tool reported errors but succeeded"
            );
        }
        return Ok(());
    }
    let how = match status.code() {
        Some(code) => format!("exited with code {code}"),
        None => format!("was stopped ({status})"),
    };
    Err(Error::tool_failed(
        subject,
        format!("{} {how}: {}", tool.name(), quote(stderr)),
        None,
    ))
}

/// Condenses a tool's error output into one line for an error message.
fn quote(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return "no error output".to_owned();
    }
    let quoted = lines[..lines.len().min(STDERR_QUOTED_LINES)].join("; ");
    match lines.len().checked_sub(STDERR_QUOTED_LINES) {
        Some(more) if more > 0 => format!("{quoted}; ({more} more lines)"),
        _ => quoted,
    }
}

/// Returns a path as a command-line argument that ffmpeg cannot mistake for an option.
pub(crate) fn path_arg(path: &Path) -> OsString {
    if path.as_os_str().as_encoded_bytes().starts_with(b"-") {
        Path::new(".").join(path).into_os_string()
    } else {
        path.as_os_str().to_owned()
    }
}

/// Returns `true` if `path` is a file the current user may execute.
fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}
