//! Electrical behaviour of the bend circuit on the ADC data bus.

#![cfg(feature = "serde")]

use vcb_core::adc::{self, Codes};
use vcb_core::afe::{self, Analog};
use vcb_core::bus::{self, Function, PotNode, Target};
use vcb_core::noise;
use vcb_core::preset::BusBend;
use vcb_core::readout::{self, Direction, Layout, Stream};
use vcb_core::sensor::{self, Charge};
use vcb_core::{Camera, Preset, Scene, Timing};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The A520 with identical line drivers, so contests have exact expected values.
fn matched() -> Camera {
    let mut camera = camera();
    camera.adc.driver_r_spread = 0.0;
    camera
}

fn preset(sources: &[u8], target: Target, function: Function) -> Preset {
    let mut preset = Preset::new("a520");
    preset.seed = 1;
    preset.bend.bus = Some(BusBend {
        source_pins: sources.to_vec(),
        target,
        function,
        cap_uf: 1.0,
        pot_kohm: 0.5,
        pot_node: PotNode::F,
        series_ohm: 0.0,
        common_bus: false,
    });
    preset
}

fn bus_of(preset: &mut Preset) -> &mut BusBend {
    preset.bend.bus.as_mut().unwrap()
}

fn timing(camera: &Camera, preset: &Preset, width: u32, height: u32) -> Timing {
    Timing::derive(camera, preset, width, height).unwrap()
}

fn params(camera: &Camera, preset: &Preset, timing: &Timing) -> bus::Params {
    bus::Params::resolve(camera, preset, timing)
        .unwrap()
        .expect("the preset has a bus bend")
}

/// Duration of one circuit sub-step.
#[cfg(feature = "test-util")]
fn dt(timing: &Timing) -> f64 {
    timing.pixel_period_s / f64::from(timing.oversample)
}

/// Returns the word with the given lines high.
fn word(lines: &[u8]) -> u16 {
    lines.iter().fold(0, |word, &line| word | (1 << line))
}

fn mask(preset: &Preset) -> u16 {
    let bus = preset.bend.bus.as_ref().unwrap();
    let target = match bus.target {
        Target::Pin(pin) => 1 << pin,
        Target::Gnd => 0,
    };
    word(&bus.source_pins) | target
}

/// Codes of a frame filled with hashed 12-bit words.
fn random_codes(timing: &Timing, salt: &str) -> Codes {
    let mut codes = Codes::new(Layout::from_timing(timing, Direction::TopDown));
    for (i, code) in codes.words_mut().iter_mut().enumerate() {
        *code = (noise::hash_u32(i as u32, 0, noise::salt(salt), 7) & 0x0fff) as u16;
    }
    codes
}

/// Runs the circuit with `code` held on the drivers and returns the last probe.
#[cfg(feature = "test-util")]
fn settle(params: &bus::Params, code: u16, pixels: usize) -> bus::Probe {
    let mut probes = Vec::new();
    bus::trace(
        params,
        &mut bus::State::default(),
        &vec![code; pixels],
        &mut probes,
    );
    *probes.last().unwrap()
}

#[cfg(feature = "test-util")]
#[test]
fn equal_drivers_shorted_together_meet_at_half_rail() {
    let camera = matched();
    let mut preset = preset(&[9, 8], Target::Gnd, Function::Invert);
    bus_of(&mut preset).cap_uf = 0.001;
    let t = timing(&camera, &preset, 64, 48);
    let probe = settle(&params(&camera, &preset, &t), word(&[9]), 2000);
    let half = camera.adc.v_oh / 2.0;
    assert!((probe.s_v - half).abs() < 1e-3, "S at {} V", probe.s_v);
    assert!((probe.pin_v[9] - probe.s_v).abs() < 1e-12);
    assert!((probe.pin_v[8] - probe.s_v).abs() < 1e-12);
}

