//! `vcb-app`, the desktop app of the virtual camera bender.
//!
//! `vcb-app [FILE]` opens the window, with `FILE` (an image or a video) in front of
//! the camera if given. Camera files are read from `$VCB_CAMERAS`, else `cameras/` in
//! the working directory; the preset list from `presets/`. Logs go to stderr, filtered
//! by `RUST_LOG` (default `warn`); `RUST_LOG=info` logs every render.

#![expect(
    clippy::multiple_crate_versions,
    reason = "eframe, wgpu and winit pull several versions of platform crates that this crate cannot unify"
)]

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::WARN.into())
                .from_env_lossy(),
        )
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    match vcb_app::run(path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
