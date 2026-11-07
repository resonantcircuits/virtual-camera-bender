//! The optics, master clock, memory and power panels write full sections that survive
//! TOML, keep their values while switched off, refuse what the preset cannot hold, show
//! their readouts in physical units, and reach the running camera as kept state.

use std::path::Path;
use std::sync::Arc;

use vcb_app::panels::memory::{self, MemoryPanel, PinGrid, PinSetting};
use vcb_app::panels::optics::{self, OpticsPanel};
use vcb_app::panels::rail::{self, RailPanel, Sag};
use vcb_app::panels::reclock::{self, ReclockPanel};
use vcb_app::presets;
use vcb_app::render::{Job, Renderer};
use vcb_core::preset::{
    AddressFault, FaultKind, MemoryBend, OpticsBend, RailBend, ReclockBend, Resolution,
};
use vcb_core::{Camera, Preset, Reconfigured, Scene, Timing};

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn camera() -> Camera {
    presets::load_camera(&root().join("cameras"), "a520").expect("the A520 camera file")
}

/// The A520's bendable address pins, `a0` to `a11`.
const PINS: u8 = 12;

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

fn optics_panel() -> OpticsPanel {
    OpticsPanel {
        enabled: true,
        bend: OpticsBend {
            ir_cut_pulled_frac: 0.45,
            nir_weights: [0.5, 0.3, 0.2],
            foliage_nir: 3.5,
            nir_scale: 1.4,
        },
    }
}

fn reclock_panel() -> ReclockPanel {
    ReclockPanel {
        enabled: true,
        bend: ReclockBend {
            osc_mhz: 26.7,
            drift_ppm: 300.0,
            drift_hz: 0.8,
            ppm_per_v: -250.0,
            hd_lock_px: 6.0,
        },
    }
}

fn rail_panel() -> RailPanel {
    RailPanel {
        enabled: true,
        bend: RailBend {
            load_ma: 150.0,
            spike_ma: 400.0,
            spike_hz: 3.0,
            spike_ms: 1.5,
            ripple_mv: 20.0,
            ripple_hz: 120.0,
            source_ohm: Some(2.0),
        },
    }
}

/// A memory panel with every kind of fault, set through the grid's operations.
fn memory_panel() -> MemoryPanel {
    let mut panel = MemoryPanel {
        enabled: true,
        address: PinGrid::default(),
        ..MemoryPanel::default()
    };
    assert!(panel.address.set_kind(9, Some(FaultKind::High), PINS));
    assert!(panel.address.set_kind(2, Some(FaultKind::Low), PINS));
    assert!(panel.address.set_marginal(2, true));
    assert!(panel.address.set_kind(5, Some(FaultKind::Bridge), PINS));
    assert!(panel.address.set_partner(5, 11, PINS));
    assert!(panel.address.set_marginal(5, true));
    assert_eq!(panel.address.pins[1].pin, 2);
    panel.address.pins[1].on_frac = 0.6;
    panel.address.pins[1].switch_hz = 12.0;
    panel
}

#[test]
fn each_panel_round_trips_a_full_section_through_toml() {
    let panel = optics_panel();
    let preset = written(|preset| panel.apply(preset));
    assert_eq!(preset.bend.optics, Some(panel.bend));
    assert_eq!(OpticsPanel::from_preset(&preset), panel);

    let panel = reclock_panel();
    let preset = written(|preset| panel.apply(preset));
    assert_eq!(preset.bend.clock, Some(panel.bend));
    assert_eq!(ReclockPanel::from_preset(&preset), panel);

    let panel = rail_panel();
    let preset = written(|preset| panel.apply(preset));
    assert_eq!(preset.bend.rail, Some(panel.bend));
    assert_eq!(RailPanel::from_preset(&preset), panel);
    let camera_source = RailPanel {
        bend: RailBend {
            source_ohm: None,
            ..panel.bend
        },
        ..panel
    };
    let preset = written(|preset| camera_source.apply(preset));
    let text = presets::to_toml(&preset).expect("serialises");
    assert!(
        !text.contains("source_ohm"),
        "the camera's source impedance is not written:\n{text}"
    );
    assert_eq!(RailPanel::from_preset(&preset), camera_source);

    let panel = memory_panel();
    let preset = written(|preset| panel.apply(preset));
    let faults = &preset.bend.memory.as_ref().expect("the memory bend").faults;
    assert_eq!(faults.len(), 3);
    let mut loaded = panel.clone();
    loaded.load(&preset);
    assert_eq!(loaded, panel, "the panel reads back its own preset");
    let fresh = MemoryPanel::from_preset(&preset);
    assert_eq!(fresh.bend(), panel.bend());
    assert_eq!(
        fresh.address.setting(memory::DEFAULT_PIN).kind,
        None,
        "a fresh panel's default fault is set back to sound by a preset without it"
    );
}

