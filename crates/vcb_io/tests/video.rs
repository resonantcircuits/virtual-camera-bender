//! Probing, decoding and encoding video through ffmpeg.
//!
//! The tests generate their clip with ffmpeg's `testsrc2` source and are skipped, with
//! a message on stderr, when ffmpeg is not installed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use vcb_core::{Frame, Scene};
use vcb_io::video::{
    Codec, ReadOptions, VideoReader, VideoWriter, WriteOptions, frame_to_even, probe_video,
};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vcb_io_video");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// The ffmpeg the library would use, if it runs.
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

/// A 2-second, 320 x 240, 10 fps test pattern, generated once per test run.
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

fn read_all(path: &Path, options: ReadOptions) -> (VideoReader, Vec<Scene>) {
    let mut reader = VideoReader::open(path, options).unwrap();
    let mut scenes = Vec::new();
    while let Some(scene) = reader.next_frame().unwrap() {
        scenes.push(scene);
    }
    (reader, scenes)
}

fn to_frame(scene: &Scene) -> Frame {
    Frame::from_rgb(scene.width(), scene.height(), scene.to_srgb8())
}

#[test]
fn probe_reports_size_rate_and_count() {
    let Some(clip) = clip() else { return };
    let info = probe_video(clip).unwrap();
    assert_eq!((info.width, info.height), (320, 240), "{info}");
    assert!((info.fps - 10.0).abs() < 1e-9, "{info}");
    assert_eq!(info.frames, Some(20), "{info}");
    let duration_s = info.duration_s.unwrap();
    assert!((duration_s - 2.0).abs() < 0.05, "{info}");
}

#[test]
fn reader_yields_every_frame_in_order() {
    let Some(clip) = clip() else { return };
    let (mut reader, scenes) = read_all(clip, ReadOptions::default());
    assert_eq!(scenes.len(), 20);
    assert_eq!(reader.estimated_frames(), Some(20));
    for scene in &scenes {
        assert_eq!((scene.width(), scene.height()), (320, 240));
    }
    assert_ne!(scenes[0], scenes[19], "the test pattern moves");
    assert!(reader.next_frame().unwrap().is_none(), "the end is sticky");
}

#[test]
fn max_dimension_scales_down_keeping_the_aspect_ratio() {
    let Some(clip) = clip() else { return };
    let options = ReadOptions {
        max_dimension: Some(160),
        ..ReadOptions::default()
    };
    let (reader, scenes) = read_all(clip, options);
    assert_eq!((reader.width(), reader.height()), (160, 120));
    assert_eq!(scenes.len(), 20);
    assert!(
        scenes
            .iter()
            .all(|scene| (scene.width(), scene.height()) == (160, 120))
    );
}

#[test]
fn start_and_duration_select_a_window() {
    let Some(clip) = clip() else { return };
    let options = ReadOptions {
        start_s: Some(0.5),
        duration_s: Some(1.0),
        ..ReadOptions::default()
    };
    let (reader, scenes) = read_all(clip, options);
    assert_eq!(scenes.len(), 10);
    assert_eq!(reader.estimated_frames(), Some(10));
    let (_, full) = read_all(clip, ReadOptions::default());
    assert_eq!(scenes[0], full[5], "the window starts at frame 5");
}

#[test]
fn writer_round_trip_keeps_count_and_size() {
    let Some(clip) = clip() else { return };
    let (reader, scenes) = read_all(clip, ReadOptions::default());
    let output = scratch("round-trip.mp4");
    let mut writer =
        VideoWriter::create(&output, 320, 240, WriteOptions::new(reader.info().fps)).unwrap();
    for scene in &scenes {
        writer.write_frame(&to_frame(scene)).unwrap();
    }
    assert_eq!(writer.frames_written(), 20);
    writer.finish().unwrap();
    let info = probe_video(&output).unwrap();
    assert_eq!((info.width, info.height, info.frames), (320, 240, Some(20)));
    assert!((info.fps - 10.0).abs() < 1e-9, "{info}");
}

