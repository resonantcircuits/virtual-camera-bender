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

/// A smooth sRGB test chart: horizontal red ramp, vertical green ramp, diagonal blue.
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

#[test]
fn flat_grey_scene_develops_to_a_flat_frame() {
    let (width, height) = (96, 64);
    let scene = Scene::from_srgb8(width, height, &vec![128; (width * height * 3) as usize]);
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
fn the_same_frame_index_renders_identical_bytes() {
    let (width, height) = (128, 96);
    let scene = Scene::from_srgb8(width, height, &gradient(width, height));
    let mut sim = simulator(camera(), preset_file("a520-clean"), width, height);
    let first: Frame = sim.frame(&scene, 3).clone();
    assert_eq!(sim.frame(&scene, 3), &first);
    sim.reset();
    assert_eq!(sim.frame(&scene, 3), &first);
    let mut other = simulator(camera(), preset_file("a520-clean"), width, height);
    assert_eq!(other.frame(&scene, 3), &first);
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
    let mut red = vec![0u8; (width * height * 3) as usize];
    for pixel in red.chunks_exact_mut(3) {
        pixel[0] = 220;
    }
    let mut preset = preset_file("a520-clean");
    preset.readout.mode = Mode::Movie;
    let mut sim = simulator(camera(), preset, width, height);
    let frame = sim.frame(&Scene::from_srgb8(width, height, &red), 0);
    let [r, g, b] = frame.pixel(frame.width() / 2, frame.height() / 2);
    assert!(
        r.abs_diff(220) <= 1 && g <= 1 && b <= 1,
        "red became {:?}",
        [r, g, b]
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
    let frame = sim.frame(&Scene::from_srgb8(4, 3, &[100; 36]), 0);
    assert_eq!((frame.width(), frame.height()), size);
    assert!(frame.rgb().iter().all(|&c| c.abs_diff(100) <= 1));
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
fn highlights_clip_at_full_well() {
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
