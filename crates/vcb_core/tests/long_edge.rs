//! The long-edge limit of a match-input sensor: its size, its clocks, its renders and
//! its validation.

#![cfg(feature = "serde")]

use std::path::PathBuf;

use vcb_core::preset::{Mode, Orientation, Readout, Resolution};
use vcb_core::scene::Channel;
use vcb_core::{Camera, Preset, Reconfigured, Scene, Simulator, Timing};

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn camera() -> Camera {
    let text = std::fs::read_to_string(root().join("cameras/a520.toml")).unwrap();
    toml::from_str(&text).unwrap()
}

/// A shipped preset, read only.
fn shipped(name: &str) -> Preset {
    let path = root().join(format!("presets/{name}.toml"));
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The clean camera at match-input resolution with `long_edge`.
fn limited(long_edge: Option<u32>) -> Preset {
    let mut preset = Preset::new("a520");
    preset.readout.resolution = Resolution::MatchInput;
    preset.readout.long_edge = long_edge;
    preset
}

fn close(a: f64, b: f64, relative: f64) -> bool {
    (a - b).abs() <= relative * a.abs().max(b.abs())
}

/// A colour gradient with a clipped disc and a dark block, so the bends and the
/// highlight reconstruction have edges and white to act on.
fn photo(width: u32, height: u32) -> Scene {
    let rgb: Vec<u8> = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let (dx, dy) = (
                    i64::from(x) - i64::from(width) * 2 / 3,
                    i64::from(y) - i64::from(height) / 3,
                );
                let r = i64::from(width.min(height)) / 5;
                if dx * dx + dy * dy < r * r {
                    [255, 255, 250]
                } else if x < width / 3 && y > height / 2 {
                    [12, 14, 20]
                } else {
                    [(x * 255 / width) as u8, (y * 255 / height) as u8, 140]
                }
            })
        })
        .collect();
    Scene::from_srgb8(width, height, &rgb)
}

/// Halves `scene` in both directions by area averaging, in the scene stage's own order.
///
/// Every output pixel is the mean of a 2 by 2 block: each row pair is weighed by 1/2,
/// each of its two samples by 1/2, horizontally first, as the stage's separable
/// resampler sums them.
fn halve(scene: &Scene) -> Scene {
    let (width, height) = (scene.width() / 2, scene.height() / 2);
    let mut out = Scene::new(width, height);
    for channel in Channel::ALL {
        let source = scene.plane(channel);
        let plane = out.plane_mut(channel);
        for y in 0..height {
            let (top, bottom) = (source.row(2 * y), source.row(2 * y + 1));
            for (x, value) in plane.row_mut(y).iter_mut().enumerate() {
                let across = |row: &[f32]| 0.5 * row[2 * x] + 0.5 * row[2 * x + 1];
                *value = 0.5 * across(top) + 0.5 * across(bottom);
            }
        }
    }
    out
}

/// Shrinks `scene` to `width` by `height` by area averaging in double precision.
///
/// A reduction of its own, not the scene stage's: the two agree up to rounding.
fn shrink(scene: &Scene, width: u32, height: u32) -> Scene {
    let weights = |from: u32, to: u32| -> Vec<Vec<(usize, f64)>> {
        let step = f64::from(from) / f64::from(to);
        (0..to)
            .map(|i| {
                let (a, b) = (f64::from(i) * step, f64::from(i + 1) * step);
                (a.floor() as usize..(b.ceil() as usize).min(from as usize))
                    .map(|u| (u, (b.min(u as f64 + 1.0) - a.max(u as f64)) / step))
                    .filter(|&(_, w)| w > 0.0)
                    .collect()
            })
            .collect()
    };
    let (columns, rows) = (
        weights(scene.width(), width),
        weights(scene.height(), height),
    );
    let mut out = Scene::new(width, height);
    for channel in Channel::ALL {
        let source = scene.plane(channel);
        let plane = out.plane_mut(channel);
        for (y, row_taps) in rows.iter().enumerate() {
            for (x, column_taps) in columns.iter().enumerate() {
                let mut sum = 0.0;
                for &(v, wy) in row_taps {
                    let row = source.row(v as u32);
                    for &(u, wx) in column_taps {
                        sum += wy * wx * f64::from(row[u]);
                    }
                }
                plane.set(x as u32, y as u32, sum as f32);
            }
        }
    }
    out
}

/// Frame 0 from power-on of `preset` on `scene`, as `vcb render` writes it.
fn render(preset: &Preset, scene: &Scene) -> vcb_core::Frame {
    Simulator::builder((camera(), preset.clone()))
        .input_size(scene.width(), scene.height())
        .build()
        .unwrap()
        .frame(scene, 0)
        .clone()
}

