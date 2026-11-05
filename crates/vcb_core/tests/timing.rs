//! Properties of the derived readout timing and stream layout.

#![cfg(feature = "serde")]

use vcb_core::preset::{Mode, Resolution};
use vcb_core::readout::{Direction, Layout};
use vcb_core::{Camera, Preset, Timing};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).expect("camera file is readable"))
        .expect("camera file parses")
}

fn preset(mode: Mode, resolution: Resolution) -> Preset {
    let mut preset = Preset::new("a520");
    preset.readout.mode = mode;
    preset.readout.resolution = resolution;
    preset
}

fn close(a: f64, b: f64, relative: f64) -> bool {
    (a - b).abs() <= relative * a.abs().max(b.abs())
}

const CASES: [(Mode, Resolution, u32, u32); 6] = [
    (Mode::Still, Resolution::MatchInput, 640, 480),
    (Mode::Still, Resolution::MatchInput, 1067, 1601),
    (Mode::Still, Resolution::Native, 640, 480),
    (Mode::Movie, Resolution::MatchInput, 640, 480),
    (Mode::Movie, Resolution::MatchInput, 1920, 1440),
    (Mode::Movie, Resolution::Native, 640, 480),
];

#[test]
fn line_and_frame_periods_follow_the_pixel_period() {
    let camera = camera();
    for (mode, resolution, width, height) in CASES {
        let t = Timing::derive(&camera, &preset(mode, resolution), width, height).unwrap();
        let line_len = f64::from(t.cols + t.ob_cols + t.hblank_px);
        let lines = f64::from(t.rows_read + t.vblank_lines);
        assert!(close(t.pixel_period_s * line_len, t.line_period_s, 1e-12));
        assert!(close(t.line_period_s * lines, t.frame_period_s, 1e-12));
        assert!(close(t.pixel_clock_hz * t.pixel_period_s, 1.0, 1e-12));
    }
}

#[test]
fn match_input_keeps_the_native_frame_period() {
    let camera = camera();
    for mode in [Mode::Still, Mode::Movie] {
        let native = Timing::derive(&camera, &preset(mode, Resolution::Native), 1, 1).unwrap();
        for (width, height) in [(640, 480), (1920, 1440), (301, 977)] {
            let matched = Timing::derive(
                &camera,
                &preset(mode, Resolution::MatchInput),
                width,
                height,
            )
            .unwrap();
            assert!(
                close(matched.frame_period_s, native.frame_period_s, 1e-9),
                "{mode} {width}x{height}: {} s vs native {} s",
                matched.frame_period_s,
                native.frame_period_s
            );
        }
    }
}

#[test]
fn match_input_keeps_oscillator_bands_at_the_same_place_in_the_frame() {
    let camera = camera();
    let mode = preset(Mode::Still, Resolution::MatchInput);
    let small = Timing::derive(&camera, &mode, 640, 480).unwrap();
    let large = Timing::derive(&camera, &mode, 1920, 1440).unwrap();
    for freq_hz in [60.0, 1000.0, 15_734.0] {
        let lines = |t: &Timing| t.lines_per_cycle(freq_hz) / f64::from(t.lines_per_frame());
        assert!(close(lines(&small), lines(&large), 1e-9), "{freq_hz} Hz");
        let samples = |t: &Timing| {
            t.samples_per_cycle(freq_hz)
                / (f64::from(t.line_len()) * f64::from(t.lines_per_frame()))
        };
        assert!(
            close(samples(&small), samples(&large), 1e-9),
            "{freq_hz} Hz"
        );
    }
    let rows = f64::from(large.rows_read) / f64::from(small.rows_read);
    let ratio = small.lines_per_cycle(1000.0) / large.lines_per_cycle(1000.0);
    assert!(
        (ratio * rows - 1.0).abs() < 0.02,
        "a 1 kHz cycle spans {ratio} as many lines at a third of the rows"
    );
}

#[test]
fn lines_per_cycle_counts_line_periods() {
    let camera = camera();
    let t = Timing::derive(&camera, &preset(Mode::Movie, Resolution::Native), 1, 1).unwrap();
    let freq_hz = 1000.0;
    let lines = t.lines_per_cycle(freq_hz);
    assert!(close(lines * t.line_period_s * freq_hz, 1.0, 1e-12));
    let samples = t.samples_per_cycle(freq_hz);
    assert!(close(samples / lines, f64::from(t.line_len()), 1e-12));
}