#[cfg(feature = "test-util")]
#[test]
fn the_stronger_driver_wins_the_target_pin() {
    // F settles to ground through the pot, so the inverter drives T high against the
    // target pin's driver, which converts a low bit.
    for (gate_ohm, driver_ohm) in [(90.0, 150.0), (150.0, 90.0)] {
        let mut camera = matched();
        camera.logic.hc_r_out_ohm = gate_ohm;
        camera.adc.driver_r_ohm = driver_ohm;
        let mut preset = preset(&[9, 8, 7], Target::Pin(10), Function::Invert);
        bus_of(&mut preset).cap_uf = 0.001;
        let t = timing(&camera, &preset, 64, 48);
        let probe = settle(&params(&camera, &preset, &t), 0, 500);
        assert!(probe.gate_high);
        let above = probe.t_v > camera.adc.dsp_threshold_v;
        assert_eq!(
            above,
            gate_ohm < driver_ohm,
            "gate {gate_ohm} ohm against driver {driver_ohm} ohm leaves T at {} V",
            probe.t_v
        );
    }
}

#[test]
fn a_grounded_common_bus_reads_zero_whatever_the_adc_says() {
    let camera = camera();
    let mut preset = preset(&[11, 9, 7, 5, 3], Target::Gnd, Function::Bypass);
    bus_of(&mut preset).common_bus = true;
    let t = timing(&camera, &preset, 64, 48);
    let params = params(&camera, &preset, &t);
    let before = random_codes(&t, "grounded");
    let mut after = before.clone();
    bus::bend(Some(&params), &mut bus::State::default(), 0, &mut after);
    let sources = word(&[11, 9, 7, 5, 3]);
    for (&b, &a) in before.words().iter().zip(after.words()) {
        assert_eq!(a & sources, 0, "{b:#06x} latched as {a:#06x}");
        assert_eq!(a & !sources, b & !sources);
    }
}

#[cfg(feature = "test-util")]
#[test]
fn a_patch_resistor_puts_the_pin_between_its_driver_and_the_bus() {
    let camera = matched();
    let mut preset = preset(&[9, 8], Target::Gnd, Function::Invert);
    bus_of(&mut preset).cap_uf = 0.001;
    bus_of(&mut preset).series_ohm = 100.0;
    let t = timing(&camera, &preset, 64, 48);
    let probe = settle(&params(&camera, &preset, &t), word(&[9]), 2000);
    let (s, high, low) = (probe.s_v, probe.pin_v[9], probe.pin_v[8]);
    let v_oh = camera.adc.v_oh;
    assert!(s < high && high < v_oh, "D9 at {high} V, S at {s} V");
    assert!(0.0 < low && low < s, "D8 at {low} V, S at {s} V");
    // One current flows through the driver and the patch resistor of each line.
    let ratio = (v_oh - high) / (high - s);
    assert!((ratio - camera.adc.driver_r_ohm / 100.0).abs() < 1e-6);
}

#[cfg(feature = "test-util")]
#[test]
fn a_pin_in_both_banks_joins_source_and_target() {
    let camera = matched();
    let preset = preset(&[10, 9, 8], Target::Pin(10), Function::Divide);
    let t = timing(&camera, &preset, 64, 48);
    let mut probes = Vec::new();
    bus::trace(
        &params(&camera, &preset, &t),
        &mut bus::State::default(),
        &[0, word(&[9, 8]), 0, word(&[10])],
        &mut probes,
    );
    assert!(probes.iter().all(|p| (p.s_v - p.t_v).abs() < 1e-12));
}

/// Steps every source line high after a settled low and measures node F.
///
/// Returns the step of S and F in the first sub-step and the time F takes to fall to
/// 1 / e of its step.
#[cfg(feature = "test-util")]
fn step_response(camera: &Camera, preset: &Preset) -> (f64, f64, f64) {
    let t = timing(camera, preset, 64, 48);
    let params = params(camera, preset, &t);
    let high = word(&preset.bend.bus.as_ref().unwrap().source_pins);
    let mut codes = vec![0; 200];
    codes.extend(std::iter::repeat_n(high, 4000));
    let mut probes = Vec::new();
    bus::trace(&params, &mut bus::State::default(), &codes, &mut probes);
    let at_step = 200 * t.oversample as usize;
    let (before, first) = (probes[at_step - 1], probes[at_step]);
    let df = first.f_v - before.f_v;
    let decayed = probes[at_step..]
        .iter()
        .position(|p| p.f_v <= df / std::f64::consts::E)
        .expect("F decays within the trace");
    (first.s_v - before.s_v, df, decayed as f64 * dt(&t))
}

