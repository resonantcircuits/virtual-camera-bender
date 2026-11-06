//! The scene, sensor, clock, AFE and processor panels write presets that survive TOML,
//! keep only what the user touched, and with the bus panel reproduce every shipped preset.

use std::fs;
use std::path::Path;

use vcb_app::panels::afe::{self, AfePanel};
use vcb_app::panels::clocks::{self, ClocksPanel};
use vcb_app::panels::isp::{self, Field};
use vcb_app::panels::strip::Stage;
use vcb_app::panels::{PanelId, Panels, Scale, sensor};
use vcb_app::presets;
use vcb_core::afe::{GainMod, Inject, Wave};
use vcb_core::isp::{AeMode, WbMode};
use vcb_core::preset::{
    ClockBend, IspOverride, MemoryBend, Mode, RailBend, ReclockBend, Resolution,
};
use vcb_core::{Camera, Preset, Timing};

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn camera() -> Camera {
    presets::load_camera(&root().join("cameras"), "a520").expect("the A520 camera file")
}

/// Every shipped preset by name.
fn shipped() -> Vec<(String, Preset)> {
    let entries = presets::discover(&root().join("presets")).expect("the presets folder");
    assert!(
        entries.len() >= 4,
        "the shipped presets are found: {entries:?}"
    );
    entries
        .into_iter()
        .map(|entry| {
            let text = fs::read_to_string(&entry.path).expect("readable");
            let preset = presets::from_toml(&text).expect("the shipped preset is valid");
            (entry.name, preset)
        })
        .collect()
}

/// Writes `preset` as TOML and reads it back, checking that nothing changed.
fn through_toml(preset: &Preset) -> Preset {
    let text = presets::to_toml(preset).expect("the preset serialises");
    let parsed = presets::from_toml(&text).expect("the written preset parses and validates");
    assert_eq!(&parsed, preset, "the preset survives TOML:\n{text}");
    parsed
}

/// A preset that uses every panel.
fn everything() -> (Preset, Panels) {
    let mut preset = presets::load_preset(&root().join("presets/a520-div.toml")).expect("shipped");
    preset.scene.headroom = 4.0;
    preset.scene.shoulder_start = 0.65;
    let mut panels = Panels::from_preset(&preset);
    panels.clocks = ClocksPanel {
        enabled: true,
        bend: ClockBend {
            cte: Some(0.9993),
            v_pulse_drop_hz: 3.5,
            v_stall_lines: 12.0,
            h_glitch_hz: 0.4,
            h_shift_frac: 0.35,
        },
    };
    panels.afe = AfePanel {
        enabled: true,
        inject_on: true,
        inject: Inject {
            wave: Wave::Saw,
            freq_hz: 4400.0,
            amplitude_v: 0.03,
            phase_skew: 0.125,
        },
        gain_on: true,
        gain_mod: GainMod {
            wave: Wave::Noise,
            freq_hz: 25.0,
            depth: 0.3,
        },
        cds_lag_px: 2.5,
        cds_mix: 0.4,
    };
    panels.apply(&mut preset);
    isp::set(&mut preset.isp, Field::ToneContrast, 1.4);
    isp::set(&mut preset.isp, Field::SharpenRadius, 0.001);
    isp::set_wb(&mut preset.isp, Some(WbMode::Servo));
    isp::set_ae(&mut preset.isp, Some(AeMode::Scene));
    sensor::set_exposure(&mut preset.sensor, Some(0.004));
    (preset, panels)
}

#[test]
fn every_shipped_preset_survives_the_panels() {
    for (name, preset) in shipped() {
        let mut rebuilt = preset.clone();
        Panels::from_preset(&preset).apply(&mut rebuilt);
        assert_eq!(rebuilt, preset, "{name} survives the bend panels");

        let mut clocks = preset.clone();
        ClocksPanel::from_preset(&preset).apply(&mut clocks);
        assert_eq!(clocks, preset, "{name} survives the clock panel");
        let mut afe_only = preset.clone();
        AfePanel::from_preset(&preset).apply(&mut afe_only);
        assert_eq!(afe_only, preset, "{name} survives the AFE panel");
        through_toml(&rebuilt);
    }
}

