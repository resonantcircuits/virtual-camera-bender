//! The optics, rail, clock and memory bends in the chain: inert sections change no
//! byte, adding, removing or turning one keeps the running camera's state, and the four
//! connected replay from reset.

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::preset::{
    AddressFault, FaultKind, MemoryBend, Mode, OpticsBend, RailBend, ReclockBend, Resolution,
};
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing, memory};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

/// Frames rendered from reset in each comparison.
const FRAMES: u64 = 3;

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The shipped clean preset in `mode`, with the sensor set to the scene's size.
fn clean(mode: Mode) -> Preset {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../presets/a520-clean.toml");
    let mut preset: Preset = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    preset.readout.mode = mode;
    preset.readout.resolution = Resolution::MatchInput;
    preset
}

fn build(preset: Preset) -> Simulator {
    Simulator::builder((camera(), preset))
        .input_size(WIDTH, HEIGHT)
        .build()
        .unwrap()
}

/// A scene of saturated foliage green, sky blue and warm red over a gradient.
fn scene() -> Scene {
    let rgb: Vec<u8> = (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).flat_map(move |x| {
                let v = ((x * 255 / WIDTH + y * 96 / HEIGHT) % 256) as u8;
                match (x * 3 / WIDTH, y * 2 / HEIGHT) {
                    (0, 0) => [60, 140, 40],
                    (1, 0) => [90, 150, 230],
                    (2, 0) => [220, 70, 40],
                    _ => [v, v / 2 + 60, 255 - v],
                }
            })
        })
        .collect();
    Scene::from_srgb8(WIDTH, HEIGHT, &rgb)
}

/// Frames `0..FRAMES` in order.
fn render(sim: &mut Simulator, scene: &Scene) -> Vec<Frame> {
    (0..FRAMES)
        .map(|index| sim.frame(scene, index).clone())
        .collect()
}

/// Sets one of the four sections of a preset.
type Section = (&'static str, fn(&mut Preset));

/// Each section at values that leave the camera as it is: the IR-cut filter in place,
/// no load added to the rail, the oscillator at the camera's own 27 MHz, no address
/// fault.
const INERT: [Section; 4] = [
    ("optics", |p| {
        p.bend.optics = Some(OpticsBend {
            ir_cut_pulled_frac: 0.0,
            ..OpticsBend::default()
        });
    }),
    ("rail", |p| p.bend.rail = Some(RailBend::default())),
    ("clock", |p| p.bend.clock = Some(ReclockBend::new(27.0))),
    ("memory", |p| p.bend.memory = Some(MemoryBend::default())),
];

/// Each section bent: the filter off, a loaded rail, a detuned oscillator, a stuck pin.
const BENT: [Section; 4] = [
    ("optics", |p| p.bend.optics = Some(OpticsBend::default())),
    ("rail", |p| {
        p.bend.rail = Some(RailBend {
            load_ma: 150.0,
            ..RailBend::default()
        });
    }),
    ("clock", |p| p.bend.clock = Some(ReclockBend::new(26.4))),
    ("memory", |p| {
        p.bend.memory = Some(MemoryBend {
            faults: vec![stuck(3)],
        });
    }),
];

/// Address pin `pin` stuck high.
fn stuck(pin: u8) -> AddressFault {
    AddressFault {
        pin,
        kind: FaultKind::High,
        with: None,
        on_frac: None,
        switch_hz: None,
    }
}

#[test]
fn the_camera_runs_on_one_clock_without_a_clock_bend() {
    let camera = camera();
    for mode in [Mode::Still, Mode::Movie] {
        for (resolution, size) in [
            (Resolution::MatchInput, (WIDTH, HEIGHT)),
            (Resolution::MatchInput, (1920, 1080)),
            (Resolution::Native, (WIDTH, HEIGHT)),
        ] {
            let mut preset = clean(mode);
            preset.readout.resolution = resolution;
            let timing = Timing::derive(&camera, &preset, size.0, size.1).unwrap();
            assert_eq!(
                timing.dsp_pixel_period_s.to_bits(),
                timing.pixel_period_s.to_bits(),
                "{mode} {resolution} {size:?}: the processor captures on the sensor's clock"
            );
            assert!(
                (timing.clock_ratio - 1.0).abs() < f64::EPSILON,
                "{mode} {resolution} {size:?}: ratio {}",
                timing.clock_ratio
            );
        }
    }
}

#[test]
fn inert_optics_rail_clock_and_memory_sections_render_byte_identically() {
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        let plain = clean(mode);
        let expected = render(&mut build(plain.clone()), &scene);
        let mut every = plain.clone();
        for (name, set) in INERT {
            let mut alone = plain.clone();
            set(&mut alone);
            set(&mut every);
            assert_eq!(
                render(&mut build(alone), &scene),
                expected,
                "{mode}: the inert {name} section changed the frames"
            );
        }
        let mut sim = build(every);
        assert_eq!(
            render(&mut sim, &scene),
            expected,
            "{mode}: the four inert sections changed the frames"
        );
        sim.reset();
        assert_eq!(
            render(&mut sim, &scene),
            expected,
            "{mode}: the four inert sections changed the frames after a reset"
        );
    }
}

