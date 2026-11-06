//! The sensor bend and reset gate blocks write full sections that survive TOML, keep
//! their values while switched off, show their readouts from the camera's values, light
//! their stages on the strip, and reach the running camera as kept state.

use std::path::Path;
use std::sync::Arc;

use vcb_app::panels::afe::{self, ResetPanel};
use vcb_app::panels::sensor::{self, SensorBendPanel};
use vcb_app::panels::strip::Stage;
use vcb_app::panels::{PanelId, Panels};
use vcb_app::presets;
use vcb_app::render::{Job, Renderer};
use vcb_core::preset::{AfeBend, RailBend, ResetBend, Resolution, SensorBend};
use vcb_core::{Camera, Preset, Reconfigured, Scene, Simulator};

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn camera() -> Camera {
    presets::load_camera(&root().join("cameras"), "a520").expect("the A520 camera file")
}

/// Writes `preset` as TOML and reads it back, checking that nothing changed.
fn through_toml(preset: &Preset) -> Preset {
    let text = presets::to_toml(preset).expect("the preset serialises");
    let parsed = presets::from_toml(&text).expect("the written preset parses and validates");
    assert_eq!(&parsed, preset, "the preset survives TOML:\n{text}");
    parsed
}

/// A preset with `apply` written into it, through TOML.
fn written(apply: impl FnOnce(&mut Preset)) -> Preset {
    let mut preset = Preset::new("a520");
    preset.seed = 5;
    apply(&mut preset);
    through_toml(&preset)
}

/// A heated sensor with a raised substrate bias.
fn sensor_panel() -> SensorBendPanel {
    SensorBendPanel {
        enabled: true,
        bend: SensorBend {
            temperature_c: Some(55.0),
            substrate_v: Some(13.25),
        },
    }
}

/// A reset gate with missed runs and weak pulses.
fn reset_panel() -> ResetPanel {
    ResetPanel {
        enabled: true,
        bend: ResetBend {
            drop_hz: 3500.0,
            hold_px: 40.0,
            reset_frac: 0.35,
        },
    }
}

/// Whether `a` and `b` agree to a part in a million.
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn each_block_round_trips_a_full_section_through_toml() {
    let panel = sensor_panel();
    let preset = written(|preset| panel.apply(preset));
    assert_eq!(preset.bend.sensor, Some(panel.bend));
    assert_eq!(SensorBendPanel::from_preset(&preset), panel);

    let camera_bias = SensorBendPanel {
        bend: SensorBend {
            substrate_v: None,
            ..panel.bend
        },
        ..panel
    };
    let preset = written(|preset| camera_bias.apply(preset));
    let text = presets::to_toml(&preset).expect("serialises");
    assert!(
        text.contains("temperature_c") && !text.contains("substrate_v"),
        "the camera's bias is not written:\n{text}"
    );
    assert_eq!(SensorBendPanel::from_preset(&preset), camera_bias);

    let untouched = SensorBendPanel {
        enabled: true,
        bend: SensorBend::default(),
    };
    let preset = written(|preset| untouched.apply(preset));
    assert_eq!(
        preset.bend.sensor,
        Some(SensorBend::default()),
        "a bent sensor at the camera's values writes an empty section"
    );
    assert_eq!(SensorBendPanel::from_preset(&preset), untouched);

    let panel = reset_panel();
    let preset = written(|preset| panel.apply(preset));
    assert_eq!(preset.bend.reset, Some(panel.bend));
    assert_eq!(ResetPanel::from_preset(&preset), panel);

    let fresh = ResetPanel {
        enabled: true,
        ..ResetPanel::default()
    };
    let preset = written(|preset| fresh.apply(preset));
    let bend = preset.bend.reset.expect("the reset bend");
    assert!(
        bend.drop_hz > 0.0 && close(bend.reset_frac, 1.0),
        "a fresh reset gate misses pulses, each pulse that arrives a full reset: {bend:?}"
    );
    assert_eq!(ResetPanel::from_preset(&preset), fresh);
    let fresh = SensorBendPanel {
        enabled: true,
        ..SensorBendPanel::default()
    };
    let bend = fresh.bend().expect("on");
    assert!(
        bend.temperature_c
            .is_some_and(|t| t > camera().sensor.reference_temp_c)
            && bend.substrate_v.is_none(),
        "a fresh sensor bend is a heat gun at the camera's bias: {bend:?}"
    );
}