#[test]
fn a_preset_using_every_panel_round_trips_through_toml() {
    let (preset, panels) = everything();
    let parsed = through_toml(&preset);
    assert_eq!(
        Panels::from_preset(&parsed),
        panels,
        "the panels read back their own preset"
    );
    let mut rebuilt = parsed.clone();
    Panels::from_preset(&parsed).apply(&mut rebuilt);
    assert_eq!(rebuilt, preset);
    assert!(parsed.bend.clocks.is_some() && parsed.bend.afe.is_some());
    assert!(
        parsed.bend.bus.is_some(),
        "the shipped bus bend stays patched in"
    );
}

#[test]
fn a_switched_off_bend_leaves_the_preset_but_keeps_its_values() {
    let (preset, mut panels) = everything();
    panels.clocks.enabled = false;
    panels.afe.enabled = false;
    let mut off = preset.clone();
    panels.apply(&mut off);
    assert!(off.bend.clocks.is_none() && off.bend.afe.is_none());
    through_toml(&off);

    let mut reloaded = panels.clone();
    reloaded.load(&off);
    assert!(!reloaded.clocks.enabled && !reloaded.afe.enabled);
    assert_eq!(
        reloaded.clocks.bend, panels.clocks.bend,
        "the clock values wait for the bend to be switched on again"
    );
    assert_eq!(reloaded.afe.inject, panels.afe.inject);
}

#[test]
fn the_afe_parts_switch_on_and_off_on_their_own() {
    let mut panel = AfePanel {
        enabled: true,
        inject_on: false,
        gain_on: true,
        ..AfePanel::default()
    };
    let bend = panel.bend().expect("the bend is on");
    assert!(
        bend.inject.is_none(),
        "a disconnected oscillator is not written"
    );
    assert!(bend.gain_mod.is_some());
    let mut preset = Preset::new("a520");
    panel.apply(&mut preset);
    let parsed = through_toml(&preset);

    panel.inject.freq_hz = 50.0;
    panel.load(&parsed);
    assert!(!panel.inject_on);
    assert!(
        (panel.inject.freq_hz - 50.0).abs() < f64::EPSILON,
        "loading a preset without injection keeps the oscillator's dial"
    );

    let plain = AfePanel {
        enabled: true,
        inject_on: false,
        gain_on: false,
        ..AfePanel::default()
    };
    let mut preset = Preset::new("a520");
    plain.apply(&mut preset);
    assert_eq!(
        AfePanel::from_preset(&through_toml(&preset)).bend(),
        plain.bend(),
        "an AFE bend with only the CDS settings round-trips"
    );
}

#[test]
fn the_transfer_loss_shows_in_ppm_and_is_stored_as_cte() {
    let loss = Scale::LossPpm;
    assert!(
        (loss.show(0.99999) - 10.0).abs() < 1e-6,
        "the A520 loses 10 ppm"
    );
    assert!((loss.store(1000.0) - 0.999).abs() < 1e-12);
    assert!((loss.store(loss.show(0.9995)) - 0.9995).abs() < 1e-12);
    let trail = clocks::trail_lines(0.999, 1704);
    assert!(
        (trail - 1.7).abs() < 0.01,
        "1000 ppm over 1704 transfers trails about 1.7 lines, got {trail}"
    );
    let percent = Scale::Times(100.0);
    assert!((percent.store(percent.show(0.35)) - 0.35).abs() < 1e-12);
}

#[test]
fn the_oscillator_frequency_shows_its_length_in_lines() {
    let camera = camera();
    let timing = Timing::derive(&camera, &Preset::new("a520"), 2272, 1704).expect("native still");
    let lines = timing.lines_per_cycle(1000.0);
    assert!(
        (7.0..8.5).contains(&lines),
        "1 kHz at a 132 µs line spans about 7.6 lines, got {lines}"
    );
    let shown = afe::cycle_length(1000.0, &timing);
    assert!(shown.ends_with("lines per cycle"), "{shown}");
    assert!(shown.starts_with("7."), "{shown}");
    let fast = afe::cycle_length(1e6, &timing);
    assert!(
        fast.ends_with("px per cycle"),
        "below a line the length is in pixels: {fast}"
    );
}

