//! Reconfiguring a running simulator: what `set_preset` and `set_scene_size` keep.

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::afe::{Inject, Wave};
use vcb_core::bus;
use vcb_core::preset::{AfeBend, Mode, Resolution};
use vcb_core::{Camera, Frame, Preset, Scene, Simulator};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The A520 with a small native sensor, so native-resolution frames render quickly.
fn small_camera() -> Camera {
    let mut camera = camera();
    camera.sensor.native_width = 96;
    camera.sensor.native_height = 72;
    camera
}

/// A shipped preset with the sensor set to the scene's size.
fn preset(name: &str) -> Preset {
    let path = format!("{}/../../presets/{name}.toml", env!("CARGO_MANIFEST_DIR"));
    let mut preset: Preset = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    preset.readout.resolution = Resolution::MatchInput;
    preset
}

fn build(camera: Camera, preset: Preset) -> Simulator {
    Simulator::builder((camera, preset))
        .input_size(WIDTH, HEIGHT)
        .build()
        .unwrap()
}

/// A diagonal gradient, so the bend's source lines switch along every line.
fn scene() -> Scene {
    let rgb: Vec<u8> = (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).flat_map(move |x| {
                let v = ((x * 255 / WIDTH + y * 128 / HEIGHT) % 256) as u8;
                [v, v / 2 + 60, 255 - v]
            })
        })
        .collect();
    Scene::from_srgb8(WIDTH, HEIGHT, &rgb)
}

/// Renders frames `0..count` and returns the last.
fn run(sim: &mut Simulator, scene: &Scene, count: u64) -> Frame {
    let mut last = None;
    for index in 0..count {
        last = Some(sim.frame(scene, index).clone());
    }
    last.expect("at least one frame")
}

fn with_pot(mut preset: Preset, pot_kohm: f64) -> Preset {
    preset.bend.bus.as_mut().unwrap().pot_kohm = pot_kohm;
    preset
}

#[test]
fn a_new_pot_value_keeps_the_capacitor_charge() {
    // With the pot on S the capacitor's charge leaks only through the gate inputs, so
    // it carries from frame to frame.
    let scene = scene();
    let mut sim = build(camera(), preset("a520-inv"));
    run(&mut sim, &scene, 3);
    let charged = sim.bus_state().cap_v();
    assert!(charged.abs() > 1e-3, "the capacitor holds only {charged} V");

    let turned = with_pot(preset("a520-inv"), 0.8);
    sim.set_preset(turned.clone()).unwrap();
    assert_eq!(sim.preset(), &turned);
    assert_eq!(
        sim.bus_state().cap_v().to_bits(),
        charged.to_bits(),
        "turning the pot moved the capacitor voltage"
    );

    let mut fresh = build(camera(), turned);
    let expected = fresh.frame(&scene, 3).clone();
    assert_ne!(
        sim.frame(&scene, 3),
        &expected,
        "the kept charge left no trace in the frame"
    );
    sim.reset();
    assert_eq!(
        sim.frame(&scene, 3),
        &expected,
        "after a reset the frame is a fresh simulator's"
    );
}

#[test]
fn new_source_pins_return_the_bend_to_power_on() {
    let scene = scene();
    let mut sim = build(camera(), preset("a520-hpf"));
    run(&mut sim, &scene, 2);
    assert_ne!(sim.bus_state(), &bus::State::default());
    let afe = sim.afe_state().clone();

    let mut rewired = preset("a520-hpf");
    rewired.bend.bus.as_mut().unwrap().source_pins = vec![10, 8];
    sim.set_preset(rewired).unwrap();
    assert_eq!(sim.bus_state(), &bus::State::default());
    assert_eq!(sim.afe_state(), &afe, "the AFE lost its state with the bus");

    // The same pins in another order are the same wiring.
    run(&mut sim, &scene, 2);
    let mut reordered = sim.preset().clone();
    reordered.bend.bus.as_mut().unwrap().source_pins = vec![8, 10];
    let kept = sim.bus_state().clone();
    sim.set_preset(reordered).unwrap();
    assert_eq!(sim.bus_state(), &kept);
}

#[test]
fn removing_and_restoring_the_bend_starts_it_from_power_on() {
    let scene = scene();
    let mut sim = build(camera(), preset("a520-div"));
    run(&mut sim, &scene, 2);
    let mut plain = preset("a520-div");
    plain.bend.bus = None;
    sim.set_preset(plain).unwrap();
    assert_eq!(sim.bus_state(), &bus::State::default());
    sim.frame(&scene, 2);
    sim.set_preset(preset("a520-div")).unwrap();
    assert_eq!(sim.bus_state(), &bus::State::default());
}