#[test]
fn a_switched_off_block_removes_its_section_and_restores_the_same_values() {
    fn check<P: Clone + PartialEq + std::fmt::Debug>(
        name: &str,
        mut panel: P,
        enabled: fn(&mut P) -> &mut bool,
        apply: fn(&P, &mut Preset),
        load: fn(&mut P, &Preset),
        section: fn(&Preset) -> bool,
    ) {
        let on = written(|preset| apply(&panel, preset));
        assert!(section(&on), "{name}: on writes the section");
        *enabled(&mut panel) = false;
        let off = written(|preset| apply(&panel, preset));
        assert!(!section(&off), "{name}: off removes the section");
        assert_eq!(
            off,
            Preset {
                seed: 5,
                ..Preset::new("a520")
            }
        );

        let before = panel.clone();
        load(&mut panel, &off);
        assert_eq!(
            panel, before,
            "{name}: loading a preset without the bend keeps the values"
        );
        *enabled(&mut panel) = true;
        let again = written(|preset| apply(&panel, preset));
        assert_eq!(again, on, "{name}: on again restores the same section");
    }
    check(
        "sensor bend",
        sensor_panel(),
        |panel| &mut panel.enabled,
        SensorBendPanel::apply,
        SensorBendPanel::load,
        |preset| preset.bend.sensor.is_some(),
    );
    check(
        "reset gate",
        reset_panel(),
        |panel| &mut panel.enabled,
        ResetPanel::apply,
        ResetPanel::load,
        |preset| preset.bend.reset.is_some(),
    );

    let panels = Panels {
        sensor: sensor_panel(),
        reset: reset_panel(),
        ..Panels::default()
    };
    let mut preset = Preset::new("a520");
    panels.apply(&mut preset);
    assert!(
        preset.bend.sensor.is_some() && preset.bend.reset.is_some(),
        "the panels write both sections"
    );
    assert!(
        preset.bend.afe.is_none(),
        "the reset gate's switch is its own, apart from the AFE bend's"
    );
    assert_eq!(Panels::from_preset(&through_toml(&preset)), panels);
}

#[test]
fn the_sensor_readout_shows_the_dark_multiple_and_the_well_from_the_camera() {
    let camera = camera();
    let s = &camera.sensor;
    let stock = SensorBend::default();
    assert!(close(sensor::dark_multiple(&camera, &stock), 1.0));
    let (well_e, drain) = sensor::well_at_bias(&camera, &stock).expect("the camera's values");
    assert!(
        close(well_e, s.full_well_e) && close(drain, 1.0),
        "at the camera's values the well is the camera's: {well_e} {drain}"
    );
    assert_eq!(
        sensor::bend_readout(&camera, &stock),
        format!(
            "Dark current 1.00 × that at {} °C; full well {:.0} e.",
            s.reference_temp_c, s.full_well_e
        )
    );

    let one_doubling = SensorBend {
        temperature_c: Some(s.reference_temp_c + s.dark_doubling_c),
        substrate_v: None,
    };
    assert!(
        close(sensor::dark_multiple(&camera, &one_doubling), 2.0),
        "one doubling step above the reference doubles the dark current"
    );
    let cold = SensorBend {
        temperature_c: Some(s.reference_temp_c - 3.0 * s.dark_doubling_c),
        substrate_v: None,
    };
    assert!(close(sensor::dark_multiple(&camera, &cold), 0.125));

    let heat_gun = SensorBend {
        temperature_c: Some(70.0),
        substrate_v: None,
    };
    let k = ((70.0 - s.reference_temp_c) / s.dark_doubling_c).exp2();
    let text = sensor::bend_readout(&camera, &heat_gun);
    assert!(
        text.starts_with(&format!(
            "Dark current {} × that",
            vcb_app::panels::significant(k, 3)
        )),
        "{text}"
    );
    assert_eq!(
        text, "Dark current 181 × that at 25 °C; full well 17800 e.",
        "the A520 at 70 °C"
    );

    let raised = SensorBend {
        temperature_c: None,
        substrate_v: Some(s.substrate_v + 2.0),
    };
    let (well_e, drain) = sensor::well_at_bias(&camera, &raised).expect("in range");
    assert!(
        close(
            well_e,
            s.full_well_e * 2.0f64.mul_add(-s.full_well_frac_per_v, 1.0)
        ) && close(drain, 1.0),
        "a raised bias shrinks the well and keeps the drain: {well_e} {drain}"
    );
    assert_eq!(
        sensor::bend_readout(&camera, &raised),
        "Dark current 1.00 × that at 25 °C; full well 8900 e.",
        "the A520's well halves two volts above nominal"
    );

    let lowered = SensorBend {
        temperature_c: None,
        substrate_v: Some(s.substrate_v - s.drain_loss_v / 2.0),
    };
    let (well_e, drain) = sensor::well_at_bias(&camera, &lowered).expect("in range");
    assert!(
        well_e > s.full_well_e && close(drain, 0.5),
        "a lowered bias deepens the well and halves the drain halfway to its loss: {well_e} {drain}"
    );
    assert_eq!(
        sensor::bend_readout(&camera, &lowered),
        "Dark current 1.00 × that at 25 °C; full well 22250 e, overflow drain at 50 %."
    );
    let gone = SensorBend {
        temperature_c: None,
        substrate_v: Some(s.substrate_v - s.drain_loss_v),
    };
    assert!(
        sensor::bend_readout(&camera, &gone).ends_with("overflow drain at 0 %."),
        "{}",
        sensor::bend_readout(&camera, &gone)
    );
}