#[test]
fn a_long_edge_fits_the_input_and_keeps_the_frame_period() {
    let camera = camera();
    let native = Timing::derive(&camera, &Preset::new("a520"), 1, 1).unwrap();
    // (input, long edge, sensor): the long side takes the limit, the short side the
    // aspect to the nearest pixel, both rounded down to even.
    let cases = [
        ((4000, 3000), 400, (400, 300)),
        ((1001, 667), 400, (400, 266)),
        ((4000, 3000), 401, (400, 300)),
        ((3000, 3000), 160, (160, 160)),
        ((6000, 2000), 720, (720, 240)),
    ];
    for ((width, height), edge, (sensor_w, sensor_h)) in cases {
        let unlimited = Timing::derive(&camera, &limited(None), width, height).unwrap();
        let t = Timing::derive(&camera, &limited(Some(edge)), width, height).unwrap();
        assert_eq!(
            (t.sensor_width, t.sensor_height),
            (sensor_w, sensor_h),
            "{width}x{height} at {edge}"
        );
        assert_eq!((t.output_width, t.output_height), (sensor_w, sensor_h));
        assert_eq!(t.long_edge, Some(edge));
        assert!(
            close(t.frame_period_s, native.frame_period_s, 1e-9),
            "{width}x{height} at {edge}: the frame period stays the camera's, {} s vs {} s",
            t.frame_period_s,
            native.frame_period_s
        );
        let active = f64::from(t.cols) * t.pixel_period_s;
        let native_active = f64::from(native.cols) * native.pixel_period_s;
        assert!(
            close(active, native_active, 1e-3),
            "{width}x{height} at {edge}: active line {active} s vs native {native_active} s"
        );
        assert!(
            t.pixel_period_s > unlimited.pixel_period_s,
            "fewer pixels in the same line time are longer ones"
        );
    }
}

#[test]
fn the_limit_holds_the_long_side_in_portrait_and_in_movie_mode() {
    let camera = camera();
    let mut preset = limited(Some(400));
    let tall = Timing::derive(&camera, &preset, 3000, 4000).unwrap();
    assert!(tall.rotated, "a tall input turns the camera");
    assert_eq!((tall.sensor_width, tall.sensor_height), (400, 300));
    assert_eq!((tall.output_width, tall.output_height), (300, 400));

    preset.readout.orientation = Orientation::Landscape;
    let level = Timing::derive(&camera, &preset, 3000, 4000).unwrap();
    assert_eq!((level.sensor_width, level.sensor_height), (300, 400));

    preset.readout.orientation = Orientation::Auto;
    preset.readout.mode = Mode::Movie;
    let movie = Timing::derive(&camera, &preset, 4000, 3000).unwrap();
    assert_eq!((movie.sensor_width, movie.sensor_height), (400, 300));
    assert_eq!(
        (movie.output_width, movie.output_height),
        (400 / camera.movie.h_bin, movie.rows_read),
        "the movie readout bins and skips the limited sensor"
    );
}

#[test]
fn an_input_within_the_limit_keeps_its_size() {
    let camera = camera();
    let unlimited = Timing::derive(&camera, &limited(None), 320, 240).unwrap();
    for edge in [320, 400, Readout::MAX_LONG_EDGE] {
        let mut t = Timing::derive(&camera, &limited(Some(edge)), 320, 240).unwrap();
        assert_eq!(t.long_edge, Some(edge));
        t.long_edge = None;
        assert_eq!(
            t, unlimited,
            "a limit of {edge} px leaves a 320 px input as it is"
        );
    }
}

#[test]
fn a_render_at_a_long_edge_is_the_render_of_the_input_reduced_to_it() {
    let preset = shipped("a520-hpf");
    let mut full = preset.clone();
    full.readout.resolution = Resolution::MatchInput;
    full.readout.long_edge = Some(400);
    let mut matched = full.clone();
    matched.readout.long_edge = None;

    let scene = photo(800, 600);
    let at_edge = render(&full, &scene);
    assert_eq!((at_edge.width(), at_edge.height()), (400, 300));
    let reduced = halve(&scene);
    let at_size = render(&matched, &reduced);
    assert!(
        at_edge.rgb() == at_size.rgb(),
        "the scene stage area-averages the input onto the limited sensor, as a reduced input"
    );
}

#[test]
fn a_render_at_an_uneven_reduction_is_close_to_the_reduced_input() {
    let preset = limited(Some(400));
    let scene = photo(1000, 750);
    let at_edge = render(&preset, &scene);
    assert_eq!((at_edge.width(), at_edge.height()), (400, 300));
    let at_size = render(&limited(None), &shrink(&scene, 400, 300));
    assert_eq!((at_size.width(), at_size.height()), (400, 300));
    let differences: Vec<f64> = at_edge
        .rgb()
        .iter()
        .zip(at_size.rgb())
        .map(|(&a, &b)| (f64::from(a) - f64::from(b)).abs())
        .collect();
    let mean = differences.iter().sum::<f64>() / differences.len() as f64;
    let worst = differences.iter().copied().fold(0.0, f64::max);
    assert!(
        mean < 0.01 && worst <= 2.0,
        "a 2.5 times reduction by the stage and one in double precision differ by \
         rounding at most: mean {mean:.4}, worst {worst} codes"
    );
}