#[cfg(feature = "test-util")]
#[test]
fn the_capacitor_passes_a_step_and_decays_with_the_loop_time_constant() {
    let camera = matched();
    let driver = camera.adc.driver_r_ohm;
    let pot = 1000.0;

    // Gate inputs draw no current: only the pot loads F.
    let sources: Vec<u8> = (2..=11).collect();
    let mut open = preset(&sources, Target::Gnd, Function::Invert);
    bus_of(&mut open).pot_kohm = pot / 1000.0;
    let source_ohm = driver / sources.len() as f64;
    let open_tau = 1e-6 * (source_ohm + pot);

    // In bypass the target pin's driver loads F in parallel with the pot.
    let sources: Vec<u8> = (2..=9).collect();
    let mut loaded = preset(&sources, Target::Pin(10), Function::Bypass);
    bus_of(&mut loaded).pot_kohm = pot / 1000.0;
    let source_ohm = driver / sources.len() as f64;
    let loaded_tau = 1e-6 * (source_ohm + pot * driver / (pot + driver));

    for (preset, tau) in [(open, open_tau), (loaded, loaded_tau)] {
        let (ds, df, measured) = step_response(&camera, &preset);
        assert!(ds > 2.5, "S stepped by only {ds} V");
        assert!((df - ds).abs() < 0.01 * ds, "F stepped {df} V for S {ds} V");
        assert!(
            (measured / tau - 1.0).abs() < 0.1,
            "time constant {measured} s, expected {tau} s"
        );
    }
}

#[cfg(feature = "test-util")]
#[test]
fn the_inverter_follows_the_complement_of_its_input_after_its_delay() {
    let camera = camera();
    let preset = preset(&[9], Target::Pin(10), Function::Invert);
    let t = timing(&camera, &preset, 64, 48);
    let params = params(&camera, &preset, &t);
    let v_oh = camera.adc.v_oh;
    let threshold = camera.logic.hc_threshold_v;
    let inside_band = threshold - camera.logic.hc_hysteresis_v / 4.0;
    // Low, high, a dip into the hysteresis band, low, high.
    let segments = [
        (0.0, 30),
        (v_oh, 30),
        (inside_band, 30),
        (0.0, 30),
        (v_oh, 30),
    ];
    let input: Vec<f64> = segments
        .iter()
        .flat_map(|&(volts, n)| std::iter::repeat_n(volts, n))
        .collect();
    let mut out = Vec::new();
    bus::drive_gate(&params, &mut bus::State::default(), &input, &mut out);

    // The input level with hysteresis: the dip into the band leaves it high.
    let (rise, fall) = (
        threshold + camera.logic.hc_hysteresis_v / 2.0,
        threshold - camera.logic.hc_hysteresis_v / 2.0,
    );
    let mut high = false;
    let levels: Vec<bool> = input
        .iter()
        .map(|&volts| {
            high = (high || volts > rise) && volts >= fall;
            high
        })
        .collect();
    let first_turn = out.iter().position(|&high| !high).unwrap();
    let delay = first_turn - 30;
    let expected_s =
        camera.logic.hc_prop_delay_ns * 1e-9 * t.pixel_period_s / t.native_pixel_period_s;
    assert!(
        (delay as f64 * dt(&t) - expected_s).abs() < dt(&t),
        "output turned {delay} sub-steps after the input, expected {expected_s} s"
    );
    assert!(out[..first_turn].iter().all(|&high| high));
    for (n, &high) in out.iter().enumerate().skip(delay) {
        assert_eq!(high, !levels[n - delay], "sub-step {n}");
    }
}

/// Counts the rising edges of a square wave on F and the toggles of `Q` it causes.
#[cfg(feature = "test-util")]
fn divide(
    params: &bus::Params,
    high: usize,
    low: usize,
    cycles: usize,
    v_oh: f64,
) -> (usize, usize) {
    let mut input = Vec::new();
    for _ in 0..cycles {
        input.extend(std::iter::repeat_n(0.0, low));
        input.extend(std::iter::repeat_n(v_oh, high));
    }
    // Room for the last toggle to complete.
    input.extend(std::iter::repeat_n(0.0, 64));
    let mut out = Vec::new();
    bus::drive_gate(params, &mut bus::State::default(), &input, &mut out);
    let toggles = out.windows(2).filter(|pair| pair[0] != pair[1]).count();
    (cycles, toggles)
}

