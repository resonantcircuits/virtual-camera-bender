//! End-to-end properties of the simulated signal chain.

#![cfg(feature = "serde")]

use vcb_core::preset::{Mode, Resolution};
use vcb_core::readout::Direction;
use vcb_core::{Camera, Frame, Preset, Scene, Simulator};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn preset_file(name: &str) -> Preset {
    let path = format!("{}/../../presets/{name}.toml", env!("CARGO_MANIFEST_DIR"));
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn simulator(camera: Camera, preset: Preset, width: u32, height: u32) -> Simulator {
    Simulator::builder((camera, preset))
        .input_size(width, height)
        .build()
        .unwrap()
}

/// A smooth sRGB test chart inside the sensor gamut: red, green and blue ramps.
fn gradient(width: u32, height: u32) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let fx = f64::from(x) / f64::from(width - 1);
            let fy = f64::from(y) / f64::from(height - 1);
            rgb.extend([
                (fx * 255.0).round() as u8,
                (fy * 255.0).round() as u8,
                ((1.0 - 0.5 * (fx + fy)) * 255.0).round() as u8,
            ]);
        }
    }
    rgb
}

fn uniform(width: u32, height: u32, rgb: [u8; 3]) -> Scene {
    let bytes: Vec<u8> = (0..width * height).flat_map(|_| rgb).collect();
    Scene::from_srgb8(width, height, &bytes)
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let mse = a
        .iter()
        .zip(b)
        .map(|(&p, &q)| (f64::from(p) - f64::from(q)).powi(2))
        .sum::<f64>()
        / a.len() as f64;
    10.0 * (255.0f64.powi(2) / mse.max(1e-12)).log10()
}

/// Returns the centroid of the pixels brighter than mid-grey in every channel.
fn bright_centroid(frame: &Frame) -> (f64, f64) {
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            if frame.pixel(x, y).iter().all(|&c| c > 128) {
                sx += f64::from(x);
                sy += f64::from(y);
                n += 1.0;
            }
        }
    }
    assert!(n > 0.0, "no bright pixels in the frame");
    (sx / n, sy / n)
}

/// A dark portrait scene with a white square centred at `(45, 55)` of 300 by 400.
fn marked_portrait() -> Scene {
    let (width, height) = (300, 400);
    let mut scene = uniform(width, height, [20, 20, 20]);
    for plane in scene.planes_mut() {
        for y in 40..70 {
            for x in 30..60 {
                plane.set(x, y, 1.0);
            }
        }
    }
    scene
}

#[test]
fn flat_grey_scene_develops_to_a_flat_frame() {
    let (width, height) = (96, 64);
    let scene = uniform(width, height, [128, 128, 128]);
    let mut sim = simulator(camera(), preset_file("a520-clean"), width, height);
    let frame = sim.frame(&scene, 0);
    let first = frame.pixel(0, 0);
    assert!(
        frame.rgb().chunks_exact(3).all(|pixel| pixel == first),
        "frame is not flat"
    );
    assert!(
        first.iter().all(|&c| c.abs_diff(128) <= 1),
        "grey became {first:?}"
    );
}

#[test]
fn smooth_gradient_round_trips_through_the_clean_camera() {
    let (width, height) = (256, 192);
    let input = gradient(width, height);
    let scene = Scene::from_srgb8(width, height, &input);
    let mut sim = simulator(camera(), preset_file("a520-clean"), width, height);
    let frame = sim.frame(&scene, 0);
    assert_eq!((frame.width(), frame.height()), (width, height));
    let quality = psnr(&input, frame.rgb());
    assert!(quality > 35.0, "PSNR {quality:.2} dB");
}

#[test]
fn the_same_sequence_from_reset_renders_identical_bytes() {
    let (width, height) = (128, 96);
    let scene = Scene::from_srgb8(width, height, &gradient(width, height));
    let mut sim = simulator(camera(), preset_file("a520-clean"), width, height);
    let first: Vec<Frame> = (0..3).map(|i| sim.frame(&scene, i).clone()).collect();
    sim.reset();
    let again: Vec<Frame> = (0..3).map(|i| sim.frame(&scene, i).clone()).collect();
    assert_eq!(again, first);
    let mut other = simulator(camera(), preset_file("a520-clean"), width, height);
    let fresh: Vec<Frame> = (0..3).map(|i| other.frame(&scene, i).clone()).collect();
    assert_eq!(fresh, first);
}

#[test]
fn movie_mode_bins_columns_and_skips_lines() {
    let camera = camera();
    let (h_bin, skip) = (camera.movie.h_bin, camera.movie.line_skip);
    let (width, height) = (256, 192);
    let mut preset = preset_file("a520-clean");
    preset.readout.mode = Mode::Movie;
    let mut sim = simulator(camera, preset, width, height);
    let scene = Scene::from_srgb8(width, height, &gradient(width, height));
    let frame = sim.frame(&scene, 0);
    assert_eq!(
        (frame.width(), frame.height()),
        (width / h_bin, height / skip)
    );
}

