//! Job coalescing on the worker, stale-reply rejection, the preview reduction, and
//! the real renderer's answers.

use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use vcb_app::presets;
use vcb_app::render::{self, Generations, Job, PREVIEW_LONG_EDGE, Renderer, Worker};
use vcb_core::Preset;
use vcb_core::Scene;
use vcb_core::bus::Target;
use vcb_core::preset::Resolution;
use vcb_core::scene::Channel;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// A job for the shipped high-pass bend at match-input resolution on a gradient.
fn hpf_job(preview: bool) -> Job {
    let camera = presets::load_camera(&root().join("cameras"), "a520").expect("camera");
    let mut preset = presets::load_preset(&root().join("presets/a520-hpf.toml")).expect("preset");
    preset.readout.resolution = Resolution::MatchInput;
    let (width, height) = (800, 600);
    let rgb: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            let x = (i % width * 255 / width) as u8;
            [x, 128, 255 - x]
        })
        .collect();
    Job {
        generation: 3,
        scene: Arc::new(Scene::from_srgb8(width, height, &rgb)),
        camera: Arc::new(camera),
        preset,
        preview,
    }
}

#[test]
fn the_renderer_answers_previews_reduced_and_full_renders_at_size() {
    let mut renderer = Renderer::default();
    let preview = renderer.render(&hpf_job(true));
    assert_eq!((preview.generation, preview.preview), (3, true));
    let output = preview.outcome.expect("the preview renders");
    assert_eq!(
        (output.frame.width(), output.frame.height()),
        (PREVIEW_LONG_EDGE, 540)
    );
    assert_eq!(
        output.image.size,
        [720, 540],
        "the texture matches the frame"
    );

    let full = renderer.render(&hpf_job(false));
    let output = full.outcome.expect("the full render renders");
    assert_eq!((output.frame.width(), output.frame.height()), (800, 600));
    assert_eq!(output.sensor, [800, 600]);
}

#[test]
fn an_invalid_preset_is_an_error_reply() {
    let mut job = hpf_job(false);
    if let Some(bus) = job.preset.bend.bus.as_mut() {
        bus.target = Target::Pin(2);
    }
    let reply = Renderer::default().render(&job);
    let message = reply.outcome.expect_err("D2 is not on the target selector");
    assert!(message.contains("target"), "{message}");
}

/// Waits for `count` replies from `worker`, failing after a few seconds.
fn replies<J: Send + 'static>(worker: &Worker<J, u64>, count: usize) -> Vec<u64> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut out = Vec::new();
    while out.len() < count {
        assert!(Instant::now() < deadline, "worker answered only {out:?}");
        match worker.poll() {
            Some(reply) => out.push(reply),
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    out
}

#[test]
fn the_worker_skips_to_the_newest_job_after_a_burst() {
    let (started, started_rx) = mpsc::channel::<u64>();
    let (release, gate) = mpsc::channel::<()>();
    let worker = Worker::spawn(
        "fake-render",
        move |generation: u64| {
            started.send(generation).expect("the test listens");
            if generation == 1 {
                gate.recv().expect("the test releases the first job");
            }
            generation
        },
        || {},
    )
    .expect("thread starts");

    assert!(worker.submit(1));
    assert_eq!(
        started_rx.recv_timeout(Duration::from_secs(5)),
        Ok(1),
        "the first job starts at once"
    );
    for generation in 2..=5 {
        assert!(worker.submit(generation));
    }
    release.send(()).expect("worker waits");

    assert_eq!(replies(&worker, 2), vec![1, 5], "jobs 2 to 4 are skipped");
    let rendered: Vec<u64> = started_rx.try_iter().collect();
    assert_eq!(rendered, vec![5], "only the newest queued job is rendered");
}

#[test]
fn replies_older_than_the_frame_on_screen_are_dropped() {
    let mut generations = Generations::default();
    let ids: Vec<u64> = (0..5).map(|_| generations.issue()).collect();
    assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    assert!(generations.is_pending());

    assert!(
        generations.accept(2),
        "an older reply is still the best view during a drag"
    );
    assert!(generations.accept(5));
    assert!(
        !generations.accept(3),
        "a reply older than the frame on screen is stale"
    );
    assert!(!generations.accept(5), "the same reply twice is stale");
    assert!(generations.is_latest(5));
    assert!(!generations.is_pending());
}

#[test]
fn a_new_scene_makes_every_earlier_job_stale() {
    let mut generations = Generations::default();
    let before = generations.issue();
    generations.invalidate();
    let after = generations.issue();
    assert!(
        !generations.accept(before),
        "a render of the previous scene is never shown"
    );
    assert!(generations.accept(after));
}

#[test]
fn previews_are_needed_unless_they_would_equal_the_full_render() {
    let large = Scene::new(1600, 1200);
    let small = Scene::new(640, 480);
    let native = Preset::new("a520");
    let mut matched = Preset::new("a520");
    matched.readout.resolution = Resolution::MatchInput;
    assert!(render::needs_preview(&native, &small));
    assert!(render::needs_preview(&matched, &large));
    assert!(!render::needs_preview(&matched, &small));
    assert_eq!(
        render::preview_preset(&native).readout.resolution,
        Resolution::MatchInput
    );
}

#[test]
fn reduction_crops_to_the_aspect_and_limits_the_long_edge() {
    let scene = Scene::new(3000, 1500);
    let reduced = render::reduce(&scene, 4.0 / 3.0, PREVIEW_LONG_EDGE);
    assert_eq!(
        (reduced.width(), reduced.height()),
        (PREVIEW_LONG_EDGE, 540),
        "a 2:1 scene is cropped to 4:3, then shrunk"
    );
    let small = render::reduce(&Scene::new(400, 300), 4.0 / 3.0, PREVIEW_LONG_EDGE);
    assert_eq!(
        (small.width(), small.height()),
        (400, 300),
        "a small scene is not enlarged"
    );
}

#[test]
fn reduction_averages_light_over_each_footprint() {
    // Columns alternate 0 and 1: halving the width averages each pair to 0.5,
    // and a non-integer step keeps the mean.
    let (width, height) = (1440, 1080);
    let mut scene = Scene::new(width, height);
    for channel in Channel::ALL {
        for row in scene.plane_mut(channel).rows_mut() {
            for (x, value) in row.iter_mut().enumerate() {
                *value = (x % 2) as f32;
            }
        }
    }
    let halved = render::reduce(&scene, 4.0 / 3.0, PREVIEW_LONG_EDGE);
    assert_eq!((halved.width(), halved.height()), (720, 540));
    for value in halved.plane(Channel::Green).data() {
        assert!(
            (value - 0.5).abs() < 1e-5,
            "pairs average to 0.5, got {value}"
        );
    }
    let odd = render::reduce(&scene, 4.0 / 3.0, 500);
    let data = odd.plane(Channel::Red).data();
    let mean = data.iter().map(|value| f64::from(*value)).sum::<f64>() / data.len() as f64;
    assert!(
        (mean - 0.5).abs() < 1e-3,
        "area averaging keeps the mean, got {mean}"
    );
}