#[test]
fn the_reset_readout_shows_the_swing_from_the_camera() {
    let camera = camera();
    let swing = camera.afe.fd_swing_v / (camera.sensor.conv_gain_uv_per_e * 1e-6);
    assert!(close(afe::swing_e(&camera), swing));
    assert!(
        swing > camera.sensor.full_well_e,
        "the A520's swing holds a full well with room to spare: {swing}"
    );
    let sound = ResetBend::default();
    assert_eq!(
        afe::reset_readout(&camera, &sound),
        "Swing 23077 e; a full reset leaves no charge behind."
    );
    let weak = ResetBend {
        reset_frac: 0.2,
        ..sound
    };
    assert_eq!(
        afe::reset_readout(&camera, &weak),
        format!(
            "Swing 23077 e; a weak reset shows on packets above {:.0} e.",
            0.2 * swing
        )
    );
    assert_eq!(
        afe::reset_readout(&camera, &weak),
        "Swing 23077 e; a weak reset shows on packets above 4615 e."
    );
}

#[test]
fn the_knobs_show_their_units_and_span_the_defaults() {
    let camera = camera();
    let s = &camera.sensor;
    assert_eq!(sensor::TEMPERATURE.format(70.0), "70 °C");
    assert_eq!(sensor::TEMPERATURE.parse("-5 °C"), Some(-5.0));
    assert_eq!(sensor::TEMPERATURE.parse("−5"), Some(-5.0));
    let bias = sensor::bias_knob(&camera);
    assert_eq!(bias.format(13.5), "13.50 V");
    assert!(
        close(*bias.range.start(), s.substrate_v - sensor::BIAS_SPAN_V)
            && close(*bias.range.end(), s.substrate_v + sensor::BIAS_SPAN_V),
        "the bias turns 4 V either way of the camera's: {:?}",
        bias.range
    );
    assert_eq!(afe::DROPS.format(2000.0), "2000 Hz");
    assert_eq!(afe::RUN.format(8.0), "8.00 px");
    assert_eq!(afe::STRENGTH.format(afe::STRENGTH.scale.show(0.2)), "20 %");

    let reset = ResetBend::default();
    for (name, knob, value) in [
        ("temperature", &sensor::TEMPERATURE, s.reference_temp_c),
        (
            "heat gun",
            &sensor::TEMPERATURE,
            sensor::DEFAULT_TEMPERATURE_C,
        ),
        ("bias", &bias, s.substrate_v),
        (
            "bias at the drain's loss",
            &bias,
            s.substrate_v - s.drain_loss_v,
        ),
        ("missed pulses", &afe::DROPS, afe::DEFAULT_DROP_HZ),
        ("no missed pulses", &afe::DROPS, reset.drop_hz),
        ("run length", &afe::RUN, reset.hold_px),
        ("sound reset", &afe::STRENGTH, reset.reset_frac),
    ] {
        let shown = knob.scale.show(value);
        assert!(
            knob.range.contains(&shown),
            "{name}: the default {shown} lies outside {:?}",
            knob.range
        );
        assert!(!knob.unit.trim().is_empty(), "{name} has a unit");
    }

    let mut preset = Preset::new("a520");
    preset.readout.resolution = Resolution::MatchInput;
    for (temperature_c, substrate_v, drop_hz, hold_px, reset_frac) in [
        (-20.0, *bias.range.start(), 100_000.0, 2000.0, 0.0),
        (120.0, *bias.range.end(), 0.1, 1.0, 1.0),
    ] {
        preset.bend.sensor = Some(SensorBend {
            temperature_c: Some(temperature_c),
            substrate_v: Some(substrate_v),
        });
        preset.bend.reset = Some(ResetBend {
            drop_hz,
            hold_px,
            reset_frac,
        });
        let preset = through_toml(&preset);
        let scene = Scene::from_srgb8(48, 32, &[140; 48 * 32 * 3]);
        let mut simulator = Simulator::builder((camera.clone(), preset))
            .input_size(48, 32)
            .build()
            .expect("the core accepts the knobs' ends");
        let frame = simulator.frame(&scene, 0);
        assert_eq!((frame.width(), frame.height()), (48, 32));
    }
}