#[test]
fn prores_archives_go_into_quicktime_files() {
    let Some(clip) = clip() else { return };
    let options = ReadOptions {
        duration_s: Some(0.5),
        max_dimension: Some(64),
        ..ReadOptions::default()
    };
    let (reader, scenes) = read_all(clip, options);
    let prores = WriteOptions {
        codec: Codec::ProRes,
        ..WriteOptions::new(10.0)
    };
    let (width, height) = (reader.width(), reader.height());
    let wrong = VideoWriter::create(scratch("archive.mp4"), width, height, prores).unwrap_err();
    assert!(wrong.is_invalid_argument(), "{wrong}");

    let output = scratch("archive.mov");
    let mut writer = VideoWriter::create(&output, width, height, prores).unwrap();
    for scene in &scenes {
        writer.write_frame(&to_frame(scene)).unwrap();
    }
    writer.finish().unwrap();
    let info = probe_video(&output).unwrap();
    assert_eq!((info.width, info.height), (64, 48));
    assert_eq!(info.frames, Some(scenes.len() as u64));
}

#[test]
fn odd_dimensions_are_rejected_at_create() {
    let output = scratch("odd.mp4");
    let _ = std::fs::remove_file(&output);
    let error = VideoWriter::create(&output, 321, 240, WriteOptions::new(10.0)).unwrap_err();
    assert!(error.is_invalid_argument(), "{error}");
    assert!(error.to_string().contains("crop or pad"), "{error}");
    assert!(!output.exists(), "no ffmpeg was started");
    for (width, height, fps) in [(320, 241, 10.0), (0, 240, 10.0), (320, 240, 0.0)] {
        let error =
            VideoWriter::create(&output, width, height, WriteOptions::new(fps)).unwrap_err();
        assert!(error.is_invalid_argument(), "{error}");
    }
}

#[test]
fn a_frame_of_another_size_is_rejected() {
    if ffmpeg().is_none() {
        return;
    }
    let mut writer =
        VideoWriter::create(scratch("mismatch.mp4"), 32, 24, WriteOptions::new(10.0)).unwrap();
    let error = writer.write_frame(&Frame::new(24, 32)).unwrap_err();
    assert!(error.is_invalid_argument(), "{error}");
    writer.write_frame(&Frame::new(32, 24)).unwrap();
    writer.finish().unwrap();
}

#[test]
fn missing_and_corrupt_videos_report_their_cause() {
    let missing = probe_video(scratch("no-such-clip.mp4")).unwrap_err();
    assert!(missing.is_io(), "{missing}");
    assert!(missing.path().ends_with("no-such-clip.mp4"));

    if ffmpeg().is_none() {
        return;
    }
    let corrupt = scratch("corrupt.mp4");
    std::fs::write(&corrupt, b"not a video").unwrap();
    let error = probe_video(&corrupt).unwrap_err();
    assert!(error.is_tool_failed(), "{error}");
    assert!(error.to_string().contains("ffprobe exited"), "{error}");
    let error = VideoReader::open(&corrupt, ReadOptions::default()).unwrap_err();
    assert!(error.is_tool_failed(), "{error}");
}

#[test]
fn bad_read_options_are_rejected_before_probing() {
    let path = scratch("never-opened.mp4");
    for options in [
        ReadOptions {
            start_s: Some(-1.0),
            ..ReadOptions::default()
        },
        ReadOptions {
            duration_s: Some(0.0),
            ..ReadOptions::default()
        },
        ReadOptions {
            max_dimension: Some(1),
            ..ReadOptions::default()
        },
    ] {
        let error = VideoReader::open(&path, options).unwrap_err();
        assert!(error.is_invalid_argument(), "{options:?}: {error}");
    }
}

#[test]
fn frame_to_even_drops_the_last_odd_column_and_row() {
    let (width, height) = (5, 3);
    let rgb: Vec<u8> = (0..width * height * 3).map(|i| i as u8).collect();
    let frame = Frame::from_rgb(width, height, rgb);
    let even = frame_to_even(&frame);
    assert_eq!((even.width(), even.height()), (4, 2));
    for y in 0..2 {
        for x in 0..4 {
            assert_eq!(even.pixel(x, y), frame.pixel(x, y), "pixel ({x}, {y})");
        }
    }
    assert_eq!(frame_to_even(&even), even, "even frames are unchanged");
}

#[test]
fn readers_and_writers_can_move_between_threads() {
    fn send<T: Send>() {}
    send::<VideoReader>();
    send::<VideoWriter>();
    send::<vcb_io::Error>();
}
