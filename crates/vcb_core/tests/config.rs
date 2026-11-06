//! Parsing, round-tripping and validation of camera and preset files.

#![cfg(feature = "serde")]

use std::path::PathBuf;

use vcb_core::afe::{GainMod, Inject, Wave};
use vcb_core::bus::{Function, PotNode, Target};
use vcb_core::codec::Quality;
use vcb_core::isp::WbMode;
use vcb_core::memory;
use vcb_core::preset::{
    AddressFault, AfeBend, BusBend, ClockBend, FaultKind, IspOverride, MemoryBend, Mode,
    OpticsBend, Orientation, RailBend, ReclockBend, Resolution, SensorOverride,
};
use vcb_core::{Camera, Preset, Simulator, Timing};

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
    preset.sensor = Some(SensorOverride {
        exposure_s: Some(0.02),
    });
    preset.bend.bus = Some(BusBend {
        source_pins: vec![9, 8, 7],
        target: Target::Gnd,
        function: Function::Divide,
        cap_uf: 0.47,
        pot_kohm: 22.0,
        pot_node: PotNode::S,
        series_ohm: 47.0,
        common_bus: true,
    });
    preset.bend.clocks = Some(ClockBend {
        cte: Some(0.9995),
        v_pulse_drop_hz: 3.0,
        v_stall_lines: 2.5,
        h_glitch_hz: 0.5,
        h_shift_frac: 0.1,
    });
    preset.bend.afe = Some(AfeBend {
        inject: Some(Inject {
            wave: Wave::Saw,
            freq_hz: 1200.0,
            amplitude_v: 0.15,
            phase_skew: 0.01,
        }),
        gain_mod: Some(GainMod {
            wave: Wave::Sine,
            freq_hz: 30.0,
            depth: 0.2,
        }),
        cds_lag_px: 1.5,
        cds_mix: 0.4,
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
    assert_eq!(preset.readout.resolution, Resolution::Native);
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

    let mut camera = self::camera();
    camera.sensor.nominal_exposure_s = 0.0;
    assert!(camera.validate().unwrap_err().is_config());

    // The electronic shutter cannot integrate for longer than a frame.
    let mut camera = self::camera();
    camera.movie.exposure_s = 1.5 / camera.movie.fps;
    let error = camera.validate().unwrap_err();
    assert!(error.to_string().contains("movie.exposure_s"), "{error}");
}

#[test]
fn the_sensor_section_sets_the_exposure_time() {
    let preset: Preset = toml::from_str("camera = \"a520\"\n[sensor]\nexposure_s = 0.25").unwrap();
    preset.validate().unwrap();
    assert_eq!(preset.sensor.and_then(|s| s.exposure_s), Some(0.25));
    let empty: Preset = toml::from_str("camera = \"a520\"\n[sensor]").unwrap();
    assert_eq!(empty.sensor, Some(SensorOverride::default()));
    let text = toml::to_string(&Preset::new("a520")).unwrap();
    assert!(!text.contains("sensor"), "{text}");
    let unknown = toml::from_str::<Preset>("camera = \"a520\"\n[sensor]\niso = 100").unwrap_err();
    assert!(unknown.to_string().contains("iso"), "{unknown}");
    for exposure_s in [0.0, -0.01, f64::INFINITY, f64::NAN] {
        let mut preset = preset.clone();
        preset.sensor = Some(SensorOverride {
            exposure_s: Some(exposure_s),
        });
        let error = preset.validate().unwrap_err();
        assert!(error.is_config(), "{exposure_s}: {error}");
    }
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
fn analog_bends_parse_as_inline_tables_with_defaults() {
    let text = r#"
        camera = "a520"
        [bend.clocks]
        v_pulse_drop_hz = 2.0
        h_glitch_hz = 1.0
        [bend.afe]
        inject = { wave = "noise", freq_hz = 300.0, amplitude_v = 0.1 }
        gain_mod = { wave = "sine", freq_hz = 12.0, depth = 0.25 }
        cds_lag_px = 2.0
        cds_mix = 0.5
    "#;
    let preset: Preset = toml::from_str(text).unwrap();
    preset.validate().unwrap();
    let clocks = preset.bend.clocks.unwrap();
    let defaults = ClockBend::default();
    assert!(clocks.cte.is_none());
    assert!((clocks.v_stall_lines - defaults.v_stall_lines).abs() < f64::EPSILON);
    assert!((clocks.h_shift_frac - defaults.h_shift_frac).abs() < f64::EPSILON);
    let afe = preset.bend.afe.unwrap();
    let inject = afe.inject.unwrap();
    assert_eq!(inject.wave, Wave::Noise);
    assert!(inject.phase_skew.abs() < f64::EPSILON);
    assert_eq!(afe.gain_mod.unwrap().wave, Wave::Sine);
}

#[test]
fn analog_bend_validation_rejects_unphysical_values() {
    let broken: [fn(&mut Preset); 5] = [
        |p| p.bend.clocks.as_mut().unwrap().v_stall_lines = 0.5,
        |p| p.bend.clocks.as_mut().unwrap().h_shift_frac = 1.5,
        |p| p.bend.afe.as_mut().unwrap().cds_mix = -0.1,
        |p| p.bend.afe.as_mut().unwrap().cds_lag_px = f64::NAN,
        |p| {
            if let Some(gain_mod) = p.bend.afe.as_mut().unwrap().gain_mod.as_mut() {
                gain_mod.depth = 1.5;
            }
        },
    ];
    for (case, breaks) in broken.iter().enumerate() {
        let mut preset = full_preset();
        breaks(&mut preset);
        let error = preset.validate().unwrap_err();
        assert!(error.is_config(), "case {case}: {error}");
    }

    let mut camera = camera();
    camera.afe.noise_1f_corner_hz = -1.0;
    assert!(camera.validate().unwrap_err().is_config());
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

/// A solid fault of `kind` on `pin`.
fn solid(pin: u8, kind: FaultKind) -> AddressFault {
    AddressFault {
        pin,
        kind,
        with: None,
        on_frac: None,
        switch_hz: None,
    }
}

/// A preset with the optics, rail, clock and memory sections away from their defaults.
fn milestone3_preset() -> Preset {
    let mut preset = Preset::new("a520");
    preset.bend.optics = Some(OpticsBend {
        ir_cut_pulled_frac: 0.4,
        nir_weights: [0.6, 0.3, 0.02],
        foliage_nir: 5.0,
        nir_scale: 1.5,
    });
    preset.bend.rail = Some(RailBend {
        load_ma: 150.0,
        spike_ma: 400.0,
        spike_hz: 3.0,
        spike_ms: 1.5,
        ripple_mv: 40.0,
        ripple_hz: 120.0,
        source_ohm: Some(2.0),
    });
    preset.bend.clock = Some(ReclockBend {
        osc_mhz: 26.4,
        drift_ppm: 300.0,
        drift_hz: 0.25,
        ppm_per_v: -800.0,
        hd_lock_px: 12.0,
    });
    preset.bend.memory = Some(MemoryBend {
        faults: vec![
            solid(3, FaultKind::High),
            AddressFault {
                pin: 5,
                kind: FaultKind::Bridge,
                with: Some(6),
                on_frac: Some(0.3),
                switch_hz: Some(4.0),
            },
            AddressFault {
                on_frac: Some(1.0),
                switch_hz: Some(0.0),
                ..solid(0, FaultKind::Low)
            },
        ],
    });
    preset
}

/// Copies one section from the second preset into the first.
type CopySection = (&'static str, fn(&mut Preset, &Preset));

#[test]
fn milestone3_sections_round_trip_through_toml() {
    let preset = milestone3_preset();
    preset.validate().unwrap();
    let text = toml::to_string(&preset).unwrap();
    for header in [
        "[bend.optics]",
        "[bend.rail]",
        "[bend.clock]",
        "[[bend.memory.faults]]",
        "kind = \"bridge\"",
    ] {
        assert!(text.contains(header), "missing {header} in\n{text}");
    }
    let back: Preset = toml::from_str(&text).unwrap();
    assert_eq!(back, preset);

    // Each section on its own; the absent sections stay absent.
    let sections: [CopySection; 4] = [
        ("optics", |p, from| p.bend.optics = from.bend.optics),
        ("rail", |p, from| p.bend.rail = from.bend.rail),
        ("clock", |p, from| p.bend.clock = from.bend.clock),
        ("memory", |p, from| {
            p.bend.memory.clone_from(&from.bend.memory);
        }),
    ];
    for (name, set) in sections {
        let mut alone = Preset::new("a520");
        set(&mut alone, &preset);
        let text = toml::to_string(&alone).unwrap();
        for (other, _) in sections {
            let written = text.contains(&format!("bend.{other}"));
            assert_eq!(written, other == name, "{name} alone:\n{text}");
        }
        let back: Preset = toml::from_str(&text).unwrap();
        assert_eq!(back, alone, "{name} alone");
    }
    let plain = toml::to_string(&Preset::new("a520")).unwrap();
    for (name, _) in sections {
        assert!(!plain.contains(&format!("bend.{name}")), "{plain}");
    }
}

#[test]
fn milestone3_sections_parse_with_their_defaults() {
    let text = r#"
        camera = "a520"
        [bend.optics]
        [bend.rail]
        [bend.clock]
        osc_mhz = 27.0
        [bend.memory]
    "#;
    let preset: Preset = toml::from_str(text).unwrap();
    preset.validate().unwrap();
    assert_eq!(preset.bend.optics, Some(OpticsBend::default()));
    assert_eq!(preset.bend.rail, Some(RailBend::default()));
    assert_eq!(preset.bend.clock, Some(ReclockBend::new(27.0)));
    assert_eq!(preset.bend.memory, Some(MemoryBend::default()));
    let optics = OpticsBend::default();
    assert!(
        (optics.ir_cut_pulled_frac - 1.0).abs() < f64::EPSILON,
        "the default filter is fully removed"
    );
    let rail = RailBend::default();
    assert!(rail.load_ma.abs() + rail.spike_ma.abs() + rail.ripple_mv.abs() < f64::EPSILON);
    assert!(
        rail.source_ohm.is_none(),
        "the camera's source impedance stays"
    );

    let faults = r#"
        camera = "a520"
        [[bend.memory.faults]]
        pin = 4
        kind = "low"
        [[bend.memory.faults]]
        pin = 7
        kind = "bridge"
        with = 9
    "#;
    let preset: Preset = toml::from_str(faults).unwrap();
    preset.validate().unwrap();
    let memory = preset.bend.memory.unwrap();
    assert_eq!(memory.faults[0], solid(4, FaultKind::Low));
    assert_eq!(memory.faults[1].kind, FaultKind::Bridge);
    assert_eq!(memory.faults[1].with, Some(9));
}

#[test]
fn milestone3_sections_reject_missing_and_unknown_keys() {
    let cases = [
        ("[bend.clock]\ndrift_ppm = 10.0", "osc_mhz"),
        ("[bend.optics]\nir_cut = 1.0", "ir_cut"),
        ("[bend.rail]\nload_a = 1.0", "load_a"),
        ("[bend.clock]\nosc_mhz = 27.0\nosc_hz = 1.0", "osc_hz"),
        ("[bend.memory]\nfault = []", "fault"),
        ("[[bend.memory.faults]]\npin = 1\nkind = \"stuck\"", "stuck"),
        ("[[bend.memory.faults]]\nkind = \"low\"", "pin"),
        (
            "[[bend.memory.faults]]\npin = 1\nkind = \"low\"\nwidth = 2",
            "width",
        ),
    ];
    for (section, needle) in cases {
        let text = format!("camera = \"a520\"\n{section}");
        let error = toml::from_str::<Preset>(&text).unwrap_err();
        assert!(error.to_string().contains(needle), "{section}: {error}");
    }
}

/// Breaks one value of a preset.
type BreakPreset = fn(&mut Preset);

fn optics_of(preset: &mut Preset) -> &mut OpticsBend {
    preset.bend.optics.as_mut().unwrap()
}

fn rail_of(preset: &mut Preset) -> &mut RailBend {
    preset.bend.rail.as_mut().unwrap()
}

fn clock_of(preset: &mut Preset) -> &mut ReclockBend {
    preset.bend.clock.as_mut().unwrap()
}

fn faults_of(preset: &mut Preset) -> &mut Vec<AddressFault> {
    &mut preset.bend.memory.as_mut().unwrap().faults
}

#[test]
fn milestone3_validation_names_the_offending_field() {
    let cases: [(BreakPreset, &str); 30] = [
        (
            |p| optics_of(p).ir_cut_pulled_frac = 1.5,
            "bend.optics.ir_cut_pulled_frac",
        ),
        (
            |p| optics_of(p).ir_cut_pulled_frac = -0.1,
            "bend.optics.ir_cut_pulled_frac",
        ),
        (
            |p| optics_of(p).nir_weights[1] = -0.1,
            "bend.optics.nir_weights",
        ),
        (
            |p| optics_of(p).nir_weights[2] = f64::NAN,
            "bend.optics.nir_weights",
        ),
        (
            |p| optics_of(p).foliage_nir = -1.0,
            "bend.optics.foliage_nir",
        ),
        (
            |p| optics_of(p).nir_scale = f64::INFINITY,
            "bend.optics.nir_scale",
        ),
        (|p| rail_of(p).load_ma = -1.0, "bend.rail.load_ma"),
        (|p| rail_of(p).spike_ma = f64::NAN, "bend.rail.spike_ma"),
        (|p| rail_of(p).spike_hz = -0.5, "bend.rail.spike_hz"),
        (|p| rail_of(p).spike_ms = 0.0, "bend.rail.spike_ms"),
        (|p| rail_of(p).ripple_mv = -3.0, "bend.rail.ripple_mv"),
        (|p| rail_of(p).ripple_hz = 0.0, "bend.rail.ripple_hz"),
        (
            |p| rail_of(p).source_ohm = Some(0.0),
            "bend.rail.source_ohm",
        ),
        (|p| clock_of(p).osc_mhz = 0.0, "bend.clock.osc_mhz"),
        (|p| clock_of(p).osc_mhz = f64::NAN, "bend.clock.osc_mhz"),
        (|p| clock_of(p).drift_ppm = -1.0, "bend.clock.drift_ppm"),
        (|p| clock_of(p).drift_hz = 0.0, "bend.clock.drift_hz"),
        (
            |p| clock_of(p).ppm_per_v = f64::INFINITY,
            "bend.clock.ppm_per_v",
        ),
        (|p| clock_of(p).hd_lock_px = -2.0, "bend.clock.hd_lock_px"),
        // A bridge names its partner, another pin; a stuck pin has none.
        (
            |p| faults_of(p)[1].with = None,
            "bend.memory.faults[1].with",
        ),
        (
            |p| faults_of(p)[1].with = Some(5),
            "bend.memory.faults[1].with",
        ),
        (
            |p| faults_of(p)[0].with = Some(9),
            "bend.memory.faults[0].with",
        ),
        // A marginal contact sets both of its values, each in range.
        (
            |p| faults_of(p)[1].on_frac = Some(0.0),
            "bend.memory.faults[1].on_frac",
        ),
        (
            |p| faults_of(p)[1].on_frac = Some(1.5),
            "bend.memory.faults[1].on_frac",
        ),
        (
            |p| faults_of(p)[1].switch_hz = Some(-1.0),
            "bend.memory.faults[1].switch_hz",
        ),
        (
            |p| faults_of(p)[1].switch_hz = None,
            "bend.memory.faults[1].on_frac",
        ),
        (
            |p| faults_of(p)[1].on_frac = None,
            "bend.memory.faults[1].switch_hz",
        ),
        // Each pin carries one fault, as `pin` or as a bridge's `with`.
        (
            |p| faults_of(p).push(solid(3, FaultKind::Low)),
            "bend.memory.faults[3].pin",
        ),
        (
            |p| faults_of(p).push(solid(6, FaultKind::High)),
            "bend.memory.faults[3].pin",
        ),
        (
            |p| faults_of(p)[1].with = Some(3),
            "bend.memory.faults[1].with",
        ),
    ];
    for (case, (breaks, field)) in cases.iter().enumerate() {
        let mut preset = milestone3_preset();
        breaks(&mut preset);
        let error = preset.validate().unwrap_err();
        assert!(error.is_config(), "case {case}: {error}");
        assert!(
            error.to_string().contains(field),
            "case {case}: \"{error}\" does not name {field}"
        );
    }
}

#[test]
fn memory_fault_pins_must_exist_on_the_camera_sdram() {
    let camera = camera();
    let bits = u8::try_from(camera.memory.address_bits()).unwrap();
    let timing = Timing::derive(&camera, &Preset::new("a520"), 64, 48).unwrap();
    let with_faults = |faults: Vec<AddressFault>| {
        let mut preset = Preset::new("a520");
        preset.bend.memory = Some(MemoryBend { faults });
        preset
    };

    let highest = with_faults(vec![solid(bits - 1, FaultKind::High)]);
    highest.validate().unwrap();
    let params = memory::Params::resolve(&camera, &highest, &timing)
        .unwrap()
        .expect("the preset has a memory bend");
    assert_eq!(params.address_bits, u32::from(bits));
    assert!(
        memory::Params::resolve(&camera, &Preset::new("a520"), &timing)
            .unwrap()
            .is_none()
    );

    let beyond = with_faults(vec![solid(bits, FaultKind::Low)]);
    beyond.validate().unwrap();
    let error = memory::Params::resolve(&camera, &beyond, &timing).unwrap_err();
    assert!(error.is_config(), "{error}");
    assert!(
        error.to_string().contains("bend.memory.faults[0].pin"),
        "{error}"
    );
    let error = Simulator::builder((camera.clone(), beyond))
        .build()
        .unwrap_err();
    assert!(
        error.to_string().contains("bend.memory.faults[0].pin"),
        "{error}"
    );

    let bridge = with_faults(vec![AddressFault {
        with: Some(bits + 2),
        ..solid(1, FaultKind::Bridge)
    }]);
    let error = memory::Params::resolve(&camera, &bridge, &timing).unwrap_err();
    assert!(
        error.to_string().contains("bend.memory.faults[0].with"),
        "{error}"
    );
}

/// Breaks one value of a camera.
type BreakCamera = fn(&mut Camera);

#[test]
fn camera_power_memory_and_nir_values_are_validated() {
    let cases: [(BreakCamera, &str); 19] = [
        (|c| c.power.rail_v = 0.0, "power.rail_v"),
        (|c| c.power.source_ohm = -0.5, "power.source_ohm"),
        (|c| c.power.decoupling_uf = f64::NAN, "power.decoupling_uf"),
        (|c| c.power.quiescent_ma = 0.0, "power.quiescent_ma"),
        (|c| c.power.afe_restart_lines = 0, "power.afe_restart_lines"),
        (|c| c.power.afe_uvlo_v = c.power.rail_v, "power.afe_uvlo_v"),
        (|c| c.power.logic_min_v = 4.0, "power.logic_min_v"),
        (|c| c.power.dsp_brownout_v = -1.0, "power.dsp_brownout_v"),
        (|c| c.memory.column_bits = 0, "memory.column_bits"),
        (|c| c.memory.row_bits = 17, "memory.row_bits"),
        (|c| c.memory.bank_bits = 0, "memory.bank_bits"),
        (
            |c| c.memory.raw_base = 1 << c.memory.address_bits(),
            "memory.raw_base",
        ),
        (|c| c.sensor.nir_response[0] = -0.1, "sensor.nir_response"),
        (
            |c| c.sensor.nir_response[2] = f64::INFINITY,
            "sensor.nir_response",
        ),
        (|c| c.lens.nir_defocus_frac = 0.5, "lens.nir_defocus_frac"),
        (|c| c.lens.nir_defocus_frac = -0.01, "lens.nir_defocus_frac"),
        (
            |c| c.lens.filter_edge_blur_frac = f64::NAN,
            "lens.filter_edge_blur_frac",
        ),
        (
            |c| c.lens.filter_edge_blur_frac = 0.75,
            "lens.filter_edge_blur_frac",
        ),
        (|c| c.lens.vignette_stops = -1.0, "lens.vignette_stops"),
    ];
    for (case, (breaks, field)) in cases.iter().enumerate() {
        let mut camera = camera();
        breaks(&mut camera);
        let error = camera.validate().unwrap_err();
        assert!(error.is_config(), "case {case}: {error}");
        assert!(
            error.to_string().contains(field),
            "case {case}: \"{error}\" does not name {field}"
        );
    }

    // The SDRAM's last word may hold the raw buffer's start.
    let mut camera = camera();
    camera.memory.raw_base = (1 << camera.memory.address_bits()) - 1;
    camera.validate().unwrap();
}

#[test]
fn the_power_and_memory_sections_and_the_nir_keys_are_required() {
    let text = std::fs::read_to_string(root().join("cameras/a520.toml")).unwrap();
    let file: toml::Table = toml::from_str(&text).unwrap();
    for section in ["power", "memory"] {
        let mut without = file.clone();
        without.remove(section);
        let error = toml::from_str::<Camera>(&toml::to_string(&without).unwrap()).unwrap_err();
        assert!(error.to_string().contains(section), "{section}: {error}");
    }
    for (section, key) in [
        ("sensor", "nir_response"),
        ("lens", "nir_defocus_frac"),
        ("lens", "filter_edge_blur_frac"),
    ] {
        let mut without = file.clone();
        without
            .get_mut(section)
            .and_then(toml::Value::as_table_mut)
            .unwrap()
            .remove(key);
        let error = toml::from_str::<Camera>(&toml::to_string(&without).unwrap()).unwrap_err();
        assert!(error.to_string().contains(key), "{key}: {error}");
    }
}
