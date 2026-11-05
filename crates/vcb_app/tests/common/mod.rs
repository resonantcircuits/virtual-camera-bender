//! A synthetic clip for the video tests, generated once per test binary with ffmpeg.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// A 2-second, 320 x 240, 10 fps test pattern, or `None` (with a note) without ffmpeg.
///
/// The pattern changes every frame, so frames can be told apart by their bytes.
pub fn clip() -> Option<&'static Path> {
    static CLIP: OnceLock<Option<PathBuf>> = OnceLock::new();
    let clip = CLIP.get_or_init(|| {
        let program = std::env::var_os("VCB_FFMPEG")
            .filter(|value| !value.is_empty())
            .map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from);
        let runs = Command::new(&program)
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !runs {
            eprintln!("skipping video test: ffmpeg not found (install it or set VCB_FFMPEG)");
            return None;
        }
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vcb_app_video");
        std::fs::create_dir_all(&dir).expect("scratch directory");
        // One file per test binary, so binaries running in parallel never share a half-written clip.
        let binary = std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "tests".to_owned());
        let path = dir.join(format!("testsrc2-{binary}.mp4"));
        let status = Command::new(program)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=320x240:rate=10")
            .args(["-t", "2", "-pix_fmt", "yuv420p"])
            .arg(&path)
            .status()
            .expect("ffmpeg runs");
        assert!(status.success(), "ffmpeg could not generate the test clip");
        Some(path)
    });
    clip.as_deref()
}