#[test]
fn adding_or_removing_a_section_keeps_the_running_camera() {
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        for (name, set) in INERT.into_iter().chain(BENT) {
            let plain = clean(mode);
            let mut sim = build(plain.clone());
            render(&mut sim, &scene);
            let mut with = plain.clone();
            set(&mut with);
            assert_eq!(
                sim.set_preset(with.clone()).unwrap(),
                KEPT,
                "{mode}: adding {name}"
            );
            assert_eq!(sim.preset(), &with);
            sim.frame(&scene, FRAMES);
            assert_eq!(
                sim.set_preset(plain).unwrap(),
                KEPT,
                "{mode}: removing {name}"
            );
            sim.frame(&scene, FRAMES + 1);
        }
    }
}

#[test]
fn turning_a_bend_knob_keeps_the_running_camera() {
    let scene = scene();
    let mut bent = clean(Mode::Movie);
    for (_, set) in BENT {
        set(&mut bent);
    }
    let mut sim = build(bent.clone());
    render(&mut sim, &scene);
    let turns: [Section; 4] = [
        ("the filter's position", |p| {
            p.bend.optics.as_mut().unwrap().ir_cut_pulled_frac = 0.35;
        }),
        ("the rail's load", |p| {
            p.bend.rail.as_mut().unwrap().load_ma = 400.0;
        }),
        ("the oscillator", |p| {
            p.bend.clock.as_mut().unwrap().osc_mhz = 24.0;
        }),
        ("the address faults", |p| {
            p.bend.memory.as_mut().unwrap().faults.push(stuck(6));
        }),
    ];
    for (index, (knob, turn)) in (FRAMES..).zip(turns) {
        turn(&mut bent);
        assert_eq!(
            sim.set_preset(bent.clone()).unwrap(),
            KEPT,
            "turning {knob}"
        );
        sim.frame(&scene, index);
    }
}

#[test]
fn a_new_oscillator_leaves_the_kept_buffers_on_the_sensor_clock() {
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        let plain = clean(mode);
        let mut sim = build(plain.clone());
        sim.frame(&scene, 0);
        let mut reclocked = plain;
        reclocked.bend.clock = Some(ReclockBend::new(20.0));
        assert_eq!(sim.set_preset(reclocked).unwrap(), KEPT, "{mode}");
        // Every stage checks its buffers against its parameters' clock.
        sim.frame(&scene, 1);
        assert_eq!(
            sim.codes().layout().pixel_period_s().to_bits(),
            sim.timing().pixel_period_s.to_bits(),
            "{mode}: the codes carry the sensor-side pixel period"
        );
    }
}

