//! The supply rail: its sag under load and spikes, its memory, and what it does to the
//! AFE, the bus bend and the processor.

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::adc::Codes;
use vcb_core::bus::{self, Target};
use vcb_core::isp::{self, Raw};
use vcb_core::power::{self, Rail};
use vcb_core::preset::{Mode, Orientation, RailBend, Resolution};
use vcb_core::readout::{Direction, Layout};
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing, noise};

/// A small landscape frame for whole-chain renders where lines may be long.
const SMALL: (u32, u32) = (64, 48);

/// A narrow, tall landscape frame: at the native frame period its lines last about
/// 220 µs, close to the native still line, so rail and clamp time constants span a
/// few lines.
const TALL: (u32, u32) = (64, 1024);

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A shipped preset at match-input resolution, held level, in `mode`.
fn shipped(name: &str, mode: Mode) -> Preset {
    let path = format!(
        "{}/../../presets/a520-{name}.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut preset: Preset = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    preset.readout.mode = mode;
    preset.readout.resolution = Resolution::MatchInput;
    preset.readout.orientation = Orientation::Landscape;
    preset
}

/// The clean preset with a rail bend.
fn railed(mode: Mode, bend: RailBend) -> Preset {
    let mut preset = shipped("clean", mode);
    preset.bend.rail = Some(bend);
    preset
}

fn build(preset: Preset, (width, height): (u32, u32)) -> Simulator {
    Simulator::builder((camera(), preset))
        .input_size(width, height)
        .build()
        .unwrap()
}

/// A horizontal ramp from black to white on every row, under a vertical tint.
fn ramp((width, height): (u32, u32)) -> Scene {
    let rgb: Vec<u8> = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let v = (x * 255 / (width - 1)) as u8;
                [v, v.saturating_sub((y * 40 / height) as u8), v]
            })
        })
        .collect();
    Scene::from_srgb8(width, height, &rgb)
}

/// A uniform mid-grey card.
fn grey((width, height): (u32, u32)) -> Scene {
    Scene::from_srgb8(width, height, &vec![118; (width * height * 3) as usize])
}

/// Frames `0..count` in order.
fn render(sim: &mut Simulator, scene: &Scene, count: u64) -> Vec<Frame> {
    (0..count)
        .map(|index| sim.frame(scene, index).clone())
        .collect()
}

/// The 12-bit words of readout line `line` of the last frame.
fn line_words(codes: &Codes, line: u32) -> &[u16] {
    let len = codes.layout().line_len() as usize;
    &codes.words()[line as usize * len..(line as usize + 1) * len]
}

/// The mean optical-black word and the mean active word of readout line `line`.
fn black_and_signal(codes: &Codes, line: u32) -> (f64, f64) {
    let ob = codes.layout().ob_cols() as usize;
    let words = line_words(codes, line);
    let mean =
        |words: &[u16]| words.iter().map(|&w| f64::from(w)).sum::<f64>() / words.len() as f64;
    (mean(&words[..ob]), mean(&words[ob..]))
}

/// Resolves the rail of `preset` for a timing of the given input size.
fn rail_params(preset: &Preset, (width, height): (u32, u32)) -> (power::Params, Layout) {
    let camera = camera();
    let timing = Timing::derive(&camera, preset, width, height).unwrap();
    let params = power::Params::resolve(&camera, preset, &timing)
        .unwrap()
        .expect("the preset bends the rail");
    (params, Layout::from_timing(&timing, Direction::TopDown))
}

#[test]
fn an_unloaded_rail_holds_the_nominal_voltage_and_changes_no_byte() {
    let rail_v = camera().power.rail_v;
    let scene = ramp(SMALL);
    for mode in [Mode::Still, Mode::Movie] {
        let expected = render(&mut build(shipped("clean", mode), SMALL), &scene, 3);
        let mut sim = build(railed(mode, RailBend::default()), SMALL);
        for (index, expected) in (0..).zip(&expected) {
            assert_eq!(sim.frame(&scene, index), expected, "{mode}: frame {index}");
            for (line, [start_v, end_v]) in sim.rail().lines_v().iter().enumerate() {
                assert!(
                    (start_v - rail_v).abs() < 1e-6 && (end_v - rail_v).abs() < 1e-6,
                    "{mode}: line {line} runs from {start_v} V to {end_v} V"
                );
            }
        }
    }
}