#[cfg(feature = "test-util")]
#[test]
fn the_divider_toggles_on_slow_edges_and_skips_pixel_rate_edges() {
    let camera = camera();
    let preset = preset(&[9], Target::Pin(10), Function::Divide);
    for (width, height) in [(64, 48), (2272, 1704)] {
        let t = timing(&camera, &preset, width, height);
        let params = params(&camera, &preset, &t);
        let os = t.oversample as usize;
        let v_oh = camera.adc.v_oh;

        let period = 10 * os;
        let (edges, toggles) = divide(&params, period / 2, period - period / 2, 40, v_oh);
        assert_eq!(
            toggles, edges,
            "at a tenth of the pixel rate ({width} wide)"
        );

        let (edges, toggles) = divide(&params, os / 2, os - os / 2, 400, v_oh);
        assert!(
            toggles * 4 < edges,
            "{toggles} toggles on {edges} edges at the pixel rate ({width} wide)"
        );
    }
}

#[test]
fn without_a_bend_the_codes_pass_unchanged() {
    let camera = camera();
    let preset = Preset::new("a520");
    let t = timing(&camera, &preset, 64, 48);
    assert!(
        bus::Params::resolve(&camera, &preset, &t)
            .unwrap()
            .is_none()
    );
    let before = random_codes(&t, "none");
    let mut after = before.clone();
    bus::bend(None, &mut bus::State::default(), 0, &mut after);
    assert_eq!(after, before);
}

#[test]
fn a_bend_changes_only_the_lines_it_touches() {
    let camera = camera();
    for function in [Function::Bypass, Function::Invert, Function::Divide] {
        for series_ohm in [0.0, 47.0] {
            let mut preset = preset(&[9, 6, 3], Target::Pin(5), function);
            bus_of(&mut preset).series_ohm = series_ohm;
            let t = timing(&camera, &preset, 64, 48);
            let params = params(&camera, &preset, &t);
            let before = random_codes(&t, "touch");
            let mut after = before.clone();
            bus::bend(Some(&params), &mut bus::State::default(), 0, &mut after);
            let touched = mask(&preset);
            let mut changed = 0u16;
            for (&b, &a) in before.words().iter().zip(after.words()) {
                changed |= a ^ b;
            }
            assert_eq!(changed & !touched, 0, "{function:?}, {series_ohm} ohm");
            assert_ne!(changed, 0, "{function:?}, {series_ohm} ohm changed nothing");
        }
    }
}

#[test]
fn pins_outside_the_selector_banks_are_rejected() {
    let camera = camera();
    let t = timing(&camera, &Preset::new("a520"), 64, 48);
    let cases = [
        (vec![1, 9], Target::Pin(10)),
        (vec![9, 12], Target::Pin(10)),
        (vec![9, 8, 9], Target::Pin(10)),
        (vec![], Target::Pin(10)),
        (vec![9], Target::Pin(3)),
        (vec![9], Target::Pin(12)),
    ];
    for (sources, target) in cases {
        let preset = preset(&sources, target, Function::Bypass);
        let error = bus::Params::resolve(&camera, &preset, &t).unwrap_err();
        assert!(error.is_config(), "{sources:?} -> {target}: {error}");
    }
    let edges = preset(&[11, 2], Target::Pin(4), Function::Divide);
    assert_eq!(params(&camera, &edges, &t).source_pins(), [11, 2]);
}

#[test]
fn identical_codes_and_state_give_identical_words() {
    let camera = camera();
    for function in [Function::Bypass, Function::Invert, Function::Divide] {
        let preset = preset(&[9, 8, 7], Target::Pin(10), function);
        let t = timing(&camera, &preset, 64, 48);
        let params = params(&camera, &preset, &t);
        let codes = random_codes(&t, "determinism");
        let mut state = bus::State::default();
        bus::bend(Some(&params), &mut state, 0, &mut codes.clone());

        let (mut first, mut second) = (codes.clone(), codes.clone());
        let (mut a, mut b) = (state.clone(), state.clone());
        bus::bend(Some(&params), &mut a, 1, &mut first);
        bus::bend(Some(&params), &mut b, 1, &mut second);
        assert_eq!(first, second, "{function:?}");
        assert_eq!(a, b, "{function:?}");
    }
}

