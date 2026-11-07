//! End-to-end runs of the `vcb` binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use vcb_core::preset::{Mode, Resolution};
use vcb_core::{Camera, Frame, Preset, Timing};
use vcb_io::video::probe_video;

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn command(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vcb"));
    command
        .args(args)
        .current_dir(root())
        .env_remove("VCB_CAMERAS");
    command
}

fn vcb(args: &[&str]) -> Output {
    command(args).output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vcb_cli");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn path_arg(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn render_writes_the_upright_input_size() {
    let output = scratch("sample-clean.png");
    let _ = std::fs::remove_file(&output);
    let run = vcb(&[
        "render",
        "samples/sample-01.jpg",
        path_arg(&output),
        "--preset",
        "presets/a520-clean.toml",
        "--resolution",
        "match-input",
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let input = vcb_io::probe_image(root().join("samples/sample-01.jpg")).unwrap();
    let written = vcb_io::probe_image(&output).unwrap();
    assert_eq!((written.width, written.height), (input.width, input.height));
    let stdout = text(&run.stdout);
    assert!(
        stdout.contains(&format!("{}x{}", input.width, input.height)) && stdout.contains("still"),
        "{stdout}"
    );
}

#[test]
fn render_honours_the_camera_directory_and_mode_override() {
    let output = scratch("sample-movie.jpg");
    let run = vcb(&[
        "render",
        "samples/sample-01.jpg",
        path_arg(&output),
        "--preset",
        "presets/a520-hpf.toml",
        "--camera-dir",
        path_arg(&root().join("cameras")),
        "--mode",
        "movie",
        "--seed",
        "9",
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    assert!(text(&run.stdout).contains("movie"));
    let written = vcb_io::probe_image(&output).unwrap();
    assert!(written.width < written.height);
}

#[test]
fn render_explains_a_missing_camera() {
    let run = vcb(&[
        "render",
        "samples/sample-01.jpg",
        path_arg(&scratch("never.png")),
        "--preset",
        "presets/a520-clean.toml",
        "--camera-dir",
        "no-such-dir",
    ]);
    assert!(!run.status.success());
    assert!(text(&run.stderr).contains("a520"), "{}", text(&run.stderr));
}

#[test]
fn inspect_prints_the_derived_timing() {
    let run = vcb(&[
        "inspect",
        "--preset",
        "presets/a520-clean.toml",
        "--width",
        "640",
        "--height",
        "480",
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let stdout = text(&run.stdout);
    for needle in ["pixel clock", "line period", "frame period", "640 x 480"] {
        assert!(stdout.contains(needle), "missing {needle:?} in\n{stdout}");
    }
}

#[test]
fn presets_lists_every_preset_file() {
    let run = vcb(&["presets", "--dir", "presets"]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let stdout = text(&run.stdout);
    let shipped = [
        ("a520-clean", "none"),
        ("a520-hpf", "bus"),
        ("a520-div", "bus"),
        ("a520-inv", "bus"),
        ("a520-full-spectrum", "optics"),
        ("a520-chlorophyll", "optics"),
        ("a520-full-spectrum-hpf", "bus,optics"),
        ("a520-brownout", "rail"),
        ("a520-sieve", "rail"),
        ("a520-dying-battery", "rail,memory"),
        ("a520-loose-sync", "clock"),
        ("a520-reclock-roll", "clock"),
        ("a520-s9000-tear", "memory"),
        ("a520-mirror-bank", "memory"),
        ("a520-heat-gun", "sensor"),
        ("a520-substrate", "sensor"),
        ("a520-reset-drag", "reset"),
        ("a520-hot-reclock", "sensor,clock"),
    ];
    for (name, bends) in shipped {
        let line = stdout
            .lines()
            .find(|line| line.split_whitespace().next() == Some(name))
            .unwrap_or_else(|| panic!("missing {name} in\n{stdout}"));
        assert!(
            line.ends_with(&format!("bends={bends}")),
            "{name} should list bends={bends}: {line}"
        );
    }
    assert_eq!(stdout.lines().count(), shipped.len(), "{stdout}");
}

#[test]
fn presets_names_the_milestone_3_and_4_bends() {
    let dir = scratch("presets-milestone3");
    std::fs::create_dir_all(&dir).unwrap();
    let preset = "camera = \"a520\"\n\
                  [bend.optics]\n\
                  [bend.sensor]\n\
                  [bend.rail]\n\
                  [bend.clock]\nosc_mhz = 27.0\n\
                  [bend.memory]\n\
                  [bend.reset]\n\
                  [bend.serial]\n\
                  [bend.leak]\n";
    std::fs::write(dir.join("a520-every-new-bend.toml"), preset).unwrap();
    let run = vcb(&["presets", "--dir", path_arg(&dir)]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let stdout = text(&run.stdout);
    assert!(
        stdout.contains("bends=optics,sensor,rail,clock,memory,reset,serial,leak"),
        "{stdout}"
    );
}

/// The ffmpeg the binary would use, if it runs.
fn ffmpeg() -> Option<PathBuf> {
    let program = std::env::var_os("VCB_FFMPEG")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from);
    let runs = Command::new(&program)
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success());
    runs.then_some(program)
}

/// A 2-second, 320 x 240, 10 fps test pattern; `None`, with a message, without ffmpeg.
fn clip() -> Option<&'static Path> {
    static CLIP: OnceLock<Option<PathBuf>> = OnceLock::new();
    let clip = CLIP.get_or_init(|| {
        let Some(ffmpeg) = ffmpeg() else {
            eprintln!("skipping video test: ffmpeg not found (install it or set VCB_FFMPEG)");
            return None;
        };
        let path = scratch("testsrc2.mp4");
        let status = Command::new(ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("testsrc2=size=320x240:rate=10")
            .args(["-t", "2", "-pix_fmt", "yuv420p"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success(), "ffmpeg could not generate the test clip");
        Some(path)
    });
    clip.as_deref()
}

fn read_text(relative: &str) -> String {
    std::fs::read_to_string(root().join(relative)).unwrap()
}

#[test]
fn render_video_keeps_the_input_frame_count_and_size() {
    let Some(clip) = clip() else { return };
    let output = scratch("clip-clean.mp4");
    let _ = std::fs::remove_file(&output);
    let run = vcb(&[
        "render-video",
        path_arg(clip),
        path_arg(&output),
        "--preset",
        "presets/a520-clean.toml",
        "--resolution",
        "match-input",
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let input = probe_video(clip).unwrap();
    let written = probe_video(&output).unwrap();
    assert_eq!(
        (written.width, written.height, written.frames),
        (input.width, input.height, input.frames)
    );
    assert!((written.fps - input.fps).abs() < 1e-9, "{written}");
    let stdout = text(&run.stdout);
    assert!(
        stdout.contains("20 frames") && stdout.contains("320x240") && stdout.contains("still"),
        "{stdout}"
    );
}

#[test]
fn render_video_in_movie_mode_writes_the_movie_size() {
    let Some(clip) = clip() else { return };
    let output = scratch("clip-movie.mp4");
    let run = vcb(&[
        "render-video",
        path_arg(clip),
        path_arg(&output),
        "--preset",
        "presets/a520-clean.toml",
        "--resolution",
        "match-input",
        "--mode",
        "movie",
        "--duration",
        "1",
        "--fps",
        "15",
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let camera: Camera = toml::from_str(&read_text("cameras/a520.toml")).unwrap();
    let mut preset: Preset = toml::from_str(&read_text("presets/a520-clean.toml")).unwrap();
    preset.readout.mode = Mode::Movie;
    preset.readout.resolution = Resolution::MatchInput;
    let input = probe_video(clip).unwrap();
    let timing = Timing::derive(&camera, &preset, input.width, input.height).unwrap();
    let written = probe_video(&output).unwrap();
    assert_eq!(
        (written.width, written.height),
        (timing.output_width & !1, timing.output_height & !1)
    );
    assert!(written.width < input.width, "movie readout bins and skips");
    assert_eq!(written.frames, Some(10), "one second of a 10 fps clip");
    assert!((written.fps - 15.0).abs() < 1e-9, "{written}");
}

#[test]
fn render_video_explains_a_missing_ffprobe() {
    let run = command(&[
        "render-video",
        "samples/sample-01.jpg",
        path_arg(&scratch("never.mp4")),
        "--preset",
        "presets/a520-clean.toml",
    ])
    .env("VCB_FFPROBE", root().join("no-such-ffprobe"))
    .output()
    .unwrap();
    assert!(!run.status.success());
    let stderr = text(&run.stderr);
    assert!(stderr.contains("VCB_FFPROBE"), "{stderr}");
}

#[test]
fn inspect_sizes_the_sensor_from_a_video() {
    let Some(clip) = clip() else { return };
    let run = vcb(&[
        "inspect",
        "--preset",
        "presets/a520-clean.toml",
        "--video",
        path_arg(clip),
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let stdout = text(&run.stdout);
    assert!(stdout.contains("input         320 x 240 px"), "{stdout}");
    assert!(stdout.contains("20 frames"), "{stdout}");
}

/// Writes a `width` by `height` colour gradient as PNG and returns its path.
fn gradient(name: &str, width: u32, height: u32) -> PathBuf {
    let rgb = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| [(x * 255 / width) as u8, (y * 255 / height) as u8, 128])
        })
        .collect();
    let path = scratch(name);
    let frame = Frame::from_rgb(width, height, rgb);
    vcb_io::write_frame(&path, &frame, vcb_io::Format::Png).unwrap();
    path
}

#[test]
fn sheet_tiles_renders_then_images_row_major() {
    let input = gradient("sheet-input.png", 96, 64);
    let extra = gradient("sheet-extra.png", 30, 60);
    let output = scratch("sheet.png");
    let run = vcb(&[
        "sheet",
        path_arg(&output),
        "--input",
        path_arg(&input),
        "--preset",
        "presets/a520-clean.toml",
        "presets/a520-hpf.toml",
        "--resolution",
        "match-input",
        "--image",
        path_arg(&extra),
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    // Three tiles: two columns and two rows of cells the size of the first render,
    // separated and framed by 4-pixel gaps.
    let sheet = vcb_io::read_image(&output).unwrap();
    assert_eq!(
        (sheet.width(), sheet.height()),
        (2 * 96 + 3 * 4, 2 * 64 + 3 * 4)
    );
    let stdout = text(&run.stdout);
    for needle in [
        "tile 0",
        "a520-clean",
        "tile 1",
        "a520-hpf",
        "tile 2",
        "sheet-extra",
    ] {
        assert!(stdout.contains(needle), "missing {needle:?} in\n{stdout}");
    }
    let pixels = Frame::from_rgb(sheet.width(), sheet.height(), sheet.to_srgb8());
    let background = pixels.pixel(0, 0);
    let (right, bottom) = (sheet.width() - 1, sheet.height() - 1);
    assert_eq!(
        pixels.pixel(right, bottom),
        background,
        "the fourth cell is empty"
    );
    assert_ne!(pixels.pixel(4 + 48, 4 + 32), background, "tile 0 is drawn");
}

#[test]
fn sheet_honours_columns_and_cell_size() {
    let input = gradient("sheet-input-small.png", 96, 64);
    let output = scratch("sheet-cells.png");
    let run = vcb(&[
        "sheet",
        path_arg(&output),
        "--input",
        path_arg(&input),
        "--preset",
        "presets/a520-clean.toml",
        "--preset",
        "presets/a520-inv.toml",
        "--cols",
        "1",
        "--cell",
        "50x40",
    ]);
    assert!(run.status.success(), "stderr: {}", text(&run.stderr));
    let sheet = vcb_io::probe_image(&output).unwrap();
    assert_eq!((sheet.width, sheet.height), (50 + 2 * 4, 2 * 40 + 3 * 4));
}
