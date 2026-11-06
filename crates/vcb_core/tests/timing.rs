//! Properties of the derived readout timing and stream layout.

#![cfg(feature = "serde")]

use vcb_core::preset::{Mode, Orientation, Resolution};
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

fn derive(mode: Mode, resolution: Resolution, width: u32, height: u32) -> Timing {
    Timing::derive(&camera(), &preset(mode, resolution), width, height).unwrap()
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
    for (mode, resolution, width, height) in CASES {
        let t = derive(mode, resolution, width, height);
        let line_len = f64::from(t.cols + t.ob_cols + t.hblank_px);
        let lines = f64::from(t.rows_read + t.vblank_lines);
        assert!(close(t.pixel_period_s * line_len, t.line_period_s, 1e-12));
        assert!(close(t.line_period_s * lines, t.frame_period_s, 1e-12));
        assert!(close(t.pixel_clock_hz * t.pixel_period_s, 1.0, 1e-12));
    }
}

#[test]
fn match_input_keeps_the_native_frame_period() {
    for mode in [Mode::Still, Mode::Movie] {
        let native = derive(mode, Resolution::Native, 1, 1);
        for (width, height) in [(640, 480), (1920, 1440), (301, 977)] {
            let matched = derive(mode, Resolution::MatchInput, width, height);
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
fn match_input_keeps_the_native_active_line_time() {
    for mode in [Mode::Still, Mode::Movie] {
        let native = derive(mode, Resolution::Native, 1, 1);
        let native_active = f64::from(native.cols) * native.pixel_period_s;
        let small = derive(mode, Resolution::MatchInput, 640, 480);
        let large = derive(mode, Resolution::MatchInput, 1920, 1440);
        for t in [&small, &large] {
            let active = f64::from(t.cols) * t.pixel_period_s;
            assert!(
                close(active, native_active, 1e-3),
                "{mode} {}x{}: active line {active} s vs native {native_active} s",
                t.cols,
                t.rows_read
            );
            assert!(
                t.hblank_px > native.hblank_px,
                "blanking absorbs the longer line"
            );
        }
        let lines = large.lines_per_cycle(1000.0) / small.lines_per_cycle(1000.0);
        let rows = f64::from(large.rows_read) / f64::from(small.rows_read);
        assert!(
            (lines / rows - 1.0).abs() < 0.01,
            "{mode}: a 1 kHz cycle spans {lines} times the lines at {rows} times the rows"
        );
    }
}

#[test]
fn inputs_with_more_rows_than_the_sensor_keep_minimum_blanking() {
    let native = derive(Mode::Still, Resolution::Native, 1, 1);
    let tall = derive(Mode::Still, Resolution::MatchInput, 4000, 3000);
    assert_eq!(tall.hblank_px, 8);
    assert!(tall.frame_period_s > native.frame_period_s);
    let native_active = f64::from(native.cols) * native.pixel_period_s;
    let active = f64::from(tall.cols) * tall.pixel_period_s;
    assert!(close(active, native_active, 1e-12));
}

#[test]
fn lines_per_cycle_counts_line_periods() {
    let t = derive(Mode::Movie, Resolution::Native, 1, 1);
    let freq_hz = 1000.0;
    let lines = t.lines_per_cycle(freq_hz);
    assert!(close(lines * t.line_period_s * freq_hz, 1.0, 1e-12));
    let samples = t.samples_per_cycle(freq_hz);
    assert!(close(samples / lines, f64::from(t.line_len()), 1e-12));
}

#[test]
fn native_pixel_period_is_the_camera_clock_at_every_resolution() {
    let camera = camera();
    for (mode, resolution, width, height) in CASES {
        let t = derive(mode, resolution, width, height);
        let clock_hz = camera.clocking(mode).pixel_clock_hz;
        assert!(close(t.native_pixel_period_s * clock_hz, 1.0, 1e-12));
    }
}

#[test]
fn match_input_at_the_native_size_reproduces_the_native_clocks() {
    let camera = camera();
    let (width, height) = (camera.sensor.native_width, camera.sensor.native_height);
    for mode in [Mode::Still, Mode::Movie] {
        let native = derive(mode, Resolution::Native, 1, 1);
        let matched = derive(mode, Resolution::MatchInput, width, height);
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
    let t = derive(Mode::Still, Resolution::MatchInput, 1601, 1067);
    assert_eq!((t.sensor_width, t.sensor_height), (1600, 1066));
    assert_eq!((t.output_width, t.output_height), (1600, 1066));
}

#[test]
fn portrait_inputs_turn_onto_the_landscape_sensor() {
    let camera = camera();
    let auto = derive(Mode::Still, Resolution::MatchInput, 480, 640);
    assert!(auto.rotated);
    assert_eq!((auto.sensor_width, auto.sensor_height), (640, 480));
    assert_eq!((auto.output_width, auto.output_height), (480, 640));
    assert_eq!(auto.developed_size(), (640, 480));

    let mut level = preset(Mode::Still, Resolution::MatchInput);
    level.readout.orientation = Orientation::Landscape;
    let kept = Timing::derive(&camera, &level, 480, 640).unwrap();
    assert!(!kept.rotated);
    assert_eq!((kept.sensor_width, kept.sensor_height), (480, 640));

    let mut held = preset(Mode::Still, Resolution::Native);
    held.readout.orientation = Orientation::Portrait;
    let native = Timing::derive(&camera, &held, 800, 600).unwrap();
    assert_eq!(
        (native.sensor_width, native.sensor_height),
        (camera.sensor.native_width, camera.sensor.native_height)
    );
    assert_eq!(
        (native.output_width, native.output_height),
        (camera.sensor.native_height, camera.sensor.native_width)
    );
}

#[test]
fn movie_mode_reads_one_row_pair_in_every_line_skip_pairs() {
    let camera = camera();
    let skip = camera.movie.line_skip;
    let t = derive(Mode::Movie, Resolution::MatchInput, 256, 192);
    assert_eq!(t.rows_read, t.sensor_height / skip);
    assert_eq!(
        (t.output_width, t.output_height),
        (256 / camera.movie.h_bin, 192 / skip)
    );

    let layout = Layout::from_timing(&t, Direction::TopDown);
    let map = layout.row_map();
    assert_eq!(map.len(), t.rows_read as usize);
    for pair in map.as_chunks::<2>().0 {
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
    let still = derive(Mode::Still, Resolution::Native, 800, 600);
    assert_eq!(
        (still.output_width, still.output_height),
        (camera.sensor.native_width, camera.sensor.native_height)
    );
    let movie = derive(Mode::Movie, Resolution::Native, 800, 600);
    assert_eq!(
        (movie.output_width, movie.output_height),
        (camera.movie.output_width, camera.movie.output_height)
    );
}

#[test]
fn layout_periods_agree_with_the_timing() {
    for (mode, resolution, width, height) in CASES {
        let t = derive(mode, resolution, width, height);
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
    let text = derive(Mode::Movie, Resolution::Native, 1, 1).to_string();
    let needles = [
        "movie",
        "native",
        "landscape",
        "MHz",
        "µs",
        "ms",
        "frames per second",
        "px",
    ];
    for needle in needles {
        assert!(text.contains(needle), "missing {needle:?} in\n{text}");
    }
}