#[test]
fn a_frame_rendered_twice_from_reset_is_identical() {
    let camera = camera();
    let preset = preset(&[9, 8, 7], Target::Pin(10), Function::Divide);
    let t = timing(&camera, &preset, 64, 48);
    let params = params(&camera, &preset, &t);
    let codes = random_codes(&t, "reset");
    let render = |state: &mut bus::State, index| {
        let mut out = codes.clone();
        bus::bend(Some(&params), state, index, &mut out);
        out
    };
    let mut state = bus::State::default();
    let first = render(&mut state, 0);
    let _ = render(&mut state, 1);
    let mut state = bus::State::default();
    assert_eq!(render(&mut state, 0), first);
}

#[cfg(feature = "test-util")]
#[test]
fn a_frame_differs_after_its_predecessor_only_through_the_capacitor() {
    let camera = camera();
    // Bypass has no gate, so the capacitor is the circuit's only memory. Blanking
    // charges it to what the black code drives: D9 to D7 low, which is its power-on
    // voltage of 0 V, or D6 to D4 high, which is not.
    for (sources, remembers) in [([9, 8, 7], false), ([6, 5, 4], true)] {
        let preset = preset(&sources, Target::Pin(10), Function::Bypass);
        let t = timing(&camera, &preset, 64, 48);
        let params = params(&camera, &preset, &t);
        let codes = random_codes(&t, "memory");

        let mut state = bus::State::default();
        bus::bend(Some(&params), &mut state, 0, &mut codes.clone());
        let carried = state.cap_v() - bus::State::default().cap_v();
        let mut after_first = codes.clone();
        bus::bend(Some(&params), &mut state, 1, &mut after_first);
        let mut after_reset = codes.clone();
        bus::bend(
            Some(&params),
            &mut bus::State::default(),
            1,
            &mut after_reset,
        );

        assert_eq!(
            carried.abs() > 1e-3,
            remembers,
            "{sources:?} carried {carried} V"
        );
        assert_eq!(after_first != after_reset, remembers, "{sources:?}");
    }
}

/// Runs a horizontal grey ramp through the sensor, readout, AFE and ADC.
///
/// The ramp is set in sensor RGB, equal on every photosite, so the Bayer mosaic does
/// not alternate the codes from one sample to the next; it spans the black level to
/// about 1.35 times reference white along each line.
fn ramp_codes(camera: &Camera, preset: &Preset, width: u32, height: u32) -> (Timing, Codes) {
    let t = timing(camera, preset, width, height);
    let mut scene = Scene::new(t.sensor_width, t.sensor_height);
    for plane in scene.planes_mut() {
        for row in plane.rows_mut() {
            for (x, value) in row.iter_mut().enumerate() {
                *value = 1.35 * x as f32 / (t.sensor_width - 1) as f32;
            }
        }
    }
    let mut charge = Charge::new(t.sensor_width, t.sensor_height);
    let sensor = sensor::Params::resolve(camera, preset, &t).unwrap();
    sensor::expose(
        &sensor,
        &mut sensor::State::default(),
        &scene,
        0,
        &mut charge,
    );
    let readout = readout::Params::resolve(camera, preset, &t).unwrap();
    let mut stream = Stream::new(readout.layout.clone());
    readout::read(
        &readout,
        &mut readout::State::default(),
        &charge,
        0,
        &mut stream,
    );
    let afe = afe::Params::resolve(camera, preset, &t).unwrap();
    let mut analog = Analog::new(readout.layout.clone());
    afe::process(&afe, &mut afe::State::default(), &stream, 0, &mut analog);
    let mut codes = Codes::new(readout.layout.clone());
    adc::convert(
        &adc::Params::resolve(camera, preset, &t).unwrap(),
        &analog,
        &mut codes,
    );
    (t, codes)
}

/// Number of source lines high in `code`: the source mix with matched drivers.
fn mix(code: u16, sources: u16) -> u32 {
    (code & sources).count_ones()
}