#[test]
fn a_switched_off_panel_removes_its_section_and_restores_the_same_values() {
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
        "optics",
        optics_panel(),
        |panel| &mut panel.enabled,
        OpticsPanel::apply,
        OpticsPanel::load,
        |preset| preset.bend.optics.is_some(),
    );
    check(
        "master clock",
        reclock_panel(),
        |panel| &mut panel.enabled,
        ReclockPanel::apply,
        ReclockPanel::load,
        |preset| preset.bend.clock.is_some(),
    );
    check(
        "memory",
        memory_panel(),
        |panel| &mut panel.enabled,
        MemoryPanel::apply,
        MemoryPanel::load,
        |preset| preset.bend.memory.is_some(),
    );
    check(
        "power",
        rail_panel(),
        |panel| &mut panel.enabled,
        RailPanel::apply,
        RailPanel::load,
        |preset| preset.bend.rail.is_some(),
    );
}

#[test]
fn the_pin_grid_writes_exactly_the_faults_set() {
    let panel = memory_panel();
    let bend = panel.bend().expect("on");
    assert_eq!(
        bend.faults,
        [
            AddressFault {
                pin: 9,
                kind: FaultKind::High,
                with: None,
                on_frac: None,
                switch_hz: None,
            },
            AddressFault {
                pin: 2,
                kind: FaultKind::Low,
                with: None,
                on_frac: Some(0.6),
                switch_hz: Some(12.0),
            },
            AddressFault {
                pin: 5,
                kind: FaultKind::Bridge,
                with: Some(11),
                on_frac: Some(0.3),
                switch_hz: Some(4.0),
            },
        ],
        "in the order the pins were set, marginal contacts with their values"
    );

    let mut panel = panel;
    assert!(panel.address.set_kind(2, None, PINS), "a2 back to sound");
    let faults: Vec<u8> = panel
        .bend()
        .expect("on")
        .faults
        .iter()
        .map(|fault| fault.pin)
        .collect();
    assert_eq!(faults, [9, 5], "a sound pin writes no fault");
    assert_eq!(
        panel.address.setting(2),
        PinSetting {
            kind: None,
            ..panel.address.pins[1]
        },
        "a sound pin keeps its settings"
    );
    assert!(panel.address.set_kind(2, Some(FaultKind::High), PINS));
    let back = panel.address.setting(2);
    assert!(
        back.marginal && (back.on_frac - 0.6).abs() < 1e-12,
        "faulted again, it is the same marginal contact: {back:?}"
    );

    let mut cleared = MemoryPanel {
        enabled: true,
        ..MemoryPanel::default()
    };
    assert!(cleared.address.set_kind(memory::DEFAULT_PIN, None, PINS));
    let preset = written(|preset| cleared.apply(preset));
    assert_eq!(
        preset.bend.memory,
        Some(MemoryBend::default()),
        "every pin sound writes an empty section"
    );
    assert_eq!(MemoryPanel::from_preset(&preset).bend(), cleared.bend());
}

#[test]
fn a_bridge_joins_only_another_free_pin() {
    let mut panel = memory_panel();
    assert!(
        !panel.address.set_partner(5, 5, PINS),
        "a bridge to its own pin is refused"
    );
    assert!(!panel.address.partners(5, PINS).contains(&5));
    assert!(
        !panel.address.set_partner(5, 9, PINS),
        "a9 carries a fault of its own"
    );
    assert!(
        !panel.address.set_partner(5, PINS, PINS),
        "a12 is beyond the SDRAM"
    );
    assert_eq!(panel.address.bridged_by(11), Some(5));
    assert!(
        !panel.address.set_kind(11, Some(FaultKind::Low), PINS),
        "the other end of a bridge carries that bridge"
    );
    let partners = panel.address.partners(5, PINS);
    assert!(partners.contains(&11), "its present partner stays on offer");
    assert!(
        !partners.contains(&9) && !partners.contains(&2),
        "faulted pins are not: {partners:?}"
    );
    assert!(panel.address.set_partner(5, 6, PINS));
    assert_eq!(panel.address.bridged_by(11), None, "a11 is free again");
    assert!(panel.address.is_free(11));

    let mut fresh = MemoryPanel {
        enabled: true,
        address: PinGrid::default(),
        ..MemoryPanel::default()
    };
    assert!(fresh.address.set_kind(4, Some(FaultKind::High), PINS));
    assert!(fresh.address.set_kind(3, Some(FaultKind::Bridge), PINS));
    assert_eq!(
        fresh.address.setting(3).with,
        2,
        "a new bridge joins the nearest free pin, a4 being faulted"
    );
    let preset = written(|preset| fresh.apply(preset));
    assert!(
        vcb_core::Simulator::builder((camera(), preset))
            .input_size(64, 48)
            .build()
            .is_ok(),
        "the core accepts what the grid wrote"
    );

    let mut crowded = MemoryPanel {
        enabled: true,
        address: PinGrid::default(),
        ..MemoryPanel::default()
    };
    assert!(crowded.address.set_kind(0, Some(FaultKind::High), 2));
    assert!(
        !crowded.address.set_kind(1, Some(FaultKind::Bridge), 2),
        "no free pin is left to bridge to"
    );
    assert_eq!(crowded.address.setting(1).kind, None);
}