#[test]
fn a_limit_the_input_does_not_reach_renders_the_same_bytes() {
    let preset = shipped("a520-div");
    let mut matched = preset.clone();
    matched.readout.resolution = Resolution::MatchInput;
    let mut roomy = matched.clone();
    roomy.readout.long_edge = Some(240);
    let scene = photo(240, 180);
    assert!(
        render(&roomy, &scene).rgb() == render(&matched, &scene).rgb(),
        "a long edge at the input's own changes nothing"
    );

    let text = toml::to_string(&matched).unwrap();
    assert!(
        !text.contains("long_edge"),
        "an unset limit is not written:\n{text}"
    );
    let parsed: Preset = toml::from_str(&text).unwrap();
    assert_eq!(parsed.readout.long_edge, None);
}

#[test]
fn a_long_edge_round_trips_through_toml() {
    let text = "camera = \"a520\"\n[readout]\nresolution = \"match-input\"\nlong_edge = 400\n";
    let preset: Preset = toml::from_str(text).unwrap();
    preset.validate().unwrap();
    assert_eq!(preset.readout.long_edge, Some(400));
    let written = toml::to_string(&preset).unwrap();
    assert!(written.contains("long_edge = 400"), "{written}");
    let back: Preset = toml::from_str(&written).unwrap();
    assert_eq!(back, preset);
}

#[test]
fn validation_names_the_long_edge() {
    let mut native = Preset::new("a520");
    native.readout.long_edge = Some(400);
    let error = native.validate().unwrap_err();
    assert!(error.is_config(), "{error}");
    let message = error.to_string();
    assert!(
        message.contains("readout.long_edge") && message.contains("match-input"),
        "the native sensor has no long edge to limit: {message}"
    );
    let built = Simulator::builder((camera(), native.clone()))
        .input_size(640, 480)
        .build()
        .unwrap_err();
    assert!(built.to_string().contains("readout.long_edge"), "{built}");
    let derived = Timing::derive(&camera(), &native, 640, 480).unwrap_err();
    assert!(
        derived.to_string().contains("readout.long_edge"),
        "{derived}"
    );

    for edge in [
        0,
        Readout::MIN_LONG_EDGE - 1,
        Readout::MAX_LONG_EDGE + 1,
        u32::MAX,
    ] {
        let error = limited(Some(edge)).validate().unwrap_err();
        assert!(error.is_config(), "{edge}: {error}");
        assert!(
            error.to_string().contains("readout.long_edge"),
            "{edge}: {error}"
        );
    }
    for edge in [Readout::MIN_LONG_EDGE, 720, Readout::MAX_LONG_EDGE] {
        limited(Some(edge)).validate().unwrap();
    }
}

#[test]
fn a_new_long_edge_rebuilds_the_running_camera_only_when_the_sensor_changes() {
    let mut sim = Simulator::builder((camera(), limited(None)))
        .input_size(800, 600)
        .build()
        .unwrap();
    let scene = photo(800, 600);
    sim.frame(&scene, 0);
    assert_eq!(
        sim.set_preset(limited(Some(400))).unwrap(),
        Reconfigured::Rebuilt,
        "a smaller sensor is a new geometry"
    );
    assert_eq!(
        (sim.timing().sensor_width, sim.timing().sensor_height),
        (400, 300)
    );
    let frame = sim.frame(&scene, 0);
    assert_eq!((frame.width(), frame.height()), (400, 300));

    sim.set_scene_size(320, 240).unwrap();
    let kept = sim.set_preset(limited(Some(360))).unwrap();
    assert!(
        kept.state_kept(),
        "a limit the input does not reach keeps the camera running: {kept}"
    );
    assert_eq!(sim.timing().long_edge, Some(360));
    assert_eq!(
        sim.set_scene_size(800, 600).unwrap(),
        Reconfigured::Rebuilt,
        "a larger input meets the limit"
    );
    assert_eq!(
        (sim.timing().sensor_width, sim.timing().sensor_height),
        (360, 270)
    );
}

#[test]
fn the_timing_names_the_limit() {
    let t = Timing::derive(&camera(), &limited(Some(400)), 4000, 3000).unwrap();
    let text = t.to_string();
    assert!(
        text.contains("match-input, long edge at most 400 px"),
        "{text}"
    );
    assert!(text.contains("400 x 300 photosites"), "{text}");
    let plain = Timing::derive(&camera(), &limited(None), 4000, 3000).unwrap();
    assert!(!plain.to_string().contains("long edge"), "{plain}");
}