#[test]
fn the_strip_lights_the_sensor_and_the_afe_for_the_new_bends() {
    let with = |bend: fn(&mut Preset)| {
        let mut preset = Preset::new("a520");
        bend(&mut preset);
        preset
    };
    let lit = |preset: &Preset| {
        Stage::ALL
            .into_iter()
            .filter(|stage| stage.is_bent(preset))
            .collect::<Vec<_>>()
    };
    let heated = with(|p| p.bend.sensor = Some(SensorBend::default()));
    assert_eq!(lit(&heated), [Stage::Sensor]);
    assert_eq!(Stage::Sensor.panel(&heated), PanelId::Sensor);
    assert!(
        Stage::Sensor.hover(&heated).contains("bent"),
        "{}",
        Stage::Sensor.hover(&heated)
    );

    let reset = with(|p| p.bend.reset = Some(ResetBend::default()));
    assert_eq!(lit(&reset), [Stage::Afe], "the reset gate lights the AFE");
    assert_eq!(Stage::Afe.panel(&reset), PanelId::Afe);

    let both = with(|p| {
        p.bend.reset = Some(ResetBend::default());
        p.bend.afe = Some(AfeBend::default());
    });
    assert_eq!(
        lit(&both),
        [Stage::Afe],
        "the AFE bend and the reset gate light the one stage"
    );
    let railed = with(|p| {
        p.bend.reset = Some(ResetBend::default());
        p.bend.rail = Some(RailBend::default());
    });
    assert_eq!(
        Stage::Afe.panel(&railed),
        PanelId::Afe,
        "the reset gate, the AFE's own, comes before the rail"
    );
    assert_eq!(Stage::Bus.panel(&railed), PanelId::Power);

    let clean = Preset::new("a520");
    assert!(!Stage::Sensor.is_bent(&clean) && !Stage::Afe.is_bent(&clean));
    let mut exposed = clean.clone();
    sensor::set_exposure(&mut exposed.sensor, Some(0.5));
    assert!(
        !Stage::Sensor.is_bent(&exposed),
        "an exposure time is a setting, not a bend"
    );
}

/// A knob's name and the change of the preset it makes.
type Turn = (&'static str, fn(&mut Preset));

/// A small running job at match-input resolution with both new bends on.
fn bent_job() -> Job {
    let mut preset = Preset::new("a520");
    preset.seed = 3;
    preset.readout.resolution = Resolution::MatchInput;
    sensor_panel().apply(&mut preset);
    reset_panel().apply(&mut preset);
    let (width, height) = (128, 96);
    let rgb: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            let x = (i % width * 255 / width) as u8;
            [x, 200, 255 - x]
        })
        .collect();
    Job {
        generation: 1,
        scene: Arc::new(Scene::from_srgb8(width, height, &rgb)),
        camera: Arc::new(camera()),
        preset,
        preview: false,
        power_cycle: 0,
    }
}

#[test]
fn turning_the_new_knobs_keeps_the_camera_running() {
    let mut renderer = Renderer::default();
    let mut job = bent_job();
    let first = renderer
        .render(&job)
        .outcome
        .expect("the bent camera renders");
    assert_eq!(first.reconfigured, Reconfigured::Rebuilt, "a new simulator");
    let kept = Reconfigured::Kept {
        bus_reset: false,
        servo_reset: false,
    };
    let turns: [Turn; 9] = [
        ("the temperature", |p| {
            if let Some(sensor) = p.bend.sensor.as_mut() {
                sensor.temperature_c = Some(90.0);
            }
        }),
        ("the substrate bias", |p| {
            if let Some(sensor) = p.bend.sensor.as_mut() {
                sensor.substrate_v = Some(10.5);
            }
        }),
        ("the temperature's reset", |p| {
            if let Some(sensor) = p.bend.sensor.as_mut() {
                sensor.temperature_c = None;
            }
        }),
        ("the missed pulses", |p| {
            if let Some(reset) = p.bend.reset.as_mut() {
                reset.drop_hz = 20_000.0;
            }
        }),
        ("the run length", |p| {
            if let Some(reset) = p.bend.reset.as_mut() {
                reset.hold_px = 300.0;
            }
        }),
        ("the reset strength", |p| {
            if let Some(reset) = p.bend.reset.as_mut() {
                reset.reset_frac = 0.8;
            }
        }),
        ("the sensor bend's switch", |p| p.bend.sensor = None),
        ("the reset gate's switch", |p| p.bend.reset = None),
        ("the reset gate's switch, on again", |p| {
            p.bend.reset = reset_panel().bend();
        }),
    ];
    for (index, (name, turn)) in turns.into_iter().enumerate() {
        turn(&mut job.preset);
        job.generation += 1;
        let output = renderer.render(&job).outcome.expect("renders");
        assert_eq!(
            output.reconfigured, kept,
            "turning {name} keeps every buffer and state, so the status line reports no circuit reset"
        );
        assert_eq!(output.index, index as u64 + 1, "{name}: the next frame");
    }
}