#[test]
fn the_pins_name_the_address_bits_they_carry() {
    let camera = camera();
    assert_eq!(memory::pin_count(&camera.memory), PINS);
    assert_eq!(
        memory::pin_bits(&camera.memory, 3),
        "a3 carries column bit 3 and row bit 14 of the word address"
    );
    assert_eq!(
        memory::pin_bits(&camera.memory, 10),
        "a10 carries row bit 21 of the word address"
    );
}

#[test]
fn the_oscillator_readout_shows_the_ratio_and_the_sensor_line() {
    let camera = camera();
    let nominal = camera.still.pixel_clock_hz;
    let plain = Timing::derive(&camera, &Preset::new("a520"), 2272, 1704).expect("native still");
    assert_eq!(
        reclock::ratio_readout(27.0, nominal, Some(&plain)),
        "100.000 % of 27.0 MHz, sensor line 132.00 µs"
    );
    let shown = reclock::ratio_readout(26.7, nominal, Some(&plain));
    assert_eq!(shown, "98.889 % of 27.0 MHz, sensor line 133.48 µs");
    assert_eq!(
        reclock::ratio_readout(26.7, nominal, None),
        "98.889 % of 27.0 MHz",
        "without a scene there is no line to show"
    );

    let mut preset = Preset::new("a520");
    reclock_panel().apply(&mut preset);
    let bent = Timing::derive(&camera, &preset, 2272, 1704).expect("native still");
    assert!(
        shown.contains(&format!("{:.2} µs", bent.line_period_s * 1e6)),
        "the readout's line is the bent timing's: {shown}"
    );
    assert_eq!(
        reclock::ratio_readout(26.7, nominal, Some(&bent)),
        shown,
        "the same whether or not the timing already has the bend"
    );

    let slip = reclock::slip_px(26.7 / 27.0, &camera, vcb_core::preset::Mode::Still);
    assert!(
        (slip + 39.6).abs() < 0.05,
        "a slow sensor clock slips 39.6 px of the 3564 px line, got {slip}"
    );
}

#[test]
fn the_knobs_show_their_units_and_span_the_defaults() {
    let hd = &reclock::HD_LOCK;
    assert_eq!(hd.format(0.0), "free-running");
    assert_eq!(hd.format(12.0), "12.0 px");
    assert_eq!(hd.parse("free-running"), Some(0.0));
    assert_eq!(hd.parse("12.5 px"), Some(12.5));
    assert_eq!(hd.parse("8"), Some(8.0));
    assert_eq!(reclock::OSCILLATOR.format(26.7), "26.700 MHz");
    assert_eq!(
        optics::PULLED.format(optics::PULLED.scale.show(0.45)),
        "45 % of height"
    );

    let camera = camera();
    let defaults = OpticsBend::default();
    let clock = ReclockBend::new(27.0);
    let rail = RailBend::default();
    for (name, knob, value) in [
        ("pulled", &optics::PULLED, defaults.ir_cut_pulled_frac),
        ("level", &optics::LEVEL, defaults.nir_scale),
        ("foliage", &optics::FOLIAGE, defaults.foliage_nir),
        ("red weight", &optics::WEIGHT, defaults.nir_weights[0]),
        (
            "oscillator",
            &reclock::OSCILLATOR,
            camera.still.pixel_clock_hz / 1e6,
        ),
        ("drift", &reclock::DRIFT, clock.drift_ppm),
        ("drift rate", &reclock::DRIFT_RATE, clock.drift_hz),
        ("supply sensitivity", &reclock::SUPPLY, clock.ppm_per_v),
        ("hd lock", &reclock::HD_LOCK, clock.hd_lock_px),
        ("spike length", &rail::SPIKE_LENGTH, rail.spike_ms),
        ("ripple frequency", &rail::RIPPLE_FREQUENCY, rail.ripple_hz),
        ("source impedance", &rail::SOURCE, camera.power.source_ohm),
        ("faulted", &memory::FAULTED, 0.3),
        ("switching", &memory::SWITCHING, 4.0),
    ] {
        let shown = knob.scale.show(value);
        assert!(
            knob.range.contains(&shown),
            "{name}: the default {shown} lies outside {:?}",
            knob.range
        );
        assert!(!knob.unit.trim().is_empty(), "{name} has a unit");
    }
}