#[test]
fn a_steady_load_drops_the_settled_rail_by_the_source_drop() {
    let camera = camera();
    let scene = grey(SMALL);
    for source_ohm in [None, Some(2.0)] {
        let bend = RailBend {
            load_ma: 100.0,
            source_ohm,
            ..RailBend::default()
        };
        let mut sim = build(railed(Mode::Still, bend), SMALL);
        sim.frame(&scene, 0);
        let rail = sim.rail();
        let r = source_ohm.unwrap_or(camera.power.source_ohm);
        assert_eq!(
            rail.start_v(0).to_bits(),
            camera.power.rail_v.to_bits(),
            "the decoupling holds the rail at power-on"
        );
        let settled_v = camera.power.rail_v - 0.1 * r;
        let last = rail.lines() - 1;
        assert!(
            (rail.end_v(last) - settled_v).abs() < 1e-6,
            "{r} ohm: the rail settles at {} V, not {settled_v} V",
            rail.end_v(last)
        );
    }
}

#[test]
fn a_load_the_source_cannot_carry_leaves_the_rail_at_0_v() {
    // 1 A through 10 ohm would settle 10 V under the nominal rail: the loads starve
    // at 0 V instead of reversing it.
    let bend = RailBend {
        load_ma: 1000.0,
        source_ohm: Some(10.0),
        ..RailBend::default()
    };
    let (params, layout) = rail_params(&railed(Mode::Still, bend), SMALL);
    let mut rail = Rail::new(&layout, camera().power.rail_v);
    let mut state = power::State::default();
    for index in 0..2 {
        power::track(
            Some(&params),
            &mut state,
            &bus::State::default(),
            index,
            &mut rail,
        );
        for (line, [start_v, end_v]) in rail.lines_v().iter().enumerate() {
            assert!(
                *start_v >= 0.0 && *end_v >= 0.0,
                "frame {index}: line {line} runs from {start_v} V to {end_v} V"
            );
        }
        assert_eq!(
            rail.end_v(rail.lines() - 1),
            0.0,
            "frame {index}: the rail rests at 0 V"
        );
    }
}