#[test]
fn processor_overrides_hold_only_what_was_touched() {
    let camera = camera();
    let mut preset = Preset::new("a520");
    assert_eq!(
        isp::value(preset.isp.as_ref(), &camera, Field::ToneContrast),
        (camera.isp.tone_contrast, false),
        "an untouched setting shows the camera's value"
    );
    isp::set(&mut preset.isp, Field::ToneContrast, 1.5);
    assert_eq!(
        preset.isp,
        Some(IspOverride {
            tone_contrast: Some(1.5),
            ..IspOverride::default()
        })
    );
    assert_eq!(
        isp::value(preset.isp.as_ref(), &camera, Field::ToneContrast),
        (1.5, true)
    );
    for field in Field::ALL {
        if field != Field::ToneContrast {
            assert_eq!(
                isp::value(preset.isp.as_ref(), &camera, field),
                (field.camera(&camera), false),
                "{field:?} stays the camera's"
            );
        }
    }
    let text = presets::to_toml(&preset).expect("serialises");
    assert!(text.contains("tone_contrast"), "{text}");
    assert!(
        !text.contains("tone_toe"),
        "untouched values are not written:\n{text}"
    );
    through_toml(&preset);

    isp::reset(&mut preset.isp, Field::ToneContrast);
    assert!(
        preset.isp.is_none(),
        "resetting the last override removes [isp]"
    );

    isp::set_wb(&mut preset.isp, Some(WbMode::Auto));
    isp::set_ae(&mut preset.isp, Some(AeMode::Servo));
    through_toml(&preset);
    isp::set_wb(&mut preset.isp, None);
    assert!(preset.isp.is_some(), "the auto-exposure override remains");
    isp::set_ae(&mut preset.isp, None);
    assert!(preset.isp.is_none());
}

#[test]
fn a_hand_written_colour_matrix_survives_the_processor_panel() {
    let camera = camera();
    let mut preset = Preset::new("a520");
    preset.isp = Some(IspOverride {
        xyz_to_cam: Some(camera.isp.xyz_to_cam),
        ..IspOverride::default()
    });
    isp::set(&mut preset.isp, Field::SharpenGain, 0.8);
    isp::reset(&mut preset.isp, Field::SharpenGain);
    assert_eq!(
        preset.isp.as_ref().and_then(|isp| isp.xyz_to_cam),
        Some(camera.isp.xyz_to_cam),
        "values the panel does not show are kept"
    );
    through_toml(&preset);
}

#[test]
fn every_processor_knob_spans_its_camera_value() {
    let camera = camera();
    let width = f64::from(camera.sensor.native_width);
    for field in Field::ALL {
        let knob = field.knob(width);
        let shown = knob.scale.show(field.camera(&camera));
        assert!(
            knob.range.contains(&shown),
            "{field:?}: the camera's {shown} lies outside {:?}",
            knob.range
        );
    }
}

#[test]
fn the_scene_panel_values_survive_toml() {
    let mut preset = Preset::new("a520");
    preset.scene.headroom = 6.5;
    preset.scene.shoulder_start = 0.55;
    through_toml(&preset);
    let shown = Scale::Times(100.0).show(preset.scene.shoulder_start);
    assert!(
        (shown - 55.0).abs() < 1e-9,
        "the shoulder shows in percent of white"
    );
}

#[test]
fn the_exposure_is_a_preset_override_that_survives_toml() {
    let camera = camera();
    let mut preset = Preset::new("a520");
    let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
    assert!(
        close(
            sensor::integration_s(&camera, &preset, Mode::Still),
            camera.still.exposure_s
        ),
        "untouched, the camera's still exposure applies"
    );
    assert!(
        close(
            sensor::integration_s(&camera, &preset, Mode::Movie),
            camera.movie.exposure_s.min(1.0 / camera.movie.fps)
        ),
        "movie mode integrates the camera's movie exposure"
    );

    sensor::set_exposure(&mut preset.sensor, Some(0.25));
    assert_eq!(sensor::exposure_override(&preset), Some(0.25));
    let text = presets::to_toml(&preset).expect("serialises");
    assert!(
        text.contains("exposure_s"),
        "the exposure is saved:\n{text}"
    );
    through_toml(&preset);
    assert!(close(
        sensor::integration_s(&camera, &preset, Mode::Still),
        0.25
    ));
    let movie = sensor::integration_s(&camera, &preset, Mode::Movie);
    assert!(
        (movie - 1.0 / camera.movie.fps).abs() < 1e-12,
        "the electronic shutter integrates at most one frame, got {movie}"
    );

    sensor::set_exposure(&mut preset.sensor, None);
    assert!(
        preset.sensor.is_none(),
        "resetting the exposure removes [sensor]"
    );
    let knob = sensor::EXPOSURE;
    for exposure_s in [camera.still.exposure_s, camera.movie.exposure_s] {
        assert!(knob.range.contains(&knob.scale.show(exposure_s)));
    }
}