#[test]
fn bypass_flips_the_target_mostly_right_after_source_edges() {
    let camera = matched();
    let mut preset = preset(&[9, 8, 7], Target::Pin(10), Function::Bypass);
    let (width, height) = (256, 16);
    let t = timing(&camera, &preset, width, height);
    // A time constant of three pixel periods: the high-pass forgets an edge long before
    // the next one arrives.
    let driver = camera.adc.driver_r_ohm;
    let pot = bus_of(&mut preset).pot_kohm * 1e3;
    let loop_ohm = driver / 3.0 + pot * driver / (pot + driver);
    bus_of(&mut preset).cap_uf = 3.0 * t.pixel_period_s / loop_ohm * 1e6;
    let (t, before) = ramp_codes(&camera, &preset, width, height);
    let mut after = before.clone();
    bus::bend(
        Some(&params(&camera, &preset, &t)),
        &mut bus::State::default(),
        0,
        &mut after,
    );

    let sources = word(&[9, 8, 7]);
    let line_len = t.ob_cols as usize + t.cols as usize;
    let (mut near, mut changed) = (0, 0);
    for (b, a) in before
        .words()
        .chunks(line_len)
        .zip(after.words().chunks(line_len))
    {
        let mut previous = mix(camera.adc.black_level, sources);
        let mut since_edge = usize::MAX;
        for (&code, &latched) in b.iter().zip(a) {
            let current = mix(code, sources);
            since_edge = if current == previous {
                since_edge.saturating_add(1)
            } else {
                0
            };
            previous = current;
            if (code ^ latched) & (1 << 10) != 0 {
                changed += 1;
                near += usize::from(since_edge < 9);
            }
        }
    }
    assert!(
        changed >= height as usize,
        "only {changed} target bits changed"
    );
    assert!(
        near * 4 > changed * 3,
        "{near} of {changed} changed target bits lie within three time constants of an edge"
    );
}

#[test]
fn invert_drives_the_complement_of_the_high_passed_source_mix() {
    let camera = matched();
    let preset = preset(&[9, 8, 7], Target::Pin(10), Function::Invert);
    let (t, before) = ramp_codes(&camera, &preset, 128, 32);
    let mut after = before.clone();
    bus::bend(
        Some(&params(&camera, &preset, &t)),
        &mut bus::State::default(),
        0,
        &mut after,
    );

    // The ideal circuit: the unloaded source mix drives the capacitor and the pot in
    // series through the parallel source drivers; F is the pot's share of the
    // capacitor current, sampled at the latch instant.
    let bus = preset.bend.bus.as_ref().unwrap();
    let sources = word(&bus.source_pins);
    let count = bus.source_pins.len() as f64;
    let source_ohm = camera.adc.driver_r_ohm / count;
    let pot = bus.pot_kohm * 1e3;
    let tau = bus.cap_uf * 1e-6 * (source_ohm + pot);
    let pixel = t.pixel_period_s;
    let latch = camera.adc.latch_phase * pixel;
    let level = |code: u16| camera.adc.v_oh * f64::from(mix(code, sources)) / count;
    let advance = |cap: f64, s: f64, seconds: f64| s + (cap - s) * (-seconds / tau).exp();
    let dark = level(camera.adc.black_level);

    let line_len = t.ob_cols as usize + t.cols as usize;
    let (mut agree, mut total, mut expected_high) = (0, 0, 0);
    let mut cap = 0.0;
    for (b, a) in before
        .words()
        .chunks(line_len)
        .zip(after.words().chunks(line_len))
    {
        for (i, (&code, &latched)) in b.iter().zip(a).enumerate() {
            let s = level(code);
            let f = (s - advance(cap, s, latch)) * pot / (source_ohm + pot);
            cap = advance(cap, s, pixel);
            if i >= t.ob_cols as usize {
                let expected = f <= camera.logic.hc_threshold_v;
                total += 1;
                expected_high += usize::from(expected);
                agree += usize::from(expected == ((latched >> 10) & 1 == 1));
            }
        }
        cap = advance(cap, dark, f64::from(t.hblank_px) * pixel);
    }
    assert!(
        expected_high * 10 > total && expected_high * 10 < total * 9,
        "the ideal inverter is high on {expected_high} of {total} pixels"
    );
    assert!(
        agree * 10 > total * 9,
        "the target agrees with the ideal inverter on {agree} of {total} pixels"
    );
}

