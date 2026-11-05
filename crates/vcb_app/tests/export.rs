//! The video export job: progress, completion, cancellation, and the form's settings.
//!
//! The export tests generate their clip with ffmpeg and are skipped, with a note on
//! stderr, when ffmpeg is not installed.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use vcb_app::export::{self, Dialog, Export, Outcome, Progress, Quality, Settings};
use vcb_app::presets;
use vcb_app::video::Clip;
use vcb_core::Preset;
use vcb_core::preset::{Mode, Resolution};
use vcb_io::video::{Codec, probe_video};

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// A fresh output path for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vcb_app_export");
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let path = dir.join(name);
    if path.exists() {
        std::fs::remove_file(&path).expect("old output removed");
    }
    path
}

/// Export settings for the clip through the HPF preset at match-input resolution.
fn settings(input: &Path, output: PathBuf, mode: Mode) -> Settings {
    let camera = presets::load_camera(&root().join("cameras"), "a520").expect("camera");
    let mut preset = presets::load_preset(&root().join("presets/a520-hpf.toml")).expect("preset");
    preset.readout.mode = mode;
    preset.readout.resolution = Resolution::MatchInput;
    Settings {
        input: input.to_path_buf(),
        output,
        camera,
        preset,
        codec: Codec::H264,
        quality: Quality::Small,
        start_s: 0.0,
        duration_s: None,
    }
}

/// Waits for the export's result, failing after `limit`.
fn finish(job: &Export, limit: Duration) -> Outcome {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(result) = job.poll() {
            return result.expect("the export succeeds");
        }
        assert!(
            Instant::now() < deadline,
            "the export ends: {:?}",
            job.progress()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn an_export_reports_progress_and_writes_every_frame() {
    let Some(clip) = common::clip() else { return };
    let output = scratch("movie.mp4");
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let job = Export::start(settings(clip, output.clone(), Mode::Movie), move || {
        counter.fetch_add(1, Ordering::Relaxed);
    })
    .expect("thread starts");
    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_mins(2);
    let outcome = loop {
        let progress = job.progress();
        if seen.last() != Some(&progress.done) {
            seen.push(progress.done);
        }
        if let Some(result) = job.poll() {
            break result.expect("the export succeeds");
        }
        assert!(Instant::now() < deadline, "the export ends: {progress:?}");
        std::thread::sleep(Duration::from_millis(2));
    };
    assert!(
        seen.windows(2).all(|pair| pair[0] < pair[1]),
        "progress only grows: {seen:?}"
    );
    let progress = job.progress();
    assert_eq!((progress.done, progress.total), (20, Some(20)));
    assert_eq!(progress.fraction(), Some(1.0));
    assert!(
        wakes.load(Ordering::Relaxed) >= 1,
        "the UI is woken at the end"
    );
    let Outcome::Finished { path, frames, size } = outcome else {
        panic!("the export was not cancelled, got {outcome:?}");
    };
    assert_eq!((path.as_path(), frames), (output.as_path(), 20));
    assert_eq!(
        size,
        [80, 60],
        "the movie readout bins 4 columns and skips 3 row pairs in 4"
    );
    let info = probe_video(&output).expect("the written file probes");
    assert_eq!((info.width, info.height), (80, 60), "{info}");
    assert_eq!(info.frames, Some(20), "{info}");
}

#[test]
fn a_cancelled_export_stops_and_removes_its_file() {
    let Some(clip) = common::clip() else { return };
    let output = scratch("cancelled.mp4");
    let job =
        Export::start(settings(clip, output.clone(), Mode::Still), || {}).expect("thread starts");
    let deadline = Instant::now() + Duration::from_mins(1);
    while job.progress().done < 2 {
        assert!(Instant::now() < deadline, "the export makes progress");
        assert!(job.poll().is_none(), "the export is still running");
        std::thread::sleep(Duration::from_millis(1));
    }
    job.cancel();
    assert!(job.is_cancelling());
    let outcome = finish(&job, Duration::from_secs(30));
    assert_eq!(outcome, Outcome::Cancelled);
    let done = job.progress().done;
    assert!(
        (2..20).contains(&done),
        "the export stopped part way: {done}"
    );
    assert!(!output.exists(), "the unfinished file is removed");
}

#[test]
fn an_export_over_its_source_is_refused() {
    let Some(clip) = common::clip() else { return };
    let job = Export::start(settings(clip, clip.to_path_buf(), Mode::Movie), || {})
        .expect("thread starts");
    let deadline = Instant::now() + Duration::from_secs(30);
    let result = loop {
        if let Some(result) = job.poll() {
            break result;
        }
        assert!(Instant::now() < deadline, "the export ends");
        std::thread::sleep(Duration::from_millis(5));
    };
    let error = result.expect_err("the source would be truncated");
    assert!(format!("{error:#}").contains("source video"), "{error:#}");
    assert!(clip.exists(), "the source is left alone");
}

#[test]
fn progress_tells_the_rate_and_the_time_left() {
    let progress = Progress {
        done: 30,
        total: Some(120),
        elapsed: Duration::from_secs(10),
    };
    assert!((progress.rate() - 3.0).abs() < 1e-9);
    assert_eq!(progress.eta(), Some(Duration::from_secs(30)));
    assert_eq!(progress.fraction(), Some(0.25));
    let unknown = Progress {
        total: None,
        ..progress
    };
    assert_eq!((unknown.eta(), unknown.fraction()), (None, None));
    let starting = Progress {
        done: 0,
        ..progress
    };
    assert_eq!(starting.eta(), None, "no estimate before the first frame");
    assert_eq!(export::clock(Duration::from_secs(75)), "1:15");
    assert_eq!(export::clock(Duration::from_secs(3725)), "1:02:05");
}

#[test]
fn quality_and_codec_follow_the_command_line() {
    assert_eq!(
        [Quality::Small, Quality::Balanced, Quality::High].map(Quality::crf),
        [28, 23, 18]
    );
    let mp4 = Path::new("/tmp/out.mp4");
    assert_eq!(export::with_codec_extension(mp4, Codec::H264), mp4);
    assert_eq!(
        export::with_codec_extension(mp4, Codec::ProRes),
        Path::new("/tmp/out.mov"),
        "ProRes goes into a .mov file"
    );
    assert_eq!(
        export::with_codec_extension(Path::new("/tmp/out"), Codec::H264),
        Path::new("/tmp/out.mp4")
    );
}

#[test]
fn the_form_builds_the_settings_of_an_export() {
    let Some(path) = common::clip() else { return };
    let clip = Clip::new(path, probe_video(path).expect("probe"));
    let camera = presets::load_camera(&root().join("cameras"), "a520").expect("camera");
    let preset = Preset::new("a520");
    let mut dialog = Dialog::new(&clip, Path::new("/tmp/clip-a520.mp4"), 0.0);
    assert_eq!(dialog.frames(&clip), 20, "the whole clip by default");
    dialog.to_end = false;
    dialog.start_s = 0.5;
    dialog.duration_s = 1.0;
    dialog.codec = Codec::ProRes;
    assert_eq!(dialog.frames(&clip), 10);
    let settings = dialog.settings(&clip, &camera, &preset).expect("complete");
    assert_eq!(settings.output, Path::new("/tmp/clip-a520.mov"));
    assert_eq!(settings.duration_s, Some(1.0));
    assert!((settings.start_s - 0.5).abs() < f64::EPSILON);
    assert_eq!(settings.preset, preset);

    dialog.output = "  ".to_owned();
    dialog
        .settings(&clip, &camera, &preset)
        .expect_err("an export needs a file");
}
