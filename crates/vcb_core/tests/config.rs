//! Parsing, round-tripping and validation of camera and preset files.

#![cfg(feature = "serde")]

use std::path::PathBuf;

use vcb_core::afe::{Inject, Wave};
use vcb_core::bus::{Function, Target};
use vcb_core::codec::Quality;
use vcb_core::isp::WbMode;
use vcb_core::preset::{AfeBend, BusBend, ClockBend, IspOverride, Mode, Orientation, Resolution};
use vcb_core::{Camera, Preset};

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn camera() -> Camera {
    let text = std::fs::read_to_string(root().join("cameras/a520.toml")).unwrap();
    toml::from_str(&text).unwrap()
}

fn preset_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root().join("presets"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    files
}

fn full_preset() -> Preset {
    let mut preset = Preset::new("a520");
    preset.seed = 77;
    preset.readout.mode = Mode::Movie;
    preset.readout.resolution = Resolution::Native;
    preset.readout.orientation = Orientation::Portrait;
    preset.bend.bus = Some(BusBend {
        source_pins: vec![9, 8, 7],
        target: Target::Gnd,
        function: Function::Divide,
        cap_uf: 0.47,
        pot_kohm: 22.0,
        series_ohm: 47.0,
        common_bus: true,
    });
    preset.bend.clocks = Some(ClockBend {
        cte: Some(0.9995),
        v_pulse_drop_hz: 3.0,
        h_glitch_hz: 0.5,
    });
    preset.bend.afe = Some(AfeBend {
        inject: Some(Inject {
            wave: Wave::Saw,
            freq_hz: 1200.0,
            amplitude_v: 0.15,
        }),
    });
    preset.isp = Some(IspOverride {
        wb: Some(WbMode::Auto),
        xyz_to_cam: Some([
            [0.9877, -0.3775, -0.0871],
            [-0.7613, 1.4807, 0.3072],
            [-0.1448, 0.1305, 0.7485],
        ]),
        sharpen_gain: Some(1.2),
        ..IspOverride::default()
    });
    preset
}

#[test]
fn camera_file_parses_and_validates() {
    let camera = camera();
    camera.validate().unwrap();
    assert_eq!(camera.info.name, "a520");
}

#[test]
fn camera_tables_are_the_measured_canon_tables() {
    let camera = camera();
    let tables = &camera.jpeg.tables;
    // Quantiser steps grow from superfine to normal at every frequency.
    for v in 0..8 {
        for u in 0..8 {
            assert!(tables.superfine.luma[v][u] <= tables.fine.luma[v][u]);
            assert!(tables.fine.luma[v][u] <= tables.normal.luma[v][u]);
            assert!(tables.superfine.chroma[v][u] <= tables.fine.chroma[v][u]);
        }
    }
    assert_eq!(tables.get(Quality::Normal).chroma[7][7], 79);
    assert_eq!(tables.get(camera.still.jpeg), &tables.superfine);
}

#[test]
fn camera_round_trips_through_toml() {
    let camera = camera();
    let text = toml::to_string(&camera).unwrap();
    let back: Camera = toml::from_str(&text).unwrap();
    assert_eq!(back, camera);
}

#[test]
fn every_preset_file_parses_validates_and_names_the_camera() {
    let files = preset_files();
    assert!(
        files.len() >= 4,
        "expected the A520 presets, found {files:?}"
    );
    let camera = camera();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let preset: Preset = toml::from_str(&text)
            .unwrap_or_else(|error| panic!("{} does not parse: {error}", path.display()));
        preset.validate().unwrap();
        assert_eq!(preset.camera, camera.info.name, "{}", path.display());
        let back: Preset = toml::from_str(&toml::to_string(&preset).unwrap()).unwrap();
        assert_eq!(back, preset, "{} does not round-trip", path.display());
    }
}

#[test]
fn preset_with_every_section_round_trips_through_toml() {
    let preset = full_preset();
    preset.validate().unwrap();
    let text = toml::to_string(&preset).unwrap();
    assert!(text.contains("target = \"gnd\""), "{text}");
    let back: Preset = toml::from_str(&text).unwrap();
    assert_eq!(back, preset);
}

#[test]
fn minimal_preset_is_an_unmodified_still_camera() {
    let preset: Preset = toml::from_str("camera = \"a520\"").unwrap();
    assert_eq!(preset, Preset::new("a520"));
    assert_eq!(preset.readout.mode, Mode::Still);
    assert_eq!(preset.readout.resolution, Resolution::MatchInput);
}

#[test]
fn plan_example_parses() {
    let text = r#"
        camera = "a520"
        seed = 1
        [readout]
        mode = "still"
        resolution = "match-input"
        [bend.bus]
        source_pins = [9, 8, 7]
        target = "pin:10"
        function = "bypass"
        cap_uf = 1.0
        pot_kohm = 10.0
        series_ohm = 0.0
        common_bus = false
        [bend.clocks]
        cte = 0.9995
        [bend.afe.inject]
        wave = "square"
        freq_hz = 1200.0
        amplitude_v = 0.15
    "#;
    let preset: Preset = toml::from_str(text).unwrap();
    let bus = preset.bend.bus.unwrap();
    assert_eq!(bus.target, Target::Pin(10));
    assert_eq!(bus.function, Function::Bypass);
    let clocks = preset.bend.clocks.unwrap();
    assert_eq!(clocks.cte, Some(0.9995));
    assert!(clocks.v_pulse_drop_hz.abs() < f64::EPSILON);
    let inject = preset.bend.afe.unwrap().inject.unwrap();
    assert_eq!(inject.wave, Wave::Square);
}