#[test]
fn a_spike_recovers_with_the_rc_time_constant() {
    // A weak source behind the camera's decoupling: tau = 10 ohm * 47 uF = 470 us, a
    // few native still lines of 132 us.
    let bend = RailBend {
        spike_ma: 100.0,
        spike_hz: 10.0,
        spike_ms: 1.0,
        source_ohm: Some(10.0),
        ..RailBend::default()
    };
    let preset = railed(Mode::Still, bend);
    let camera = camera();
    let (params, layout) = rail_params(&preset, (2272, 1704));
    let tau_s = params.tau_s();
    let line_s = params.line_period_s;
    let rail_v = camera.power.rail_v;
    let mut rail = Rail::new(&layout, rail_v);
    let mut state = power::State::default();
    let bus = bus::State::default();
    let mut checked = 0;
    for index in 0..8 {
        power::track(Some(&params), &mut state, &bus, index, &mut rail);
        let starts: Vec<f64> = (0..rail.lines()).map(|line| rail.start_v(line)).collect();
        // A spike pulls the rail towards rail_v - 1 V; find where each one lets go.
        for line in 1..starts.len() - 2 {
            let deep = starts[line - 1] < rail_v - 0.5;
            let rising = starts[line] > starts[line - 1] + 1e-3;
            if !(deep && rising) {
                continue;
            }
            // Lines `line + 1` and `line + 2` start after the spike's end.
            let (first, second) = (rail_v - starts[line + 1], rail_v - starts[line + 2]);
            let expected = (-line_s / tau_s).exp();
            assert!(
                (second / first - expected).abs() < 1e-9,
                "frame {index}, line {line}: the deviation falls by {} per line, not {expected}",
                second / first
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 3,
        "only {checked} spikes recovered in eight frames"
    );
}

#[test]
fn a_sagging_rail_inflates_codes_above_black_and_leaves_black_alone() {
    let camera = camera();
    // 330 mA through 1 ohm settles the rail 10 % low within a few microseconds.
    let bend = RailBend {
        load_ma: 0.1 * camera.power.rail_v * 1000.0,
        source_ohm: Some(1.0),
        ..RailBend::default()
    };
    let scene = grey(SMALL);
    let mut plain = build(shipped("clean", Mode::Still), SMALL);
    let mut sagging = build(railed(Mode::Still, bend), SMALL);
    plain.frame(&scene, 0);
    sagging.frame(&scene, 0);
    let ratio_v = sagging.rail().end_v(5) / camera.power.rail_v;
    assert!(
        (ratio_v - 0.9).abs() < 1e-9,
        "the rail sits at {ratio_v} of nominal"
    );
    let (mut black, mut signal) = ([0.0; 2], [0.0; 2]);
    let lines = 2..plain.codes().layout().rows();
    for line in lines.clone() {
        for (k, sim) in [&plain, &sagging].into_iter().enumerate() {
            let (b, s) = black_and_signal(sim.codes(), line);
            black[k] += b;
            signal[k] += s - b;
        }
    }
    let count = lines.len() as f64;
    let gain = signal[1] / signal[0];
    assert!(
        (gain - 1.0 / 0.9).abs() < 0.01,
        "codes above black grow by {gain}, a ratiometric reference gives {}",
        1.0 / 0.9
    );
    let shift = (black[1] - black[0]) / count;
    assert!(
        shift.abs() < 0.5,
        "the optical black moves by {shift} codes"
    );
}

/// The clean and the spiking simulator, after rendering the same frames until a spike
/// has locked the AFE out on whole lines; returns the frame index.
fn render_until_locked_out(bend: RailBend, scene: &Scene) -> (Simulator, Simulator, u64) {
    let uvlo_v = camera().power.afe_uvlo_v;
    let mut clean = build(shipped("clean", Mode::Still), TALL);
    let mut spiking = build(railed(Mode::Still, bend), TALL);
    for index in 0..12 {
        clean.frame(scene, index);
        spiking.frame(scene, index);
        if spiking
            .rail()
            .lines_v()
            .iter()
            .any(|&[start_v, end_v]| start_v.max(end_v) < uvlo_v)
        {
            return (clean, spiking, index);
        }
    }
    panic!("no spike locked the AFE out in twelve frames");
}

#[test]
fn below_the_lockout_the_afe_holds_its_word_then_restarts_its_clamp() {
    let power = camera().power;
    assert!(
        power.dsp_brownout_v < power.afe_uvlo_v,
        "the processor must outlast the AFE for a lockout to show"
    );
    // A spike that holds the rail midway between the processor's brownout and the
    // AFE's lockout, through a 5 ohm source: tau = 235 us, about one line.
    let source_ohm = 5.0;
    let plateau_v = f64::midpoint(power.afe_uvlo_v, power.dsp_brownout_v);
    let bend = RailBend {
        spike_ma: (power.rail_v - plateau_v) / source_ohm * 1000.0,
        spike_hz: 4.0,
        spike_ms: 5.0,
        source_ohm: Some(source_ohm),
        ..RailBend::default()
    };
    let scene = ramp(TALL);
    let (clean, spiking, index) = render_until_locked_out(bend, &scene);
    let rail = spiking.rail();
    let below = |line: u32| rail.start_v(line).max(rail.end_v(line)) < power.afe_uvlo_v;
    let first = (0..rail.lines()).find(|&line| below(line)).unwrap();
    let held = (first..rail.lines())
        .take_while(|&line| below(line))
        .count() as u32;
    // Every sample of a locked-out line, optical black included, repeats one word.
    let word = line_words(spiking.codes(), first)[0];
    for line in first..first + held {
        assert!(
            line_words(spiking.codes(), line).iter().all(|&w| w == word),
            "frame {index}: locked-out line {line} does not repeat word {word}"
        );
    }
    // The lines after the rail comes back differ from the clean camera's, and the
    // pedestal, starting from the lockout's level, lags the recovering rail: black dips.
    let back = (first + held..rail.lines())
        .find(|&line| rail.start_v(line) >= power.afe_uvlo_v)
        .unwrap();
    for line in back..back + power.afe_restart_lines {
        assert_ne!(
            line_words(spiking.codes(), line),
            line_words(clean.codes(), line),
            "frame {index}: line {line} after the lockout is the clean camera's"
        );
    }
    let (dipped, _) = black_and_signal(spiking.codes(), back + 1);
    let (normal, _) = black_and_signal(clean.codes(), back + 1);
    assert!(
        dipped < normal - 2.0,
        "frame {index}: black after the restart at {dipped}, the clean camera's {normal}"
    );
    // Once the rail and the clamp have settled, black is the clean camera's again.
    let settled = back + 40;
    assert!(
        (back + 10..=settled).all(|line| rail.start_v(line) > power.rail_v - 1e-3),
        "frame {index}: the rail has not recovered by line {settled}"
    );
    let (late, _) = black_and_signal(spiking.codes(), settled);
    let (normal, _) = black_and_signal(clean.codes(), settled);
    assert!(
        (late - normal).abs() < 1.0,
        "frame {index}: black at {late} long after the restart, the clean camera's {normal}"
    );
}

/// Codes of a frame: optical black at the camera's black code, active samples hashed.
fn hashed_codes(layout: &Layout, black: u16) -> Codes {
    let mut codes = Codes::new(layout.clone());
    let (ob, len) = (layout.ob_cols() as usize, layout.line_len() as usize);
    for (i, code) in codes.words_mut().iter_mut().enumerate() {
        *code = if i % len < ob {
            black
        } else {
            (noise::hash_u32(i as u32, 0, noise::salt("power test"), 3) & 0x0fff) as u16
        };
    }
    codes
}

#[test]
fn a_brownout_at_a_line_start_drops_the_rest_of_the_frame() {
    let camera = camera();
    let preset = shipped("clean", Mode::Still);
    let timing = Timing::derive(&camera, &preset, TALL.0, TALL.1).unwrap();
    let params = isp::Params::resolve(&camera, &preset, &timing).unwrap();
    for direction in [Direction::TopDown, Direction::BottomUp] {
        let layout = Layout::from_timing(&timing, direction);
        let codes = hashed_codes(&layout, camera.adc.black_level);
        let nominal = Rail::new(&layout, camera.power.rail_v);
        let mut whole = Raw::new(layout.cols(), layout.rows());
        isp::clamp_black(&params, &codes, &nominal, &mut whole);
        assert_eq!(whole.dropped_from(), None);

        // A dip inside line 300 that recovers before line 301 starts drops nothing; a
        // rail that starts line 600 below the brownout drops it and every later line.
        let mut rail = nominal.clone();
        let low_v = params.dsp_brownout_v - 0.1;
        rail.set(300, camera.power.rail_v, low_v);
        for line in 600..rail.lines() {
            rail.set(line, low_v, low_v);
        }
        let mut raw = Raw::new(layout.cols(), layout.rows());
        isp::clamp_black(&params, &codes, &rail, &mut raw);
        let first = layout.image_row(600);
        assert_eq!(raw.dropped_from(), Some(first), "{direction:?}");
        for line in 0..layout.rows() {
            let row = layout.image_row(line);
            let sites = raw.plane().row(row);
            if line < 600 {
                assert_eq!(sites, whole.plane().row(row), "{direction:?}: line {line}");
            } else {
                assert!(
                    sites.iter().all(|&site| site == 0.0),
                    "{direction:?}: line {line} was captured after the brownout"
                );
            }
        }

        // The next frame, on a healthy rail, is captured whole again.
        isp::clamp_black(&params, &codes, &nominal, &mut raw);
        assert_eq!(raw.dropped_from(), None, "{direction:?}");
        assert_eq!(raw.plane(), whole.plane(), "{direction:?}");
    }
}

#[test]
fn a_spike_below_the_brownout_blacks_out_the_rest_of_the_frame() {
    let power = camera().power;
    // 1 V below nominal through 5 ohm: under the processor's brownout.
    let bend = RailBend {
        spike_ma: (power.rail_v - power.dsp_brownout_v + 0.2) / 5.0 * 1000.0,
        spike_hz: 4.0,
        spike_ms: 3.0,
        source_ohm: Some(5.0),
        ..RailBend::default()
    };
    let scene = grey(TALL);
    let mut sim = build(railed(Mode::Still, bend), TALL);
    let (mut dropped, mut whole) = (0, 0);
    for index in 0..12 {
        let frame = sim.frame(&scene, index).clone();
        let rail = sim.rail();
        let drop = (0..rail.lines()).find(|&line| rail.start_v(line) < power.dsp_brownout_v);
        let mean = |row: u32| {
            (0..frame.width())
                .flat_map(|x| frame.pixel(x, row))
                .map(f64::from)
                .sum::<f64>()
                / f64::from(3 * frame.width())
        };
        match drop {
            // Rows of zero codes develop black; allow the codec a block to settle.
            Some(line) if line + 16 < rail.lines() => {
                for row in line + 16..rail.lines() {
                    assert!(
                        mean(row) < 2.0,
                        "frame {index}: row {row} after the brownout"
                    );
                }
                assert!(
                    line < 8 || mean(line - 8) > 40.0,
                    "frame {index}: before it"
                );
                dropped += 1;
            }
            Some(_) => {}
            None => {
                assert!(
                    (0..rail.lines()).all(|row| mean(row) > 40.0),
                    "frame {index}: a whole frame has black rows"
                );
                whole += 1;
            }
        }
    }
    assert!(
        dropped > 0 && whole > 0,
        "{dropped} dropped and {whole} whole frames"
    );
}

#[test]
fn the_bus_bend_records_the_current_it_draws_from_the_rail() {
    let scene = ramp(SMALL);
    let mean_current = |pot_kohm: f64| {
        let mut preset = shipped("hpf", Mode::Still);
        preset.bend.bus.as_mut().unwrap().pot_kohm = pot_kohm;
        let mut sim = build(preset, SMALL);
        sim.frame(&scene, 0);
        let currents = &sim.bus_state().line_current_ma;
        assert_eq!(currents.len() as u32, sim.rail().lines());
        assert!(
            currents.iter().all(|&ma| ma > 0.0),
            "pot {pot_kohm} kohm: a line drew no current: {currents:?}"
        );
        currents.iter().map(|&ma| f64::from(ma)).sum::<f64>() / currents.len() as f64
    };
    // D9 and D10 share node S, so they contest each other whatever the pot; the pot
    // to ground adds its own path, the more the lower it is set.
    let (low, high) = (mean_current(0.05), mean_current(1.0));
    assert!(
        low > 1.2 * high,
        "the pot at 50 ohm draws {low} mA, at 1 kohm {high} mA"
    );
}

#[test]
fn the_bends_own_current_loads_the_rail_one_frame_later() {
    let camera = camera();
    let mut preset = shipped("hpf", Mode::Still);
    preset.bend.rail = Some(RailBend {
        source_ohm: Some(20.0),
        ..RailBend::default()
    });
    let scene = ramp(SMALL);
    let mut sim = build(preset.clone(), SMALL);
    sim.frame(&scene, 0);
    assert!(
        sim.rail().is_nominal(),
        "no current is recorded before the first frame"
    );
    let drawn_ma = sim.bus_state().line_current_ma.clone();
    sim.frame(&scene, 1);
    let (params, _) = rail_params(&preset, SMALL);
    let decay = (-params.line_period_s / params.tau_s()).exp();
    let rail = sim.rail();
    for (line, &ma) in (0..rail.lines()).zip(&drawn_ma) {
        let settled_v = camera.power.rail_v - 20.0 * f64::from(ma) * 1e-3;
        let expected = settled_v + (rail.start_v(line) - settled_v) * decay;
        assert!(
            (rail.end_v(line) - expected).abs() < 1e-9,
            "line {line}: the rail ends at {} V, the RC gives {expected} V",
            rail.end_v(line)
        );
    }
    assert!(
        rail.end_v(5) < camera.power.rail_v - 0.05,
        "the bend sags the rail"
    );
}

#[test]
fn below_the_logic_minimum_the_target_line_returns_to_its_driver() {
    let camera = camera();
    let preset = shipped("inv", Mode::Still);
    let Target::Pin(target) = preset.bend.bus.as_ref().unwrap().target else {
        panic!("the inverter preset drives a pin");
    };
    let timing = Timing::derive(&camera, &preset, SMALL.0, SMALL.1).unwrap();
    let params = bus::Params::resolve(&camera, &preset, &timing)
        .unwrap()
        .unwrap();
    let layout = Layout::from_timing(&timing, Direction::TopDown);
    let adc = hashed_codes(&layout, camera.adc.black_level);
    let bit = |word: u16| (word >> target) & 1;
    let run = |rail_v: f64| {
        let mut codes = adc.clone();
        let mut rail = Rail::new(&layout, camera.power.rail_v);
        rail.lines_v_mut().fill([rail_v; 2]);
        bus::bend(
            Some(&params),
            &mut bus::State::default(),
            &rail,
            0,
            &mut codes,
        );
        codes
    };
    let differing = |codes: &Codes| {
        codes
            .words()
            .iter()
            .zip(adc.words())
            .filter(|&(&latched, &driven)| bit(latched) != bit(driven))
            .count()
    };
    let powered = run(camera.power.rail_v);
    assert!(
        differing(&powered) > adc.words().len() / 10,
        "the powered inverter drives the target"
    );
    let browned = run(camera.power.logic_min_v - 0.2);
    assert_eq!(
        differing(&browned),
        0,
        "with the logic unpowered D{target} carries the ADC's own bit"
    );
}

#[test]
fn a_bent_rail_replays_identically_from_reset() {
    let mut preset = shipped("hpf", Mode::Movie);
    preset.bend.rail = Some(RailBend {
        load_ma: 40.0,
        spike_ma: 150.0,
        spike_hz: 6.0,
        spike_ms: 8.0,
        ripple_mv: 60.0,
        ripple_hz: 7.0,
        source_ohm: Some(4.0),
    });
    let scene = ramp(SMALL);
    let mut sim = build(preset.clone(), SMALL);
    let first = render(&mut sim, &scene, 4);
    let rail_after = sim.rail().clone();
    sim.reset();
    assert_eq!(sim.power_state(), &power::State::default());
    assert_eq!(render(&mut sim, &scene, 4), first);
    assert_eq!(sim.rail(), &rail_after);
    assert_eq!(render(&mut build(preset, SMALL), &scene, 4), first);
}

#[test]
fn a_new_load_keeps_the_running_camera_and_the_capacitor_voltage() {
    let camera = camera();
    let load = |load_ma: f64| {
        railed(
            Mode::Still,
            RailBend {
                load_ma,
                source_ohm: Some(2.0),
                ..RailBend::default()
            },
        )
    };
    let scene = grey(SMALL);
    let mut sim = build(load(150.0), SMALL);
    render(&mut sim, &scene, 2);
    let carried_v = camera.power.rail_v + sim.power_state().offset_v();
    assert!(
        carried_v < camera.power.rail_v - 0.25,
        "the capacitor sits at {carried_v} V"
    );
    assert_eq!(sim.set_preset(load(20.0)).unwrap(), KEPT);
    sim.frame(&scene, 2);
    assert_eq!(
        sim.rail().start_v(0).to_bits(),
        carried_v.to_bits(),
        "the first line after the change starts from the old rail"
    );
    let settled_v = camera.power.rail_v - 2.0 * 0.02;
    let last = sim.rail().lines() - 1;
    assert!((sim.rail().end_v(last) - settled_v).abs() < 1e-6);

    // Removing the bend leaves the rail at the nominal voltage and the capacitor with it.
    let mut plain = load(0.0);
    plain.bend.rail = None;
    assert_eq!(sim.set_preset(plain).unwrap(), KEPT);
    sim.frame(&scene, 3);
    assert!(sim.rail().is_nominal());
    assert_eq!(sim.power_state(), &power::State::default());
}

#[test]
fn a_spike_running_at_the_end_of_a_frame_runs_on_into_the_next() {
    let bend = RailBend {
        spike_ma: 200.0,
        spike_hz: 20.0,
        spike_ms: 20.0,
        ..RailBend::default()
    };
    let preset = railed(Mode::Still, bend);
    let camera = camera();
    let (params, layout) = rail_params(&preset, (2272, 1704));
    let mut rail = Rail::new(&layout, camera.power.rail_v);
    let mut state = power::State::default();
    let bus = bus::State::default();
    let plateau_v = camera.power.rail_v - params.source_ohm * 0.2;
    let mut carried = 0;
    for index in 0..20 {
        let left_s = state.spike_left_s();
        power::track(Some(&params), &mut state, &bus, index, &mut rail);
        if left_s > 2.0 * params.line_period_s {
            carried += 1;
            assert!(
                (rail.end_v(0) - plateau_v).abs() < 1e-6,
                "frame {index}: a spike with {left_s} s to run left line 0 at {} V",
                rail.end_v(0)
            );
        }
    }
    assert!(
        carried > 0,
        "no spike crossed a frame boundary in twenty frames"
    );
}

#[test]
fn the_ripple_reaches_the_rail_through_the_rc_filter_and_runs_on() {
    let camera = camera();
    // tau = 20 ohm * 47 uF = 0.94 ms against a 300 Hz ripple: omega tau = 1.77.
    let bend = RailBend {
        ripple_mv: 100.0,
        ripple_hz: 300.0,
        source_ohm: Some(20.0),
        ..RailBend::default()
    };
    let preset = railed(Mode::Still, bend);
    let (params, layout) = rail_params(&preset, (2272, 1704));
    let mut rail = Rail::new(&layout, camera.power.rail_v);
    let mut state = power::State::default();
    let bus = bus::State::default();
    power::track(Some(&params), &mut state, &bus, 0, &mut rail);
    let omega_tau = std::f64::consts::TAU * 300.0 * params.tau_s();
    let amplitude_v = 0.1 / omega_tau.hypot(1.0);
    let settled = &rail.lines_v()[200..];
    let high = settled.iter().map(|v| v[0]).fold(f64::MIN, f64::max) - camera.power.rail_v;
    let low = settled.iter().map(|v| v[0]).fold(f64::MAX, f64::min) - camera.power.rail_v;
    assert!(
        (high - amplitude_v).abs() < 0.01 * amplitude_v
            && (low + amplitude_v).abs() < 0.01 * amplitude_v,
        "the rail swings from {low} V to {high} V, the RC passes {amplitude_v} V"
    );
    let phase = (300.0 * params.frame_period_s).fract();
    assert!((state.ripple_phase() - phase).abs() < 1e-9);
}

#[test]
fn a_dip_covers_the_same_share_of_the_frame_at_every_resolution() {
    let camera = camera();
    let bend = RailBend {
        spike_ma: 300.0,
        spike_hz: 15.0,
        spike_ms: 4.0,
        source_ohm: Some(2.0),
        ..RailBend::default()
    };
    let preset = railed(Mode::Still, bend);
    let share = |size: (u32, u32)| {
        let (params, layout) = rail_params(&preset, size);
        let mut rail = Rail::new(&layout, camera.power.rail_v);
        let mut state = power::State::default();
        power::track(
            Some(&params),
            &mut state,
            &bus::State::default(),
            0,
            &mut rail,
        );
        let low = rail
            .lines_v()
            .iter()
            .filter(|v| v[0] < camera.power.rail_v - 0.3)
            .count();
        (
            low as f64 / f64::from(rail.lines()),
            1.0 / f64::from(rail.lines()),
        )
    };
    let (native, native_line) = share((2272, 1704));
    let (small, small_line) = share((640, 480));
    assert!(native > 0.0, "no spike in the frame");
    assert!(
        (native - small).abs() <= 2.0 * (native_line + small_line),
        "the dips cover {native} of the native frame and {small} of the 640 wide one"
    );
}