#[test]
fn match_input_at_the_native_size_reproduces_the_native_clocks() {
    let camera = camera();
    let (width, height) = (camera.sensor.native_width, camera.sensor.native_height);
    for mode in [Mode::Still, Mode::Movie] {
        let native = Timing::derive(&camera, &preset(mode, Resolution::Native), 1, 1).unwrap();
        let matched = Timing::derive(
            &camera,
            &preset(mode, Resolution::MatchInput),
            width,
            height,
        )
        .unwrap();
        assert_eq!(
            (matched.ob_cols, matched.hblank_px, matched.vblank_lines),
            (native.ob_cols, native.hblank_px, native.vblank_lines)
        );
        assert!(close(matched.pixel_clock_hz, native.pixel_clock_hz, 1e-12));
        assert!(close(
            native.pixel_clock_hz,
            camera.clocking(mode).pixel_clock_hz,
            1e-12
        ));
    }
}

#[test]
fn match_input_rounds_the_sensor_down_to_even() {
    let camera = camera();
    let t = Timing::derive(
        &camera,
        &preset(Mode::Still, Resolution::MatchInput),
        1067,
        1601,
    )
    .unwrap();
    assert_eq!((t.sensor_width, t.sensor_height), (1066, 1600));
    assert_eq!((t.output_width, t.output_height), (1066, 1600));
}

#[test]
fn movie_mode_reads_one_row_pair_in_every_line_skip_pairs() {
    let camera = camera();
    let skip = camera.movie.line_skip;
    let t = Timing::derive(
        &camera,
        &preset(Mode::Movie, Resolution::MatchInput),
        256,
        192,
    )
    .unwrap();
    assert_eq!(t.rows_read, t.sensor_height / skip);
    assert_eq!(
        (t.output_width, t.output_height),
        (256 / camera.movie.h_bin, 192 / skip)
    );

    let layout = Layout::from_timing(&t, Direction::TopDown);
    let map = layout.row_map();
    assert_eq!(map.len(), t.rows_read as usize);
    for pair in map.chunks_exact(2) {
        assert_eq!(pair[0] % 2, 0, "pairs start on a red/green row");
        assert_eq!(pair[1], pair[0] + 1, "pairs are adjacent rows");
    }
    for step in map.windows(3).step_by(2) {
        assert_eq!(step[2] - step[0], 2 * skip);
    }

    let reversed = Layout::from_timing(&t, Direction::BottomUp);
    let mut back: Vec<u32> = reversed.row_map().to_vec();
    back.reverse();
    assert_eq!(back, map);
    assert_eq!(reversed.image_row(0), t.rows_read - 1);
}

#[test]
fn native_output_has_the_camera_frame_size() {
    let camera = camera();
    let still =
        Timing::derive(&camera, &preset(Mode::Still, Resolution::Native), 800, 600).unwrap();
    assert_eq!(
        (still.output_width, still.output_height),
        (camera.sensor.native_width, camera.sensor.native_height)
    );
    let movie =
        Timing::derive(&camera, &preset(Mode::Movie, Resolution::Native), 800, 600).unwrap();
    assert_eq!(
        (movie.output_width, movie.output_height),
        (camera.movie.output_width, camera.movie.output_height)
    );
}

#[test]
fn layout_periods_agree_with_the_timing() {
    let camera = camera();
    for (mode, resolution, width, height) in CASES {
        let t = Timing::derive(&camera, &preset(mode, resolution), width, height).unwrap();
        let layout = Layout::from_timing(&t, Direction::TopDown);
        assert!(close(layout.line_period_s(), t.line_period_s, 1e-12));
        assert!(close(layout.frame_period_s(), t.frame_period_s, 1e-12));
        assert_eq!(
            layout.sample_count(),
            (t.rows_read * (t.ob_cols + t.cols)) as usize
        );
    }
}

#[test]
fn too_small_or_empty_inputs_are_rejected() {
    let camera = camera();
    let still = preset(Mode::Still, Resolution::MatchInput);
    assert!(
        Timing::derive(&camera, &still, 0, 10)
            .unwrap_err()
            .is_input()
    );
    assert!(
        Timing::derive(&camera, &still, 1, 1)
            .unwrap_err()
            .is_input()
    );
    let movie = preset(Mode::Movie, Resolution::MatchInput);
    assert!(
        Timing::derive(&camera, &movie, 64, 6)
            .unwrap_err()
            .is_input()
    );
}

#[test]
fn display_names_every_period_with_its_unit() {
    let camera = camera();
    let t = Timing::derive(&camera, &preset(Mode::Movie, Resolution::Native), 1, 1).unwrap();
    let text = t.to_string();
    for needle in [
        "movie",
        "native",
        "MHz",
        "µs",
        "ms",
        "frames per second",
        "px",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in\n{text}");
    }
}