#[test]
fn the_power_readout_names_the_thresholds_a_load_reaches() {
    let power = camera().power;
    assert_eq!(
        rail::thresholds(&power),
        "Rail 3.30 V; AFE lockout 2.70 V, processor brownout 2.40 V, logic minimum 2.00 V."
    );
    let steady = RailBend {
        load_ma: 150.0,
        ..RailBend::default()
    };
    let Sag { steady_v, lowest_v } = rail::sag(&power, &steady);
    assert!(
        (steady_v - 3.225).abs() < 1e-9 && (lowest_v - steady_v).abs() < 1e-12,
        "150 mA through 0.5 Ω sags the rail by 75 mV: {steady_v} {lowest_v}"
    );
    let rippled = RailBend {
        ripple_mv: 20.0,
        ..steady
    };
    let Sag { lowest_v, .. } = rail::sag(&power, &rippled);
    assert!(
        (steady_v - lowest_v - 0.02).abs() < 1e-4,
        "100 Hz ripple passes the 23.5 µs RC almost whole: {lowest_v}"
    );

    let steady = RailBend {
        load_ma: 160.0,
        ..RailBend::default()
    };
    assert_eq!(
        rail::reach(&power, &steady),
        "The load holds the rail at 3.22 V; the bus bend's own current comes on top."
    );
    let spiking = RailBend {
        spike_ma: 1400.0,
        spike_hz: 3.0,
        ..steady
    };
    let text = rail::reach(&power, &spiking);
    assert!(
        text.contains("3.22 V, 2.52 V at the lowest, below the AFE lockout;"),
        "a 1.4 A spike reaches the AFE's lockout: {text}"
    );
    let dying = RailBend {
        source_ohm: Some(1.0),
        ..spiking
    };
    let text = rail::reach(&power, &dying);
    assert!(
        text.contains("below the AFE lockout, the processor brownout and the logic minimum"),
        "{text}"
    );
}

/// A knob's name and the change of the preset it makes.
type Turn = (&'static str, fn(&mut Preset));

/// A small running job at match-input resolution with every new bend on.
fn bent_job() -> Job {
    let mut preset = Preset::new("a520");
    preset.seed = 3;
    preset.readout.resolution = Resolution::MatchInput;
    optics_panel().apply(&mut preset);
    reclock_panel().apply(&mut preset);
    memory_panel().apply(&mut preset);
    RailPanel {
        enabled: true,
        bend: RailBend {
            source_ohm: None,
            ..rail_panel().bend
        },
    }
    .apply(&mut preset);
    let (width, height) = (128, 96);
    let rgb: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            let x = (i % width * 255 / width) as u8;
            [x, 150, 255 - x]
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
    let turns: [Turn; 5] = [
        ("the oscillator", |p| {
            if let Some(clock) = p.bend.clock.as_mut() {
                clock.osc_mhz = 26.9;
            }
        }),
        ("the rail load", |p| {
            if let Some(rail) = p.bend.rail.as_mut() {
                rail.load_ma = 400.0;
            }
        }),
        ("the filter", |p| {
            if let Some(optics) = p.bend.optics.as_mut() {
                optics.ir_cut_pulled_frac = 0.8;
            }
        }),
        ("an address fault", |p| {
            if let Some(memory) = p.bend.memory.as_mut() {
                memory.faults.remove(0);
            }
        }),
        ("the HD lock", |p| {
            if let Some(clock) = p.bend.clock.as_mut() {
                clock.hd_lock_px = 0.0;
            }
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
        assert!(
            !(output.reconfigured.buffers_kept() && output.reconfigured.bus_reset()),
            "the app's circuit-reset notice stays off for {name}"
        );
        assert_eq!(output.index, index as u64 + 1, "{name}: the next frame");
    }
}