#[test]
fn a_longer_exposure_brightens_the_running_camera_without_a_rebuild() {
    let mut preset = Preset::new("a520");
    preset.readout.resolution = Resolution::MatchInput;
    let scene = vcb_core::Scene::from_srgb8(64, 48, &[60; 64 * 48 * 3]);
    let mut simulator = vcb_core::Simulator::builder((camera(), preset.clone()))
        .input_size(64, 48)
        .build()
        .expect("builds");
    let mean = |frame: &vcb_core::Frame| {
        frame
            .rgb()
            .iter()
            .map(|&value| f64::from(value))
            .sum::<f64>()
            / frame.rgb().len() as f64
    };
    let short = mean(simulator.frame(&scene, 0));
    sensor::set_exposure(&mut preset.sensor, Some(0.032));
    let reconfigured = simulator.set_preset(preset).expect("valid");
    assert!(reconfigured.state_kept(), "{reconfigured}");
    let long = mean(simulator.frame(&scene, 1));
    assert!(
        long > short + 20.0,
        "two stops more light brighten a mid-dark frame: {short:.1} -> {long:.1}"
    );
}

#[test]
fn the_strip_follows_the_signal_and_marks_the_bends() {
    let names: Vec<&str> = Stage::ALL.iter().map(|stage| stage.name()).collect();
    assert_eq!(
        names,
        [
            "Scene",
            "Sensor",
            "Readout",
            "AFE",
            "ADC",
            "Bus",
            "Clock",
            "Processor",
            "Memory",
            "JPEG"
        ]
    );
    assert_eq!(
        Stage::Adc.panel(),
        PanelId::Bus,
        "the ADC leads to the bus on its lines"
    );
    assert_eq!(Stage::Clock.panel(), PanelId::Clock);
    assert_eq!(Stage::Memory.panel(), PanelId::Memory);
    assert_eq!(Stage::Jpeg.panel(), PanelId::Jpeg);

    let clean = Preset::new("a520");
    assert!(Stage::ALL.iter().all(|stage| !stage.is_bent(&clean)));
    let (bent, _) = everything();
    let marked: Vec<Stage> = Stage::ALL
        .into_iter()
        .filter(|stage| stage.is_bent(&bent))
        .collect();
    assert_eq!(
        marked,
        [Stage::Readout, Stage::Afe, Stage::Bus, Stage::Processor]
    );

    let bent_by = |bend: fn(&mut Preset)| {
        let mut preset = Preset::new("a520");
        bend(&mut preset);
        Stage::ALL
            .into_iter()
            .filter(|stage| stage.is_bent(&preset))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        bent_by(|p| p.bend.clock = Some(ReclockBend::new(26.4))),
        [Stage::Clock]
    );
    assert_eq!(
        bent_by(|p| p.bend.memory = Some(MemoryBend::default())),
        [Stage::Memory]
    );
    assert_eq!(
        bent_by(|p| p.bend.rail = Some(RailBend::default())),
        [Stage::Afe, Stage::Bus, Stage::Processor],
        "the rail lights the stages it feeds"
    );
}

#[test]
fn panel_presets_render_without_errors() {
    let (mut preset, _) = everything();
    preset.readout.resolution = Resolution::MatchInput;
    let scene = vcb_core::Scene::from_srgb8(96, 72, &vec![120; 96 * 72 * 3]);
    let mut simulator = vcb_core::Simulator::builder((camera(), preset))
        .input_size(96, 72)
        .build()
        .expect("every panel writes values the simulator accepts");
    let frame = simulator.frame(&scene, 0);
    assert_eq!((frame.width(), frame.height()), (96, 72));
}