#[test]
fn divide_holds_runs_longer_than_the_source_edge_spacing() {
    let camera = matched();
    let preset = preset(&[9, 8, 7], Target::Pin(10), Function::Divide);
    let (t, before) = ramp_codes(&camera, &preset, 256, 16);
    let mut after = before.clone();
    bus::bend(
        Some(&params(&camera, &preset, &t)),
        &mut bus::State::default(),
        0,
        &mut after,
    );

    let sources = word(&[9, 8, 7]);
    let (ob, line_len) = (t.ob_cols as usize, t.ob_cols as usize + t.cols as usize);
    let (mut edges, mut runs, mut samples) = (0, 0, 0);
    for (b, a) in before
        .words()
        .chunks(line_len)
        .zip(after.words().chunks(line_len))
    {
        let (b, a) = (&b[ob..], &a[ob..]);
        samples += a.len();
        edges += b
            .windows(2)
            .filter(|w| mix(w[0], sources) != mix(w[1], sources))
            .count();
        runs += 1 + a
            .windows(2)
            .filter(|w| (w[0] ^ w[1]) & (1 << 10) != 0)
            .count();
    }
    let run_length = samples as f64 / runs as f64;
    let edge_spacing = samples as f64 / edges.max(1) as f64;
    assert!(
        run_length > edge_spacing,
        "runs of {run_length:.1} pixels, source edges every {edge_spacing:.1} pixels"
    );
}

#[cfg(feature = "test-util")]
#[test]
fn with_the_pot_on_s_the_clamp_diodes_restore_f_between_the_rails() {
    // With the pot on S, F has only the gate inputs' leakage to half the supply. A short
    // pulse train lets the leakage centre F on half the supply, so its pulses reach past
    // the supply rail, where the diodes clamp them and recharge the capacitor. 1 nF
    // makes the leakage time constant 1.65 ms, so the trace sees it settle.
    let camera = matched();
    let mut preset = preset(&[9, 8, 7], Target::Pin(10), Function::Invert);
    bus_of(&mut preset).pot_node = PotNode::S;
    bus_of(&mut preset).cap_uf = 0.001;
    let t = timing(&camera, &preset, 64, 48);
    let params = params(&camera, &preset, &t);
    let high = word(&[9, 8, 7]);
    let pixels: Vec<u16> = (0..8000)
        .map(|n| if n % 20 < 2 { high } else { 0 })
        .collect();
    let mut probes = Vec::new();
    bus::trace(&params, &mut bus::State::default(), &pixels, &mut probes);

    let (low, highest) = probes.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        (lo.min(p.f_v), hi.max(p.f_v))
    });
    assert!(
        low >= -0.6 && highest <= 3.9,
        "F spans {low} to {highest} V"
    );
    assert!(
        highest > camera.adc.v_oh,
        "F never rose past the supply ({highest} V)"
    );
    assert!(
        probes.iter().any(|p| p.clamp_a < -1e-6),
        "the supply-side diodes never conducted"
    );
    // At the end of every pixel the inverter shows the complement of the source edge.
    let os = t.oversample as usize;
    for (n, (&code, pixel)) in pixels.iter().zip(probes.chunks(os)).enumerate() {
        let last = pixel[os - 1];
        assert_eq!(
            last.gate_high,
            code != high,
            "pixel {n}, F at {} V",
            last.f_v
        );
    }
}

#[cfg(feature = "test-util")]
#[test]
fn with_the_pot_on_f_no_clamp_diode_conducts_on_a_gradient() {
    // A 16-row frame at match-input resolution has milliseconds of blanking per line,
    // which returns the capacitor to its black-level charge before every line; F then
    // stays between the rails. On native-length lines the charge carries over and
    // every drop of the source mix below its running mean pulls F under ground.
    let camera = camera();
    for function in [Function::Bypass, Function::Invert, Function::Divide] {
        let preset = preset(&[9, 8, 7], Target::Pin(10), function);
        let (t, mut codes) = ramp_codes(&camera, &preset, 256, 16);
        let mut state = bus::State::default();
        bus::bend(
            Some(&params(&camera, &preset, &t)),
            &mut state,
            0,
            &mut codes,
        );
        assert_eq!(state.clamped_steps(), 0, "{function:?}");
    }
}