#[test]
fn movie_mode_keeps_the_bayer_phase() {
    let (width, height) = (128, 96);
    let colour = [200, 120, 60];
    let mut preset = preset_file("a520-clean");
    preset.readout.mode = Mode::Movie;
    let mut sim = simulator(camera(), preset, width, height);
    let frame = sim.frame(&uniform(width, height, colour), 0);
    let pixel = frame.pixel(frame.width() / 2, frame.height() / 2);
    assert!(
        pixel
            .iter()
            .zip(colour)
            .all(|(&got, want)| got.abs_diff(want) <= 2),
        "{colour:?} became {pixel:?}"
    );
}

#[test]
fn native_movie_mode_delivers_the_camera_movie_size() {
    let camera = camera();
    let size = (camera.movie.output_width, camera.movie.output_height);
    let mut preset = preset_file("a520-clean");
    preset.readout.mode = Mode::Movie;
    preset.readout.resolution = Resolution::Native;
    let mut sim = Simulator::builder((camera, preset)).build().unwrap();
    let frame = sim.frame(&uniform(4, 3, [100, 100, 100]), 0);
    assert_eq!((frame.width(), frame.height()), size);
    assert!(frame.rgb().iter().all(|&c| c.abs_diff(100) <= 1));
}

#[test]
fn portrait_input_in_native_mode_develops_upright_on_the_rotated_sensor() {
    let camera = camera();
    let (native_w, native_h) = (camera.sensor.native_width, camera.sensor.native_height);
    let mut preset = preset_file("a520-clean");
    preset.readout.resolution = Resolution::Native;
    let scene = marked_portrait();
    let mut sim = Simulator::builder((camera, preset))
        .input_size(scene.width(), scene.height())
        .build()
        .unwrap();
    assert!(sim.timing().rotated);
    let frame = sim.frame(&scene, 0);
    assert_eq!((frame.width(), frame.height()), (native_h, native_w));
    let scale = f64::from(native_h) / f64::from(scene.width());
    let (cx, cy) = bright_centroid(frame);
    assert!(
        (cx - 45.0 * scale).abs() < 2.0 && (cy - 55.0 * scale).abs() < 2.0,
        "mark moved to ({cx:.1}, {cy:.1}), expected ({:.1}, {:.1})",
        45.0 * scale,
        55.0 * scale
    );
}

#[test]
fn portrait_input_at_match_input_keeps_its_size_and_marks() {
    let scene = marked_portrait();
    let mut sim = simulator(
        camera(),
        preset_file("a520-clean"),
        scene.width(),
        scene.height(),
    );
    let frame = sim.frame(&scene, 0);
    assert_eq!((frame.width(), frame.height()), (300, 400));
    let (cx, cy) = bright_centroid(frame);
    assert!(
        (cx - 44.5).abs() < 0.5 && (cy - 54.5).abs() < 0.5,
        "mark moved to ({cx:.2}, {cy:.2})"
    );
    let input = scene.to_srgb8();
    assert!(psnr(&input, frame.rgb()) > 30.0);
}

#[test]
fn readout_direction_changes_time_order_not_the_image() {
    let (width, height) = (64, 48);
    let scene = Scene::from_srgb8(width, height, &gradient(width, height));
    let top_down = simulator(camera(), preset_file("a520-clean"), width, height)
        .frame(&scene, 0)
        .clone();
    let mut flipped = camera();
    flipped.still.direction = Direction::BottomUp;
    let mut sim = simulator(flipped, preset_file("a520-clean"), width, height);
    assert_eq!(sim.frame(&scene, 0), &top_down);
}

#[test]
fn clipped_highlights_stay_white() {
    let (width, height) = (32, 32);
    let mut scene = Scene::new(width, height);
    for plane in scene.planes_mut() {
        plane.fill(4.0);
    }
    let mut sim = simulator(camera(), preset_file("a520-clean"), width, height);
    let frame = sim.frame(&scene, 0);
    assert!(frame.rgb().iter().all(|&c| c == 255));
}

#[test]
fn every_preset_builds_a_simulator() {
    for name in ["a520-clean", "a520-hpf", "a520-div", "a520-inv"] {
        let mut sim = simulator(camera(), preset_file(name), 64, 48);
        let frame = sim.frame(&Scene::new(64, 48), 0);
        assert_eq!((frame.width(), frame.height()), (64, 48), "{name}");
    }
}

#[test]
fn build_rejects_inconsistent_configurations() {
    let camera = camera();
    let build = |preset: Preset| {
        Simulator::builder((camera.clone(), preset))
            .input_size(64, 48)
            .build()
            .unwrap_err()
    };

    assert!(build(Preset::new("g2")).is_config());

    let mut wide_pin = preset_file("a520-hpf");
    if let Some(bus) = wide_pin.bend.bus.as_mut() {
        bus.source_pins.push(camera.adc.bits);
    }
    assert!(build(wide_pin).is_config());

    let mut twice = preset_file("a520-hpf");
    if let Some(bus) = twice.bend.bus.as_mut() {
        bus.source_pins.push(bus.source_pins[0]);
    }
    assert!(build(twice).is_config());

    let mut negative = preset_file("a520-hpf");
    if let Some(bus) = negative.bend.bus.as_mut() {
        bus.pot_kohm = -10.0;
    }
    assert!(build(negative).is_config());

    let no_size = Simulator::builder((camera.clone(), Preset::new("a520"))).build();
    assert!(no_size.unwrap_err().is_config());

    let tiny = Simulator::builder((camera, Preset::new("a520")))
        .input_size(1, 1)
        .build();
    assert!(tiny.unwrap_err().is_input());
}