#[test]
fn a_new_waveform_continues_from_the_oscillator_phase() {
    let scene = scene();
    let inject = |wave| AfeBend {
        inject: Some(Inject {
            wave,
            freq_hz: 1234.0,
            amplitude_v: 0.05,
            phase_skew: 0.0,
        }),
        ..AfeBend::default()
    };
    let mut sine = preset("a520-clean");
    sine.bend.afe = Some(inject(Wave::Sine));
    let mut sim = build(camera(), sine);
    run(&mut sim, &scene, 2);
    let afe = sim.afe_state().clone();
    assert!(afe.phase() > 0.0);

    let mut square = preset("a520-clean");
    square.bend.afe = Some(inject(Wave::Square));
    sim.set_preset(square).unwrap();
    assert_eq!(sim.afe_state(), &afe, "the oscillator restarted");
}

#[test]
fn a_new_mode_rebuilds_as_a_fresh_simulator() {
    let scene = scene();
    let mut sim = build(camera(), preset("a520-hpf"));
    run(&mut sim, &scene, 2);
    let mut movie = preset("a520-hpf");
    movie.readout.mode = Mode::Movie;
    sim.set_preset(movie.clone()).unwrap();
    assert_eq!(sim.bus_state(), &bus::State::default());

    let mut fresh = build(camera(), movie);
    assert_eq!(sim.timing(), fresh.timing());
    let expected = fresh.frame(&scene, 2).clone();
    assert_eq!(sim.frame(&scene, 2), &expected);
    assert_eq!(
        (expected.width(), expected.height()),
        (fresh.timing().output_width, fresh.timing().output_height)
    );
}

#[test]
fn a_new_resolution_rebuilds_as_a_fresh_simulator() {
    let scene = scene();
    let mut sim = build(small_camera(), preset("a520-inv"));
    run(&mut sim, &scene, 2);
    let mut native = preset("a520-inv");
    native.readout.resolution = Resolution::Native;
    sim.set_preset(native.clone()).unwrap();
    assert_eq!(sim.timing().sensor_width, 96);

    let mut fresh = build(small_camera(), native);
    let expected = fresh.frame(&scene, 2).clone();
    assert_eq!(sim.frame(&scene, 2), &expected);
    assert_eq!((expected.width(), expected.height()), (96, 72));
}

#[test]
fn a_new_scene_size_at_match_input_rebuilds_for_that_size() {
    let small = Scene::from_srgb8(40, 30, &[120; 40 * 30 * 3]);
    let mut sim = build(camera(), preset("a520-div"));
    run(&mut sim, &scene(), 2);
    sim.set_scene_size(40, 30).unwrap();
    assert_eq!(sim.scene_size(), (40, 30));
    assert_eq!(sim.bus_state(), &bus::State::default());

    let mut fresh = Simulator::builder((camera(), preset("a520-div")))
        .input_size(40, 30)
        .build()
        .unwrap();
    let expected = fresh.frame(&small, 2).clone();
    assert_eq!(sim.frame(&small, 2), &expected);
    assert_eq!((expected.width(), expected.height()), (40, 30));
}

#[test]
fn at_native_resolution_a_new_scene_size_keeps_the_state() {
    let scene = scene();
    let mut native = preset("a520-hpf");
    native.readout.resolution = Resolution::Native;
    let mut sim = build(small_camera(), native);
    run(&mut sim, &scene, 2);
    let kept = sim.bus_state().clone();
    let timing = sim.timing().clone();
    // A landscape input of another size leaves the native readout as it is.
    sim.set_scene_size(80, 60).unwrap();
    assert_eq!(sim.timing(), &timing);
    assert_eq!(sim.bus_state(), &kept);
}

#[test]
fn a_preset_for_another_camera_is_rejected_and_the_simulator_runs_on() {
    let scene = scene();
    let mut sim = build(camera(), preset("a520-hpf"));
    run(&mut sim, &scene, 2);
    let mut reference = sim.clone();

    let error = sim.set_preset(Preset::new("s3is")).unwrap_err();
    assert!(error.is_config(), "{error}");
    let mut invalid = preset("a520-hpf");
    invalid.scene.headroom = 0.0;
    assert!(sim.set_preset(invalid).unwrap_err().is_config());
    assert!(sim.set_scene_size(1, 1).unwrap_err().is_input());

    assert_eq!(sim.preset(), reference.preset());
    assert_eq!(sim.scene_size(), (WIDTH, HEIGHT));
    let expected = reference.frame(&scene, 2).clone();
    assert_eq!(sim.frame(&scene, 2), &expected);
}

#[test]
fn the_accessors_return_the_configuration() {
    let camera = camera();
    let sim = build(camera.clone(), preset("a520-clean"));
    assert_eq!(sim.camera(), &camera);
    assert_eq!(sim.preset(), &preset("a520-clean"));
    assert_eq!(sim.scene_size(), (WIDTH, HEIGHT));

    let native = Simulator::builder((camera.clone(), Preset::new("a520")))
        .build()
        .unwrap();
    assert_eq!(
        native.scene_size(),
        (camera.sensor.native_width, camera.sensor.native_height)
    );
}