#[test]
fn the_rail_holds_the_nominal_voltage_on_every_readout_line_without_a_rail_bend() {
    let scene = scene();
    let camera = camera();
    for mode in [Mode::Still, Mode::Movie] {
        let mut sim = build(clean(mode));
        render(&mut sim, &scene);
        let rail = sim.rail();
        assert_eq!(rail.lines(), sim.timing().rows_read, "{mode}");
        assert_eq!(
            rail.nominal_v().to_bits(),
            camera.power.rail_v.to_bits(),
            "{mode}"
        );
        assert!(rail.is_nominal(), "{mode}");
        let last = rail.lines() - 1;
        for frac in [0.0, 0.25, 1.0] {
            assert!(
                (rail.v_at(last, frac) - camera.power.rail_v).abs() < 1e-12,
                "{mode}: {frac} of the last line"
            );
        }
    }
}

#[test]
fn removing_the_rail_and_memory_bends_returns_them_to_power_on() {
    let scene = scene();
    let mut bent = clean(Mode::Still);
    for (_, set) in BENT {
        set(&mut bent);
    }
    let mut sim = build(bent.clone());
    render(&mut sim, &scene);
    let mut plain = bent;
    plain.bend.rail = None;
    plain.bend.memory = None;
    assert_eq!(sim.set_preset(plain).unwrap(), KEPT);
    assert!(
        sim.rail().is_nominal(),
        "the rail is back at the nominal voltage"
    );
    assert_eq!(sim.memory_state(), &memory::State::default());
}

/// The four bends wired together: rail spikes that brown the processor out, an
/// oscillator that follows the rail, the SDRAM keeping the rows the processor missed.
fn connected(ppm_per_v: f64) -> Preset {
    let power = camera().power;
    let mut preset = clean(Mode::Still);
    preset.bend.optics = Some(OpticsBend::default());
    preset.bend.rail = Some(RailBend {
        load_ma: 100.0,
        spike_ma: (power.rail_v - power.dsp_brownout_v + 0.2) / 5.0 * 1000.0,
        spike_hz: 6.0,
        spike_ms: 10.0,
        source_ohm: Some(5.0),
        ..RailBend::default()
    });
    preset.bend.clock = Some(ReclockBend {
        drift_ppm: 500.0,
        ppm_per_v,
        ..ReclockBend::new(26.9)
    });
    preset.bend.memory = Some(MemoryBend::default());
    preset
}

#[test]
fn the_connected_bends_replay_from_reset_and_keep_their_state_through_a_knob() {
    let scene = scene();
    let brownout_v = camera().power.dsp_brownout_v;
    let mut sim = build(connected(2000.0));
    let mut browned = 0;
    let first: Vec<Frame> = (0..6)
        .map(|index| {
            let frame = sim.frame(&scene, index).clone();
            let rail = sim.rail();
            browned += usize::from((0..rail.lines()).any(|line| rail.start_v(line) < brownout_v));
            frame
        })
        .collect();
    assert!(browned > 0, "no frame browned the processor out");
    sim.reset();
    let again: Vec<Frame> = (0..6)
        .map(|index| sim.frame(&scene, index).clone())
        .collect();
    assert_eq!(again, first, "from reset");
    let mut fresh = build(connected(2000.0));
    let third: Vec<Frame> = (0..6)
        .map(|index| fresh.frame(&scene, index).clone())
        .collect();
    assert_eq!(third, first, "from a new simulator");

    // A new supply sensitivity keeps the rail, the drift and the SDRAM's content.
    let (rail, drift, sdram) = (
        sim.power_state().clone(),
        sim.capture_state().clone(),
        sim.memory_state().clone(),
    );
    assert_eq!(sim.set_preset(connected(-2000.0)).unwrap(), KEPT);
    assert_eq!(sim.power_state(), &rail);
    assert_eq!(sim.capture_state(), &drift);
    assert_eq!(sim.memory_state(), &sdram);
    sim.frame(&scene, 6);
}