#[test]
fn unknown_keys_and_bad_targets_are_rejected() {
    let unknown = toml::from_str::<Preset>("camera = \"a520\"\nsede = 3").unwrap_err();
    assert!(unknown.to_string().contains("sede"), "{unknown}");
    let bad_target = "camera = \"a520\"\n[bend.bus]\nsource_pins = [1]\ntarget = \"pin:x\"\n\
                      function = \"bypass\"\ncap_uf = 1.0\npot_kohm = 1.0";
    let target = toml::from_str::<Preset>(bad_target).unwrap_err();
    assert!(target.to_string().contains("pin:x"), "{target}");
    assert!("pin:3".parse::<Target>().is_ok_and(|t| t == Target::Pin(3)));
    assert!("vcc".parse::<Target>().unwrap_err().is_config());
    assert!("sideways".parse::<Mode>().unwrap_err().is_config());
}

#[test]
fn camera_validation_rejects_unphysical_values() {
    let mut camera = camera();
    camera.sensor.full_well_e = -1.0;
    assert!(camera.validate().unwrap_err().is_config());

    let mut camera = self::camera();
    camera.afe.pedestal_v *= 3.0;
    let error = camera.validate().unwrap_err();
    assert!(error.is_config());
    assert!(error.to_string().contains("black_level"), "{error}");

    let mut camera = self::camera();
    camera.sensor.native_width += 1;
    assert!(camera.validate().unwrap_err().is_config());

    let mut camera = self::camera();
    camera.adc.dsp_lsb = camera.adc.bits - camera.adc.dsp_bits + 1;
    assert!(camera.validate().unwrap_err().is_config());

    let mut camera = self::camera();
    camera.jpeg.tables.fine.chroma[3][4] = 0;
    assert!(camera.validate().unwrap_err().is_config());

    let mut camera = self::camera();
    camera.isp.xyz_to_cam = [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    assert!(camera.validate().unwrap_err().is_config());
}

#[test]
fn preset_validation_rejects_unphysical_values() {
    let mut preset = full_preset();
    if let Some(bus) = preset.bend.bus.as_mut() {
        bus.cap_uf = -1.0;
    }
    assert!(preset.validate().unwrap_err().is_config());

    let mut preset = full_preset();
    preset.bend.clocks = Some(ClockBend {
        cte: Some(1.5),
        ..ClockBend::default()
    });
    assert!(preset.validate().unwrap_err().is_config());
}

#[test]
fn optional_camera_fields_default_to_an_even_lens_unit_one_and_fixed_exposure() {
    let text = std::fs::read_to_string(root().join("cameras/a520.toml")).unwrap();
    let mut file: toml::Table = toml::from_str(&text).unwrap();
    file.remove("lens");
    let table = |file: &mut toml::Table, name: &str| {
        file.get_mut(name)
            .and_then(toml::Value::as_table_mut)
            .unwrap()
            .clone()
    };
    let mut sensor = table(&mut file, "sensor");
    sensor.remove("unit_seed");
    file.insert("sensor".into(), sensor.into());
    let mut isp = table(&mut file, "isp");
    isp.remove("ae_mode");
    isp.remove("demosaic");
    file.insert("isp".into(), isp.into());
    let camera: Camera = toml::from_str(&toml::to_string(&file).unwrap()).unwrap();
    camera.validate().unwrap();
    assert!(camera.lens.vignette_stops.abs() < f64::EPSILON);
    assert_eq!(camera.sensor.unit_seed, 1);
    assert_eq!(camera.isp.ae_mode, vcb_core::isp::AeMode::Fixed);
    assert_eq!(camera.isp.demosaic, vcb_core::isp::Demosaic::Malvar);

    let mut camera = self::camera();
    camera.lens.vignette_stops = -0.5;
    assert!(camera.validate().unwrap_err().is_config());
}

#[test]
fn scene_section_parses_and_rejects_unphysical_highlights() {
    let preset: Preset = toml::from_str("camera = \"a520\"\n[scene]\nheadroom = 4.0").unwrap();
    preset.validate().unwrap();
    assert!((preset.scene.headroom - 4.0).abs() < f64::EPSILON);
    let default = Preset::new("a520").scene;
    assert!((preset.scene.shoulder_start - default.shoulder_start).abs() < f64::EPSILON);
    let mut dim = preset.clone();
    dim.scene.headroom = 0.5;
    assert!(dim.validate().unwrap_err().is_config());
    let mut late = preset;
    late.scene.shoulder_start = 1.0;
    assert!(late.validate().unwrap_err().is_config());
}
