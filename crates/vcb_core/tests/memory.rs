//! The frame-buffer SDRAM's address pins: folds in powers of two, marginal contacts,
//! stale content, power-on content, resolution scaling and the running camera's state;
//! the rows a processor brownout never stored; its data pins, stuck, bridged and
//! marginal; and its cells' retention on a hot board.

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::adc::Codes;
use vcb_core::isp::{self, Raw};
use vcb_core::memory::{self, DataPin, Decay, Params, State};
use vcb_core::power::Rail;
use vcb_core::preset::{
    AddressFault, DataFault, FaultKind, MemoryBend, Mode, Orientation, RailBend, Resolution,
};
use vcb_core::readout::{Direction, Layout};
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing, noise};

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A native-like small camera: a 48 by 32 raw (a pitch that is no power of two) in a
/// 2^12-word SDRAM of 4 column, 1 bank and 7 row bits, read in `direction`.
///
/// Pins a0 to a3 carry a column and a row bit, a4 to a6 only a row bit.
fn small(direction: Direction) -> Camera {
    let mut camera = camera();
    camera.sensor.native_width = 48;
    camera.sensor.native_height = 32;
    camera.still.direction = direction;
    camera.memory.column_bits = 4;
    camera.memory.bank_bits = 1;
    camera.memory.row_bits = 7;
    camera
}

/// A camera whose 64-word lines are one SDRAM row each: pins a3 to a5 carry only column bits.
fn wide() -> Camera {
    let mut camera = camera();
    camera.sensor.native_width = 64;
    camera.sensor.native_height = 24;
    camera.memory.column_bits = 6;
    camera.memory.bank_bits = 2;
    camera.memory.row_bits = 3;
    camera
}

fn solid(pin: u8, kind: FaultKind) -> AddressFault {
    AddressFault {
        pin,
        kind,
        with: None,
        on_frac: None,
        switch_hz: None,
    }
}

fn bridge(pin: u8, with: u8) -> AddressFault {
    AddressFault {
        with: Some(with),
        ..solid(pin, FaultKind::Bridge)
    }
}

fn marginal(fault: AddressFault, on_frac: f64, switch_hz: f64) -> AddressFault {
    AddressFault {
        on_frac: Some(on_frac),
        switch_hz: Some(switch_hz),
        ..fault
    }
}

fn preset(faults: Vec<AddressFault>) -> Preset {
    let mut preset = Preset::new("a520");
    preset.bend.memory = Some(MemoryBend {
        faults,
        ..MemoryBend::default()
    });
    preset
}

/// Resolves the memory stage at the camera's native resolution.
fn native(camera: &Camera, preset: &Preset) -> Params {
    let timing = Timing::derive(camera, preset, 64, 48).unwrap();
    memory::Params::resolve(camera, preset, &timing)
        .unwrap()
        .expect("the preset has a memory bend")
}

/// Resolves the memory stage at match-input resolution for a `width` by `height` input.
fn matched(camera: &Camera, preset: &Preset, width: u32, height: u32) -> Params {
    let mut preset = preset.clone();
    preset.readout.resolution = Resolution::MatchInput;
    let timing = Timing::derive(camera, &preset, width, height).unwrap();
    memory::Params::resolve(camera, &preset, &timing)
        .unwrap()
        .expect("the preset has a memory bend")
}

/// A raw whose every site holds a value naming its frame and its site.
///
/// Exact while `(frame + 1) * sites` stays below 2^24, an `f32`'s integer range.
fn distinct(params: &Params, frame: u32) -> Raw {
    let sites = params.pitch * params.rows;
    let mut raw = Raw::new(params.pitch, params.rows);
    for (site, value) in (0u32..).zip(raw.plane_mut().data_mut()) {
        *value = (u64::from(frame) * u64::from(sites) + u64::from(site) + 1) as f32;
    }
    raw
}

/// The frame and site a value of [`distinct`] names.
fn origin(params: &Params, value: f32) -> (u32, u32) {
    let sites = params.pitch * params.rows;
    let tag = value as u32 - 1;
    (tag / sites, tag % sites)
}

/// Runs one frame through the stage and returns what the processor reads back.
fn run(params: &Params, state: &mut State, index: u64, raw: &Raw) -> Raw {
    let mut out = raw.clone();
    memory::bend(Some(params), state, index, &mut out);
    out
}

/// When raw site `site` is written: its readout line, then its column.
fn written_at(params: &Params, site: u32) -> (u32, u32) {
    (params.line(site / params.pitch), site % params.pitch)
}

/// Bit `bit` as a mask.
fn bit(bit: u32) -> u64 {
    1 << bit
}

/// The linear address bits of pin `pin`'s column and row address on `camera` at native resolution.
fn pin_bits(camera: &Camera, pin: u8) -> [Option<u32>; 2] {
    let memory = &camera.memory;
    let pin = u32::from(pin);
    [
        (pin < u32::from(memory.column_bits)).then_some(pin),
        (pin < u32::from(memory.row_bits))
            .then_some(u32::from(memory.column_bits) + u32::from(memory.bank_bits) + pin),
    ]
}

#[test]
fn without_faults_the_frame_comes_back_unchanged() {
    for direction in [Direction::TopDown, Direction::BottomUp] {
        let camera = small(direction);
        let params = native(&camera, &preset(Vec::new()));
        let mut state = State::default();
        for frame in 0..3 {
            let raw = distinct(&params, frame);
            assert_eq!(
                run(&params, &mut state, u64::from(frame), &raw),
                raw,
                "{direction:?}, frame {frame}"
            );
        }
    }
}

#[test]
fn a_contact_closed_all_the_time_renders_as_the_solid_fault() {
    // A flaky contact on another pin runs in both, so its stream must not move either.
    let flaky = marginal(solid(9, FaultKind::Low), 0.4, 300.0);
    let solid_high = preset(vec![solid(3, FaultKind::High), flaky]);
    let closed_high = preset(vec![marginal(solid(3, FaultKind::High), 1.0, 300.0), flaky]);
    let scene = Scene::from_srgb8(
        96,
        72,
        &(0..96 * 72 * 3)
            .map(|i| (i * 37 % 251) as u8)
            .collect::<Vec<_>>(),
    );
    for mode in [Mode::Still, Mode::Movie] {
        let frames = |mut preset: Preset| -> Vec<Frame> {
            preset.readout.mode = mode;
            preset.readout.resolution = Resolution::MatchInput;
            let mut sim = Simulator::builder((camera(), preset))
                .input_size(96, 72)
                .build()
                .unwrap();
            (0..3)
                .map(|index| sim.frame(&scene, index).clone())
                .collect()
        };
        assert_eq!(
            frames(closed_high.clone()),
            frames(solid_high.clone()),
            "{mode}"
        );
    }
}

/// Checks that every site of a solid fault on one row pin reads the later-written of
/// the two sites that differ in the pin's bit, and that the other half of memory is
/// left with what it held.
fn check_row_pin_fold(direction: Direction, kind: FaultKind) {
    let camera = small(direction);
    let pin = 4;
    let [None, Some(row_bit)] = pin_bits(&camera, pin) else {
        panic!("a4 carries only a row bit");
    };
    let params = native(&camera, &preset(vec![solid(pin, kind)]));
    let mut state = State::default();
    let raw = distinct(&params, 0);
    let out = run(&params, &mut state, 0, &raw);
    let sites = params.pitch * params.rows;
    let mut folded = 0;
    for site in 0..sites {
        let address = u64::from(site);
        let twin = address ^ bit(row_bit);
        let in_frame = |a: u64| a < u64::from(sites);
        let later = if in_frame(twin) {
            let twin = twin as u32;
            if written_at(&params, twin) > written_at(&params, site) {
                twin
            } else {
                site
            }
        } else {
            site
        };
        let value = out.plane().data()[site as usize];
        assert_eq!(
            origin(&params, value),
            (0, later),
            "{direction:?} {kind}: site {site} reads the later-written of it and {twin}"
        );
        if in_frame(twin) {
            folded += 1;
            assert_eq!(value, out.plane().data()[twin as usize]);
        }
        // The half of memory the fault never addresses keeps its power-on content.
        let untouched = match kind {
            FaultKind::Low => address | bit(row_bit),
            _ => address & !bit(row_bit),
        };
        assert_eq!(
            state.word(untouched),
            Some(params.power_on.word(untouched)),
            "{direction:?} {kind}: word {untouched}"
        );
    }
    assert!(folded > sites / 3, "{folded} of {sites} sites fold");
}

#[test]
fn a_solid_stuck_row_pin_folds_both_halves_onto_the_later_written_one() {
    for direction in [Direction::TopDown, Direction::BottomUp] {
        for kind in [FaultKind::Low, FaultKind::High] {
            check_row_pin_fold(direction, kind);
        }
    }
}

#[test]
fn a_bridge_reads_the_or_of_its_two_pins_in_both_address_phases() {
    let camera = small(Direction::TopDown);
    let (p, q) = (1, 2);
    let (bits_p, bits_q) = (pin_bits(&camera, p), pin_bits(&camera, q));
    let params = native(&camera, &preset(vec![bridge(p, q)]));
    let raw = distinct(&params, 0);
    let out = run(&params, &mut State::default(), 0, &raw);
    // Both pins read the OR of the two bits in the column phase and in the row phase.
    let decode = |mut a: u64| {
        for phase in 0..2 {
            let mask = bit(bits_p[phase].unwrap()) | bit(bits_q[phase].unwrap());
            if a & mask != 0 {
                a |= mask;
            }
        }
        a
    };
    let sites = params.pitch * params.rows;
    let mut moved = 0;
    for site in 0..sites {
        let address = u64::from(site);
        assert_eq!(params.decode(address, 1), decode(address), "site {site}");
        // Of the sites decoding to the same word, the last one written wins.
        let last = (0..sites)
            .filter(|&other| decode(u64::from(other)) == decode(address))
            .max_by_key(|&other| written_at(&params, other))
            .unwrap();
        let value = out.plane().data()[site as usize];
        assert_eq!(origin(&params, value), (0, last), "site {site}");
        if last != site {
            moved += 1;
        }
    }
    // Sites with exactly one of each pair's bits set alias onto the word with both set.
    assert!(
        moved > sites / 2,
        "{moved} of {sites} sites read another site"
    );
}

#[test]
fn column_pins_fold_and_interleave_within_the_line() {
    let camera = wide();
    for (fault, x_bits) in [
        (solid(4, FaultKind::Low), vec![4]),
        (solid(5, FaultKind::High), vec![5]),
        (bridge(3, 5), vec![3, 5]),
    ] {
        assert_eq!(pin_bits(&camera, fault.pin)[1], None, "{fault:?}");
        let params = native(&camera, &preset(vec![fault]));
        let raw = distinct(&params, 0);
        let out = run(&params, &mut State::default(), 0, &raw);
        let mask = x_bits.iter().fold(0, |m, &b| m | (1u32 << b));
        for (site, &value) in (0u32..).zip(out.plane().data()) {
            let (x, y) = (site % params.pitch, site / params.pitch);
            let (frame, source) = origin(&params, value);
            let (sx, sy) = (source % params.pitch, source / params.pitch);
            assert_eq!(
                (frame, sy),
                (0, y),
                "{fault:?}: ({x}, {y}) stays in its line"
            );
            assert_eq!(sx & !mask, x & !mask, "{fault:?}: ({x}, {y}) from {sx}");
            // The pin's sites alias onto the last-written column of their group.
            let group = (0..params.pitch)
                .filter(|&other| other & !mask == x & !mask)
                .filter(|&other| {
                    params.decode(u64::from(y * params.pitch + other), 1)
                        == params.decode(u64::from(site), 1)
                })
                .max()
                .unwrap();
            assert_eq!(sx, group, "{fault:?}: ({x}, {y})");
        }
        assert_ne!(out, raw, "{fault:?} folds the line");
    }
}

#[test]
fn a_pin_beyond_the_frame_changes_nothing_and_one_beyond_the_sdram_is_rejected() {
    let camera = camera();
    // a11 carries no column bit, and its row bit lies above every address of the raw.
    let params = matched(&camera, &preset(vec![solid(11, FaultKind::Low)]), 568, 426);
    let raw = distinct(&params, 0);
    assert_eq!(run(&params, &mut State::default(), 0, &raw), raw);

    let pins = camera.memory.column_bits.max(camera.memory.row_bits);
    for fault in [solid(pins, FaultKind::High), bridge(2, pins + 3)] {
        let preset = preset(vec![fault]);
        let timing = Timing::derive(&camera, &preset, 2272, 1704).unwrap();
        let error = memory::Params::resolve(&camera, &preset, &timing).unwrap_err();
        assert!(error.is_config(), "{error}");
        assert!(
            error.to_string().contains("bend.memory.faults[0]"),
            "{error}"
        );
    }
}

/// Frames of a marginal contact on the small camera: each frame's output and the
/// contact's write and read samples per readout line.
struct Run {
    params: Params,
    outputs: Vec<Raw>,
    contact: Vec<(Vec<bool>, Vec<bool>)>,
}

fn run_contact(fault: AddressFault, seed: u64, frames: u32) -> Run {
    let camera = small(Direction::TopDown);
    let mut preset = preset(vec![fault]);
    preset.seed = seed;
    let params = native(&camera, &preset);
    let mut state = State::default();
    let mut run = Run {
        params: params.clone(),
        outputs: Vec::new(),
        contact: Vec::new(),
    };
    for frame in 0..frames {
        let raw = distinct(&params, frame);
        run.outputs
            .push(self::run(&params, &mut state, u64::from(frame), &raw));
        let (write, read) = state.contact_lines(fault.pin).unwrap();
        run.contact.push((write.to_vec(), read.to_vec()));
    }
    run
}

#[test]
fn a_marginal_contact_reads_earlier_frames_where_its_write_went_astray() {
    // a4 stuck low: a site with the row bit set writes into its twin's word when the
    // contact is faulted; read with the contact clear, it finds its own word as the
    // frame before left it.
    let camera = small(Direction::TopDown);
    let [None, Some(row_bit)] = pin_bits(&camera, 4) else {
        panic!("a4 carries only a row bit");
    };
    let fault = marginal(solid(4, FaultKind::Low), 0.5, 300.0);
    let mut seen = 0;
    for seed in 0..40 {
        let run = run_contact(fault, seed, 4);
        let params = &run.params;
        for frame in 1..4 {
            let (write, read) = &run.contact[frame];
            let (earlier_write, _) = &run.contact[frame - 1];
            for line in 0..params.rows as usize {
                if !(write[line] && !read[line] && !earlier_write[line]) {
                    continue;
                }
                let y = params.line(line as u32);
                for x in 0..params.pitch {
                    let site = y * params.pitch + x;
                    if u64::from(site) & bit(row_bit) == 0 {
                        continue;
                    }
                    let value = run.outputs[frame].plane().data()[site as usize];
                    assert_eq!(
                        origin(params, value),
                        (frame as u32 - 1, site),
                        "seed {seed}, frame {frame}, line {line}: the frame before shows"
                    );
                    seen += 1;
                }
            }
        }
    }
    assert!(seen > 0, "no seed missed a write and caught the read");
}

#[test]
fn a_read_through_a_contact_finds_the_last_frame_beyond_the_buffer_or_the_power_on_content() {
    // a6 stuck high sends every address beyond the 1536-word raw buffer.
    let camera = small(Direction::TopDown);
    let [None, Some(row_bit)] = pin_bits(&camera, 6) else {
        panic!("a6 carries only a row bit");
    };
    let fault = marginal(solid(6, FaultKind::High), 0.5, 300.0);
    let (mut stale, mut power_on) = (0, 0);
    for seed in 0..40 {
        let run = run_contact(fault, seed, 4);
        let params = &run.params;
        assert!(
            params.span.end > 1 << row_bit,
            "the store reaches the faulted words"
        );
        for frame in 0..4 {
            let (write, read) = &run.contact[frame];
            for line in 0..params.rows as usize {
                if write[line] || !read[line] {
                    continue;
                }
                let y = params.line(line as u32);
                // Written in place this frame, read from beyond the buffer.
                let before = (0..frame)
                    .rev()
                    .find(|&earlier| run.contact[earlier].0[line]);
                for x in 0..params.pitch {
                    let site = y * params.pitch + x;
                    let away = u64::from(site) | bit(row_bit);
                    let value = run.outputs[frame].plane().data()[site as usize];
                    if let Some(earlier) = before {
                        assert_eq!(origin(params, value), (earlier as u32, site), "seed {seed}");
                        stale += 1;
                    } else {
                        assert_eq!(value, params.power_on.word(away), "seed {seed}");
                        power_on += 1;
                    }
                }
            }
        }
    }
    assert!(
        stale > 0 && power_on > 0,
        "stale {stale}, power-on {power_on}"
    );
}

#[test]
fn the_power_on_content_returns_after_a_reset() {
    let camera = small(Direction::TopDown);
    let fault = marginal(solid(6, FaultKind::High), 0.5, 300.0);
    let params = native(&camera, &preset(vec![fault, solid(1, FaultKind::Low)]));
    let raws: Vec<Raw> = (0..4).map(|frame| distinct(&params, frame)).collect();
    let mut state = State::default();
    let first: Vec<Raw> = (0..4)
        .map(|frame| run(&params, &mut state, frame, &raws[frame as usize]))
        .collect();
    let span = state.span();
    assert!(span.end > span.start);
    state = State::default();
    assert_eq!(state.span(), 0..0, "a reset forgets the content");
    let again: Vec<Raw> = (0..4)
        .map(|frame| run(&params, &mut state, frame, &raws[frame as usize]))
        .collect();
    assert_eq!(again, first, "the same frames replay from power-on");
    // Words no frame wrote still hold the power-on content.
    let unwritten = (span.start..span.end)
        .filter(|&a| state.word(a) == Some(params.power_on.word(a)))
        .count();
    assert!(unwritten > 0);
}

#[test]
fn a_contact_switches_at_its_rate_and_is_faulted_for_its_fraction_of_the_time() {
    let (on_frac, switch_hz) = (0.3, 1000.0);
    let fault = marginal(solid(4, FaultKind::Low), on_frac, switch_hz);
    let run = run_contact(fault, 7, 4000);
    let line_s = run.params.line_period_s;
    let (mut on, mut samples, mut releases, mut pairs) = (0usize, 0usize, 0usize, 0usize);
    for (write, _) in &run.contact {
        on += write.iter().filter(|&&s| s).count();
        samples += write.len();
        releases += write.windows(2).filter(|w| w[0] && !w[1]).count();
        pairs += write.len() - 1;
    }
    let measured_frac = on as f64 / samples as f64;
    assert!(
        (measured_frac - on_frac).abs() < 0.03,
        "faulted {measured_frac} of the time"
    );
    // One release per on-off cycle; sampling once per line misses a few short cycles.
    let rate = switch_hz / (on_frac * (1.0 - on_frac));
    let seen = (1.0 - (-rate * line_s).exp()) / (rate * line_s);
    let measured_hz = releases as f64 / (pairs as f64 * line_s);
    assert!(
        (measured_hz / (switch_hz * seen) - 1.0).abs() < 0.12,
        "{measured_hz} cycles per second for {switch_hz}"
    );
    // A contact read in one frame is written with in the next: one sample path.
    for pair in run.contact.windows(2) {
        assert_eq!(pair[1].0, pair[0].1);
    }
}

#[test]
fn folds_keep_their_fraction_of_the_frame_at_a_quarter_of_the_size() {
    let camera = camera();
    let full = |preset: &Preset| native(&camera, preset);
    let quarter = |preset: &Preset| matched(&camera, preset, 568, 426);

    // a4 stuck low: column bit 4 and row bit 15. Row 0 lies below the row bit, so each
    // of its sites reads a site one row weight further on, plus the column weight
    // where the column bit is clear.
    let low = preset(vec![solid(4, FaultKind::Low)]);
    let mut weights = Vec::new();
    for params in [full(&low), quarter(&low)] {
        let raw = distinct(&params, 0);
        let out = run(&params, &mut State::default(), 0, &raw);
        let offsets: Vec<u64> = (0..params.pitch)
            .map(|x| u64::from(origin(&params, out.plane().row(0)[x as usize]).1) - u64::from(x))
            .collect();
        let row_weight = *offsets.iter().min().unwrap();
        let column_weight = offsets.iter().max().unwrap() - row_weight;
        assert!(row_weight.is_power_of_two() && column_weight.is_power_of_two());
        let words = f64::from(params.pitch) * f64::from(params.rows);
        weights.push((
            column_weight as f64 / f64::from(params.pitch),
            row_weight as f64 / words,
        ));
    }
    assert_eq!(weights[0], weights[1], "column and row folds as fractions");

    // a10 stuck high: the top of the frame shows the bottom; the same share of it.
    let high = preset(vec![solid(10, FaultKind::High)]);
    let mut shares = Vec::new();
    for params in [full(&high), quarter(&high)] {
        let raw = distinct(&params, 0);
        let out = run(&params, &mut State::default(), 0, &raw);
        let moved = (0..params.rows)
            .filter(|&y| origin(&params, out.plane().row(y)[0]).1 / params.pitch != y)
            .count();
        shares.push(moved as f64 / f64::from(params.rows));
    }
    assert!(shares[0] > 0.4, "{shares:?}");
    assert!(
        (shares[0] - shares[1]).abs() < 1.5 / 426.0,
        "rows copied: {shares:?}"
    );

    // At a ratio that is no power of two each weight rounds to the nearest power of two.
    let all = preset(vec![solid(3, FaultKind::Low), solid(10, FaultKind::Low)]);
    let near = matched(&camera, &all, 640, 480);
    let scale_column = 640.0 / 2272.0;
    let scale_row = 640.0 * 480.0 / (2272.0 * 1704.0);
    for fault in &near.faults {
        for (phase, scale) in [(0, scale_column), (1, scale_row)] {
            if let (Some(native), Some(scaled)) =
                (pin_bits(&camera, fault.pin)[phase], fault.bits[phase])
            {
                let exact = f64::from(native) + f64::log2(scale);
                assert!(
                    (f64::from(scaled) - exact).abs() <= 0.5,
                    "a{} phase {phase}: bit {scaled} for {exact}",
                    fault.pin
                );
            }
        }
    }
}

/// A scene with structure in both directions.
fn scene(width: u32, height: u32) -> Scene {
    let rgb: Vec<u8> = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let v = ((x * 255 / width + y * 96 / height) % 256) as u8;
                [v, 255 - v, ((x ^ y) * 9 % 256) as u8]
            })
        })
        .collect();
    Scene::from_srgb8(width, height, &rgb)
}

fn tear() -> Vec<AddressFault> {
    vec![
        marginal(solid(9, FaultKind::Low), 0.3, 250.0),
        marginal(bridge(4, 6), 0.3, 400.0),
        solid(2, FaultKind::High),
    ]
}

#[test]
fn every_mode_replays_from_reset_byte_for_byte() {
    let scene = scene(160, 120);
    for mode in [Mode::Still, Mode::Movie] {
        let mut bent = preset(tear());
        bent.readout.mode = mode;
        bent.readout.resolution = Resolution::MatchInput;
        let mut clean = bent.clone();
        clean.bend.memory = None;
        let build = |preset: Preset| {
            Simulator::builder((camera(), preset))
                .input_size(160, 120)
                .build()
                .unwrap()
        };
        let mut sim = build(bent.clone());
        let first: Vec<Frame> = (0..5).map(|i| sim.frame(&scene, i).clone()).collect();
        sim.reset();
        let again: Vec<Frame> = (0..5).map(|i| sim.frame(&scene, i).clone()).collect();
        assert_eq!(again, first, "{mode}: from reset");
        let mut fresh = build(bent);
        let third: Vec<Frame> = (0..5).map(|i| fresh.frame(&scene, i).clone()).collect();
        assert_eq!(third, first, "{mode}: from a new simulator");
        let mut plain = build(clean);
        assert_ne!(plain.frame(&scene, 0), &first[0], "{mode}: the faults show");
    }
}

#[test]
fn adding_a_fault_keeps_the_sdram_and_the_next_frame_reads_its_content() {
    let scene = scene(64, 48);
    let mut empty = preset(Vec::new());
    empty.readout.resolution = Resolution::MatchInput;
    let mut sim = Simulator::builder((camera(), empty.clone()))
        .input_size(64, 48)
        .build()
        .unwrap();
    for index in 0..2 {
        sim.frame(&scene, index);
    }
    let before = sim.memory_state().clone();

    // a9 stuck low: a row bit only, which falls on bit 10 at 64 by 48.
    let mut faulted = empty;
    faulted.bend.memory = Some(MemoryBend {
        faults: vec![solid(9, FaultKind::Low)],
        ..MemoryBend::default()
    });
    assert_eq!(sim.set_preset(faulted.clone()).unwrap(), KEPT);
    assert_eq!(sim.memory_state(), &before, "the SDRAM keeps its content");
    sim.frame(&scene, 2);
    let after = sim.memory_state();
    let timing = sim.timing().clone();
    let params = memory::Params::resolve(&camera(), &faulted, &timing)
        .unwrap()
        .unwrap();
    let row_bit = u64::from(params.faults[0].bits[1].unwrap());
    let (mut carried, mut rewritten) = (0, 0);
    for address in 0..u64::from(params.pitch * params.rows) {
        let (old, new) = (before.word(address), after.word(address));
        if address & bit(row_bit as u32) != 0 {
            assert_eq!(
                new, old,
                "word {address} is never written through the fault"
            );
            carried += 1;
        } else if new != old {
            rewritten += 1;
        }
    }
    assert!(
        carried > 0 && rewritten > 0,
        "{carried} carried, {rewritten} rewritten"
    );
}

#[test]
fn a_new_marginal_fault_reads_the_frame_before_it_through_the_kept_store() {
    // The stage with the store of a frame rendered without faults, then a contact on
    // a4 stuck low: where it diverts a line's write and clears for its read, the frame
    // before shows.
    let camera = small(Direction::TopDown);
    let [None, Some(row_bit)] = pin_bits(&camera, 4) else {
        panic!("a4 carries only a row bit");
    };
    let plain = native(&camera, &preset(Vec::new()));
    let mut seen = 0;
    for seed in 0..40 {
        let mut state = State::default();
        let first = distinct(&plain, 0);
        run(&plain, &mut state, 0, &first);
        let mut faulted = preset(vec![marginal(solid(4, FaultKind::Low), 0.5, 300.0)]);
        faulted.seed = seed;
        let params = native(&camera, &faulted);
        let out = run(&params, &mut state, 1, &distinct(&params, 1));
        let (write, read) = state.contact_lines(4).unwrap();
        for line in 0..params.rows as usize {
            // Faulted at the write, clear at the read.
            let missed = write[line] && !read[line];
            if !missed {
                continue;
            }
            let y = params.line(line as u32);
            for x in 0..params.pitch {
                let site = y * params.pitch + x;
                if u64::from(site) & bit(row_bit) != 0 {
                    let value = out.plane().data()[site as usize];
                    assert_eq!(
                        origin(&params, value),
                        (0, site),
                        "seed {seed}, line {line}"
                    );
                    seen += 1;
                }
            }
        }
    }
    assert!(seen > 0);
}

/// Codes of `layout`: optical black at `black`, the active words hashed with `key`.
fn hashed_codes(layout: &Layout, black: u16, key: u32) -> Codes {
    let mut codes = Codes::new(layout.clone());
    let (ob, len) = (layout.ob_cols() as usize, layout.line_len() as usize);
    for (i, code) in codes.words_mut().iter_mut().enumerate() {
        *code = if i % len < ob {
            black
        } else {
            (noise::hash_u32(i as u32, key, noise::salt("memory test"), 5) & 0x0fff) as u16
        };
    }
    codes
}

#[test]
fn rows_lost_to_a_brownout_read_what_the_sdram_held_there() {
    for direction in [Direction::TopDown, Direction::BottomUp] {
        let camera = small(direction);
        let preset = preset(Vec::new());
        let timing = Timing::derive(&camera, &preset, 64, 48).unwrap();
        let layout = Layout::from_timing(&timing, direction);
        let processor = isp::Params::resolve(&camera, &preset, &timing).unwrap();
        let params = native(&camera, &preset);
        let black = camera.adc.black_level;
        let nominal = Rail::new(&layout, camera.power.rail_v);
        // The processor browns out at the start of readout line `lost`.
        let lost = layout.rows() / 2 + 3;
        let mut browned = nominal.clone();
        let low_v = processor.dsp_brownout_v - 0.1;
        for line in lost..layout.rows() {
            browned.set(line, low_v, low_v);
        }
        let clamp = |key: u32, rail: &Rail| {
            let mut raw = Raw::new(layout.cols(), layout.rows());
            isp::clamp_black(
                &processor,
                &hashed_codes(&layout, black, key),
                rail,
                &mut raw,
            );
            raw
        };

        // From power-on, the lost rows read the SDRAM's power-on content.
        let mut state = State::default();
        let first = clamp(1, &browned);
        assert_eq!(first.dropped_from(), Some(layout.image_row(lost)));
        let read = run(&params, &mut state, 0, &first);
        for line in 0..layout.rows() {
            let y = layout.image_row(line);
            if line < lost {
                assert_eq!(read.plane().row(y), first.plane().row(y), "{direction:?}");
            } else {
                let power_on: Vec<f32> = (0..params.pitch)
                    .map(|x| params.power_on.word(params.address(x, y)))
                    .collect();
                assert_eq!(read.plane().row(y), power_on, "{direction:?}: row {y}");
            }
        }

        // A whole frame fills the buffer; the next brownout shows it below the line.
        let whole = clamp(2, &nominal);
        assert_eq!(run(&params, &mut state, 1, &whole), whole, "{direction:?}");
        let now = clamp(3, &browned);
        let read = run(&params, &mut state, 2, &now);
        for line in 0..layout.rows() {
            let y = layout.image_row(line);
            if line < lost {
                assert_eq!(read.plane().row(y), now.plane().row(y), "{direction:?}");
            } else {
                assert!(now.plane().row(y).iter().all(|&site| site == 0.0));
                assert_eq!(
                    read.plane().row(y),
                    whole.plane().row(y),
                    "{direction:?}: row {y} shows the frame before"
                );
            }
        }

        // The next whole frame overwrites every row again.
        let later = clamp(4, &nominal);
        assert_eq!(run(&params, &mut state, 3, &later), later, "{direction:?}");
    }
}

/// A uniform card of one 8-bit sRGB level.
fn card((width, height): (u32, u32), level: u8) -> Scene {
    Scene::from_srgb8(width, height, &vec![level; (width * height * 3) as usize])
}

/// The mean of the 8-bit channels of row `row`.
fn row_mean(frame: &Frame, row: u32) -> f64 {
    let sum: f64 = (0..frame.width())
        .flat_map(|x| frame.pixel(x, row))
        .map(f64::from)
        .sum();
    sum / f64::from(3 * frame.width())
}

#[test]
fn a_spike_that_browns_the_processor_out_leaves_the_last_frame_below_the_line() {
    let camera = camera();
    let power = camera.power.clone();
    // A narrow, tall frame, so the 235 us recovery spans about one line.
    let size = (64, 1024);
    let mut spiking = Preset::new("a520");
    spiking.readout.resolution = Resolution::MatchInput;
    spiking.readout.orientation = Orientation::Landscape;
    spiking.bend.rail = Some(RailBend {
        spike_ma: (power.rail_v - power.dsp_brownout_v + 0.2) / 5.0 * 1000.0,
        spike_hz: 4.0,
        spike_ms: 3.0,
        source_ohm: Some(5.0),
        ..RailBend::default()
    });
    let mut buffered = spiking.clone();
    buffered.bend.memory = Some(MemoryBend::default());
    let build = |preset: Preset| {
        Simulator::builder((camera.clone(), preset))
            .input_size(size.0, size.1)
            .build()
            .unwrap()
    };
    let (mut plain, mut sdram) = (build(spiking), build(buffered));
    // Two greys in turn, so a row tells which frame it came from.
    let cards = [card(size, 80), card(size, 170)];
    let mut whole: Option<Frame> = None;
    let mut checked = 0;
    for index in 0..24u64 {
        let scene = &cards[(index % 2) as usize];
        let without = plain.frame(scene, index).clone();
        let with = sdram.frame(scene, index).clone();
        let rail = sdram.rail();
        let rows = rail.lines();
        let drop = (0..rows).find(|&line| rail.start_v(line) < power.dsp_brownout_v);
        if let (Some(line), Some(before)) = (drop, &whole)
            && line > 32
            && line + 32 < rows
        {
            for row in 0..line - 16 {
                assert_eq!(
                    (0..with.width())
                        .map(|x| with.pixel(x, row))
                        .collect::<Vec<_>>(),
                    (0..without.width())
                        .map(|x| without.pixel(x, row))
                        .collect::<Vec<_>>(),
                    "frame {index}: row {row} above the brownout"
                );
            }
            let now = row_mean(&with, line / 2);
            for row in line + 16..rows {
                assert!(
                    row_mean(&without, row) < 2.0,
                    "frame {index}: row {row} without the memory bend"
                );
                let stale = row_mean(&with, row);
                assert!(
                    (stale - row_mean(before, row)).abs() < 1.0 && (stale - now).abs() > 20.0,
                    "frame {index}: row {row} at {stale}, the frame before at {}, this one at {now}",
                    row_mean(before, row)
                );
            }
            checked += 1;
        }
        whole = drop.is_none().then_some(with);
    }
    assert!(checked > 0, "no brownout followed a whole frame");
}

fn data(pin: u8, kind: FaultKind) -> DataFault {
    DataFault {
        pin,
        kind,
        with: None,
        on_frac: None,
        switch_hz: None,
    }
}

fn data_bridge(pin: u8, with: u8) -> DataFault {
    DataFault {
        with: Some(with),
        ..data(pin, FaultKind::Bridge)
    }
}

fn data_marginal(fault: DataFault, on_frac: f64, switch_hz: f64) -> DataFault {
    DataFault {
        on_frac: Some(on_frac),
        switch_hz: Some(switch_hz),
        ..fault
    }
}

/// A preset with faults on the address and data pins and the SDRAM at `temperature_c`.
fn memory_preset(
    faults: Vec<AddressFault>,
    data_faults: Vec<DataFault>,
    temperature_c: Option<f64>,
) -> Preset {
    let mut preset = Preset::new("a520");
    preset.bend.memory = Some(MemoryBend {
        faults,
        data_faults,
        temperature_c,
    });
    preset
}

/// The stage's parameters and a raw clamped as the processor clamps it, for a 64 by 48 input.
///
/// The optical black sits at the camera's black level; every active bus word is
/// `active`, or without it hashed with `key`.
fn clamped(camera: &Camera, preset: &Preset, key: u32, active: Option<u16>) -> (Params, Raw) {
    let timing = Timing::derive(camera, preset, 64, 48).unwrap();
    let layout = Layout::from_timing(&timing, camera.clocking(timing.mode).direction);
    let processor = isp::Params::resolve(camera, preset, &timing).unwrap();
    let params = memory::Params::resolve(camera, preset, &timing)
        .unwrap()
        .expect("the preset has a memory bend");
    let mut codes = hashed_codes(&layout, camera.adc.black_level, key);
    if let Some(active) = active {
        let (ob, len) = (layout.ob_cols() as usize, layout.line_len() as usize);
        for (i, code) in codes.words_mut().iter_mut().enumerate() {
            if i % len >= ob {
                *code = active;
            }
        }
    }
    let mut raw = Raw::new(layout.cols(), layout.rows());
    isp::clamp_black(
        &processor,
        &codes,
        &Rail::new(&layout, camera.power.rail_v),
        &mut raw,
    );
    (params, raw)
}

/// The processor's code of every site: its value with its row's black level added back.
fn codes_of(raw: &Raw) -> Vec<u32> {
    let width = raw.plane().width() as usize;
    raw.plane()
        .data()
        .iter()
        .enumerate()
        .map(|(i, &value)| (value + raw.black_levels()[i / width]).round() as u32)
        .collect()
}

/// A raw whose every site is `value`, over a zero black level: its codes are its values.
fn uniform(params: &Params, value: f32) -> Raw {
    let mut raw = Raw::new(params.pitch, params.rows);
    raw.plane_mut().data_mut().fill(value);
    raw
}

/// A raw of hashed 10-bit codes over a zero black level, each with bit 7 clear.
fn coded(params: &Params, frame: u32) -> Raw {
    let mut raw = Raw::new(params.pitch, params.rows);
    for (site, value) in (0u32..).zip(raw.plane_mut().data_mut()) {
        let code = noise::hash_u32(site, frame, noise::salt("memory test codes"), 9) & 0x3ff;
        *value = (code & !0x80) as f32;
    }
    raw
}

fn simulator(preset: Preset, (width, height): (u32, u32)) -> Simulator {
    Simulator::builder((camera(), preset))
        .input_size(width, height)
        .build()
        .unwrap()
}

/// The mean of a frame's 8-bit channels.
fn frame_mean(frame: &Frame) -> f64 {
    (0..frame.height())
        .map(|row| row_mean(frame, row))
        .sum::<f64>()
        / f64::from(frame.height())
}

#[test]
fn a_solid_d9_high_puts_every_code_in_the_top_half() {
    for direction in [Direction::TopDown, Direction::BottomUp] {
        let camera = small(direction);
        let preset = memory_preset(Vec::new(), vec![data(9, FaultKind::High)], None);
        let (params, raw) = clamped(&camera, &preset, 1, None);
        let before = codes_of(&raw);
        assert!(before.iter().any(|&code| code < 512), "{direction:?}");
        let mut state = State::default();
        for frame in 0..2 {
            let after = codes_of(&run(&params, &mut state, frame, &raw));
            assert!(after.iter().all(|&code| code >= 512), "{direction:?}");
            for (site, (&was, &is)) in before.iter().zip(&after).enumerate() {
                assert_eq!(is, was | 512, "{direction:?}, frame {frame}, site {site}");
            }
        }
    }
}

#[test]
fn a_stuck_pin_reads_stuck_over_decayed_cells() {
    // At 130 C most words hold a decayed cell, bit 9's among them; the word crosses
    // the pins after its cells are sensed, so the pin decides bit 9.
    let camera = small(Direction::TopDown);
    for (kind, top_half) in [(FaultKind::High, true), (FaultKind::Low, false)] {
        let preset = memory_preset(Vec::new(), vec![data(9, kind)], Some(130.0));
        let (params, raw) = clamped(&camera, &preset, 9, None);
        let (fallen, risen) = params.span.clone().fold((0, 0), |(fall, rise), address| {
            let decay = params.retention.decay(address);
            (
                fall + u32::from(decay.fall & 512 != 0),
                rise + u32::from(decay.rise & 512 != 0),
            )
        });
        assert!(
            fallen > 0 && risen > 0,
            "bit 9 decays: {fallen} fall, {risen} rise"
        );
        let out = codes_of(&run(&params, &mut State::default(), 0, &raw));
        assert!(
            out.iter().all(|&code| (code >= 512) == top_half),
            "d9 {kind} over decayed cells"
        );
    }
}

#[test]
fn a_solid_d9_low_folds_the_highlights_down() {
    let camera = small(Direction::TopDown);
    let preset = memory_preset(Vec::new(), vec![data(9, FaultKind::Low)], None);
    let (params, raw) = clamped(&camera, &preset, 2, None);
    let out = run(&params, &mut State::default(), 0, &raw);
    let (before, after) = (codes_of(&raw), codes_of(&out));
    let mut folded = 0;
    for (site, (&was, &is)) in before.iter().zip(&after).enumerate() {
        if was >= 512 {
            assert_eq!(is, was - 512, "site {site}");
            folded += 1;
        } else {
            assert_eq!(
                out.plane().data()[site].to_bits(),
                raw.plane().data()[site].to_bits(),
                "site {site} keeps its value bit for bit"
            );
        }
    }
    assert!(
        folded > before.len() / 4,
        "{folded} of {} folded",
        before.len()
    );

    // The clipped highlights read one code below the middle of the range.
    let (params, white) = clamped(&camera, &preset, 2, Some(1023 << camera.adc.dsp_lsb));
    assert!(codes_of(&white).iter().all(|&code| code == 1023));
    let out = run(&params, &mut State::default(), 0, &white);
    assert!(codes_of(&out).iter().all(|&code| code == 511));
}

#[test]
fn a_solid_d5_high_lifts_the_black_of_a_dark_frame_by_32_codes() {
    let camera = small(Direction::TopDown);
    let black_code = camera.adc.black_level >> camera.adc.dsp_lsb;
    assert_eq!(
        black_code & 32,
        0,
        "the A520's black of {black_code} lacks bit 5"
    );
    let preset = memory_preset(Vec::new(), vec![data(5, FaultKind::High)], None);
    let (params, dark) = clamped(&camera, &preset, 3, Some(camera.adc.black_level));
    assert!(dark.plane().data().iter().all(|&value| value.abs() < 1e-6));
    let out = run(&params, &mut State::default(), 0, &dark);
    assert!(
        out.plane()
            .data()
            .iter()
            .all(|&value| (value - 32.0).abs() < 1e-4),
        "every site 32 codes above black"
    );

    // Developed, a black card through the stuck pin is grey.
    let mut clean = Preset::new("a520");
    clean.readout.resolution = Resolution::MatchInput;
    let mut lifted = clean.clone();
    lifted.bend.memory = preset.bend.memory;
    let black = card((64, 48), 0);
    let plain = frame_mean(simulator(clean, (64, 48)).frame(&black, 0));
    let stuck = frame_mean(simulator(lifted, (64, 48)).frame(&black, 0));
    assert!(
        stuck > plain + 10.0,
        "{plain} without, {stuck} with d5 high"
    );
}

#[test]
fn a_data_bridge_reads_the_or_of_its_two_pins() {
    let camera = small(Direction::BottomUp);
    let preset = memory_preset(Vec::new(), vec![data_bridge(3, 4)], None);
    let (params, raw) = clamped(&camera, &preset, 4, None);
    let out = run(&params, &mut State::default(), 0, &raw);
    let pair = 0b1_1000;
    let mut changed = 0;
    for (site, (&was, &is)) in codes_of(&raw).iter().zip(&codes_of(&out)).enumerate() {
        let expected = if was & pair == 0 { was } else { was | pair };
        assert_eq!(is, expected, "site {site}: {was:#b}");
        if is != was {
            changed += 1;
        }
    }
    assert!(changed > 0);
    assert_eq!(params.cross(0b0_1000, 1), 0b1_1000);
    assert_eq!(
        params.cross(0b0_1000, 0),
        0b0_1000,
        "the fault is bit 0 of `active`"
    );
}

#[test]
fn a_fault_on_a_pin_above_the_sample_changes_nothing() {
    let camera = small(Direction::TopDown);
    assert!(camera.memory.data_pins > camera.adc.dsp_bits + 5);
    for faults in [
        vec![data(12, FaultKind::High)],
        vec![data(10, FaultKind::Low)],
        vec![data(15, FaultKind::High), data_bridge(3, 12)],
        vec![data_bridge(10, 14)],
        vec![data_marginal(data(12, FaultKind::High), 0.5, 300.0)],
    ] {
        let preset = memory_preset(Vec::new(), faults.clone(), None);
        let (params, raw) = clamped(&camera, &preset, 5, None);
        assert!(
            params.data_faults.iter().all(DataPin::is_inert),
            "{faults:?}"
        );
        let mut state = State::default();
        for frame in 0..3 {
            assert_eq!(run(&params, &mut state, frame, &raw), raw, "{faults:?}");
        }
    }
}

#[test]
fn a_data_pin_beyond_the_sdram_is_rejected_by_name() {
    let camera = camera();
    let pins = camera.memory.data_pins;
    for (fault, key, named) in [
        (data(pins, FaultKind::Low), "pin", pins),
        (data_bridge(2, pins + 4), "with", pins + 4),
    ] {
        let preset = memory_preset(Vec::new(), vec![data(0, FaultKind::High), fault], None);
        preset.validate().unwrap();
        let timing = Timing::derive(&camera, &preset, 2272, 1704).unwrap();
        let error = memory::Params::resolve(&camera, &preset, &timing).unwrap_err();
        assert!(error.is_config(), "{error}");
        let message = error.to_string();
        assert!(
            message.contains(&format!("bend.memory.data_faults[1].{key}"))
                && message.contains(&format!("pin d{named}")),
            "{message}"
        );
    }
}

#[test]
fn a_data_contact_closed_all_the_time_renders_as_the_solid_fault() {
    let camera = small(Direction::TopDown);
    let flaky = data_marginal(data(2, FaultKind::High), 0.4, 300.0);
    let solid_low = memory_preset(Vec::new(), vec![data(9, FaultKind::Low), flaky], None);
    let closed_low = memory_preset(
        Vec::new(),
        vec![data_marginal(data(9, FaultKind::Low), 1.0, 300.0), flaky],
        None,
    );
    let (solid_params, raw) = clamped(&camera, &solid_low, 6, None);
    let (closed_params, _) = clamped(&camera, &closed_low, 6, None);
    let (mut solid_state, mut closed_state) = (State::default(), State::default());
    for frame in 0..3 {
        assert_eq!(
            run(&closed_params, &mut closed_state, frame, &raw),
            run(&solid_params, &mut solid_state, frame, &raw),
            "frame {frame}"
        );
    }
}

#[test]
fn a_marginal_data_contact_acts_at_each_lines_write_and_read() {
    // d9 stuck high on a flaky contact, no address fault: a word holds bit 9 where the
    // contact was faulted at its write, and the read shows it where the contact was
    // faulted then or at the read.
    let camera = small(Direction::BottomUp);
    let preset = memory_preset(
        Vec::new(),
        vec![data_marginal(data(9, FaultKind::High), 0.5, 300.0)],
        None,
    );
    let (params, raw) = clamped(&camera, &preset, 7, None);
    let before = codes_of(&raw);
    let mut state = State::default();
    let (mut stuck, mut clean) = (0, 0);
    for frame in 0..6 {
        let after = codes_of(&run(&params, &mut state, frame, &raw));
        let (write, read) = state.data_contact_lines(9).unwrap();
        for (site, (&was, &is)) in before.iter().zip(&after).enumerate() {
            let line = params.line(site as u32 / params.pitch) as usize;
            if write[line] || read[line] {
                assert_eq!(is, was | 512, "frame {frame}, site {site}");
                stuck += 1;
            } else {
                assert_eq!(is, was, "frame {frame}, site {site}");
                clean += 1;
            }
        }
    }
    assert!(stuck > 0 && clean > 0, "{stuck} stuck, {clean} clean");
}

#[test]
fn a_marginal_data_contact_leaves_its_bit_in_a_stale_word() {
    // a4 stuck low on a marginal contact: a site with the row bit set writes into its
    // twin's word while the contact is faulted, and read with the contact clear finds
    // its own word as the frame before left it. d7 stuck high on a marginal contact of
    // its own: that word holds bit 7 where the data contact was faulted at the frame
    // before's write, and a clean read shows it.
    let camera = small(Direction::TopDown);
    let [None, Some(row_bit)] = pin_bits(&camera, 4) else {
        panic!("a4 carries only a row bit");
    };
    let (mut stale_bit, mut stale_clean, mut read_bit) = (0, 0, 0);
    for seed in 0..40 {
        let mut preset = memory_preset(
            vec![marginal(solid(4, FaultKind::Low), 0.5, 300.0)],
            vec![data_marginal(data(7, FaultKind::High), 0.5, 300.0)],
            None,
        );
        preset.seed = seed;
        let params = native(&camera, &preset);
        let mut state = State::default();
        let (mut raws, mut outputs, mut lines) = (Vec::new(), Vec::new(), Vec::new());
        for frame in 0..4 {
            let raw = coded(&params, frame);
            outputs.push(run(&params, &mut state, u64::from(frame), &raw));
            raws.push(raw);
            let (address_write, address_read) = state.contact_lines(4).unwrap();
            let (data_write, data_read) = state.data_contact_lines(7).unwrap();
            lines.push([address_write, address_read, data_write, data_read].map(<[bool]>::to_vec));
        }
        for frame in 1..4 {
            let [address_write, address_read, _, data_read] = &lines[frame];
            let [earlier_address_write, _, earlier_data_write, _] = &lines[frame - 1];
            for line in 0..params.rows as usize {
                // Written astray now, read from the word the frame before wrote in place.
                if !(address_write[line] && !address_read[line] && !earlier_address_write[line]) {
                    continue;
                }
                let y = params.line(line as u32);
                for x in 0..params.pitch {
                    let site = y * params.pitch + x;
                    if u64::from(site) & bit(row_bit) == 0 {
                        continue;
                    }
                    let earlier = raws[frame - 1].plane().data()[site as usize];
                    let value = outputs[frame].plane().data()[site as usize];
                    let expected = if earlier_data_write[line] || data_read[line] {
                        (earlier as u32 | 0x80) as f32
                    } else {
                        earlier
                    };
                    assert_eq!(value, expected, "seed {seed}, frame {frame}, line {line}");
                    match (earlier_data_write[line], data_read[line]) {
                        (true, false) => stale_bit += 1,
                        (false, false) => stale_clean += 1,
                        (_, true) => read_bit += 1,
                    }
                }
            }
        }
    }
    assert!(
        stale_bit > 0 && stale_clean > 0 && read_bit > 0,
        "stale with the bit {stale_bit}, stale clean {stale_clean}, read faulted {read_bit}"
    );
}

#[test]
fn contacts_on_d3_and_a3_draw_different_streams() {
    let camera = small(Direction::TopDown);
    let preset = memory_preset(
        vec![marginal(solid(3, FaultKind::Low), 0.5, 300.0)],
        vec![data_marginal(data(3, FaultKind::Low), 0.5, 300.0)],
        None,
    );
    let params = native(&camera, &preset);
    let mut state = State::default();
    run(&params, &mut state, 0, &uniform(&params, 100.0));
    let address = state.contact_lines(3).unwrap();
    let data = state.data_contact_lines(3).unwrap();
    assert_ne!(address, data);
}

#[test]
fn at_the_reference_temperature_no_cell_of_the_still_span_decays() {
    let camera = camera();
    let reference = camera.memory.reference_temp_c;
    for temperature_c in [None, Some(reference)] {
        let preset = memory_preset(Vec::new(), Vec::new(), temperature_c);
        let params = native(&camera, &preset);
        let retention = &params.retention;
        assert!((retention.temperature_c - reference).abs() < 1e-12);
        // Not one cell in 2^32: the hashed tail does not come near the refresh period.
        assert!(retention.decay_frac < 1e-12, "{retention:?}");
        assert!(retention.is_inert());
        let raw = coded(&params, 0);
        let mut state = State::default();
        assert_eq!(run(&params, &mut state, 0, &raw), raw);
        let span = state.span();
        assert_eq!(span, params.span);
        assert_eq!(span.end - span.start, 2272 * 1704);
        assert!(
            span.clone()
                .all(|address| state.decay(address) == Some(Decay::default())),
            "a word decays at {temperature_c:?}"
        );
    }
}

#[test]
fn a_hot_board_loses_about_a_fifth_of_the_sites_at_120_c_and_a_thousandth_at_110_c() {
    let camera = camera();
    let mut fractions = Vec::new();
    for temperature_c in [100.0, 110.0, 120.0, 130.0] {
        let preset = memory_preset(Vec::new(), Vec::new(), Some(temperature_c));
        let params = native(&camera, &preset);
        let retention = params.retention;
        let mut state = State::default();
        // All ones shows the true cells, which fall; all zeros the anti cells, which rise.
        let ones = run(&params, &mut state, 0, &uniform(&params, 1023.0));
        let zeros = run(&params, &mut state, 1, &uniform(&params, 0.0));
        let sites = params.pitch * params.rows;
        let mut hit = 0u32;
        for site in 0..sites {
            let address = params.address(site % params.pitch, site / params.pitch);
            let decay = state.decay(address).unwrap();
            assert_eq!(decay, retention.decay(address), "word {address}");
            let anti = address >> retention.row_bit & 1 == 1;
            assert!(
                if anti {
                    decay.fall == 0
                } else {
                    decay.rise == 0
                },
                "word {address}: {decay:?}, the cells of a row all leak one way"
            );
            let i = site as usize;
            assert_eq!(ones.plane().data()[i], f32::from(1023 & !decay.fall));
            assert_eq!(zeros.plane().data()[i], f32::from(decay.rise));
            if !decay.is_empty() {
                hit += 1;
            }
        }
        let fraction = f64::from(hit) / f64::from(sites);
        let expected = 1.0 - (1.0 - retention.decay_frac).powf(f64::from(params.sample_bits));
        eprintln!(
            "{temperature_c} C: median retention {:.4} s, cells decayed {:.4e}, sites with a \
             decayed bit {hit} of {sites} = {fraction:.6} (expected {expected:.6})",
            retention.median_s, retention.decay_frac
        );
        let sigma = (expected * (1.0 - expected) / f64::from(sites)).sqrt();
        assert!(
            (fraction - expected).abs() <= 6.0 * sigma + 2.0 / f64::from(sites),
            "{temperature_c} C: {fraction} of the sites against {expected}"
        );
        fractions.push(fraction);
    }
    assert!(
        fractions[0] < 1e-4,
        "100 C: isolated sites, {}",
        fractions[0]
    );
    assert!(
        (3e-4..1e-2).contains(&fractions[1]),
        "110 C: about a thousandth, {}",
        fractions[1]
    );
    assert!(
        (0.1..0.3).contains(&fractions[2]),
        "120 C: about a fifth, {}",
        fractions[2]
    );
    assert!(fractions[3] > 0.5, "130 C: most sites, {}", fractions[3]);
}

#[test]
fn the_decayed_cells_are_the_same_in_every_frame_and_every_preset() {
    let camera = camera();
    let plain = memory_preset(Vec::new(), Vec::new(), Some(120.0));
    let mut other = memory_preset(
        vec![solid(10, FaultKind::High)],
        vec![data(12, FaultKind::Low)],
        Some(120.0),
    );
    other.seed = 99;
    let a = matched(&camera, &plain, 568, 426);
    let b = matched(&camera, &other, 568, 426);
    let cooler = matched(
        &camera,
        &memory_preset(Vec::new(), Vec::new(), Some(110.0)),
        568,
        426,
    );
    assert!(b.span.start <= a.span.start && a.span.end < b.span.end);
    let ones = uniform(&a, 1023.0);
    let (mut state_a, mut state_b) = (State::default(), State::default());
    let mut first: Option<Raw> = None;
    for frame in 0..3 {
        let out = run(&a, &mut state_a, frame, &ones);
        run(&b, &mut state_b, frame, &uniform(&b, 1023.0));
        if let Some(first) = &first {
            assert_eq!(&out, first, "frame {frame}: the same sites fall");
        } else {
            assert_ne!(out, ones, "cells decay at 120 C");
            first = Some(out);
        }
        for address in a.span.clone() {
            let decay = state_a.decay(address).unwrap();
            assert_eq!(
                state_b.decay(address),
                Some(decay),
                "frame {frame}: word {address}"
            );
            // A cell weak enough to decay at 110 C decays at 120 C too.
            let cool = cooler.retention.decay(address);
            assert_eq!(cool.fall & !decay.fall, 0, "word {address}");
            assert_eq!(cool.rise & !decay.rise, 0, "word {address}");
        }
    }
}

#[test]
fn a_new_temperature_is_kept_and_takes_effect_with_the_next_frame() {
    let size = (64, 48);
    let scene = scene(size.0, size.1);
    let mut cool = memory_preset(
        Vec::new(),
        vec![data_marginal(data(7, FaultKind::High), 0.3, 40.0)],
        None,
    );
    cool.readout.resolution = Resolution::MatchInput;
    let mut sim = simulator(cool.clone(), size);
    for index in 0..2 {
        sim.frame(&scene, index);
    }
    let span = sim.memory_state().span();
    let decayed = |state: &State| {
        span.clone()
            .filter(|&address| !state.decay(address).unwrap().is_empty())
            .count() as u64
    };
    assert_eq!(decayed(sim.memory_state()), 0);
    let before = sim.memory_state().clone();

    let mut hot = cool.clone();
    hot.bend.memory.as_mut().unwrap().temperature_c = Some(125.0);
    assert_eq!(sim.set_preset(hot).unwrap(), KEPT);
    assert_eq!(
        sim.memory_state(),
        &before,
        "the SDRAM's content, masks and contacts are kept"
    );
    sim.frame(&scene, 2);
    let after = sim.memory_state();
    let (_, last_read) = before.data_contact_lines(7).unwrap();
    assert_eq!(
        after.data_contact_lines(7).unwrap().0,
        last_read,
        "the data contact runs on"
    );
    let words = span.end - span.start;
    assert!(decayed(after) > words / 3, "{} of {words}", decayed(after));

    assert_eq!(sim.set_preset(cool).unwrap(), KEPT);
    sim.frame(&scene, 3);
    assert_eq!(decayed(sim.memory_state()), 0, "back at the reference");
}

/// Data faults stuck, marginal and bridged, a marginal address contact and a hot board.
fn bent_data() -> MemoryBend {
    MemoryBend {
        faults: vec![marginal(solid(9, FaultKind::Low), 0.3, 250.0)],
        data_faults: vec![
            data(8, FaultKind::Low),
            data_marginal(data(6, FaultKind::High), 0.3, 40.0),
            data_bridge(1, 2),
        ],
        temperature_c: Some(115.0),
    }
}

#[test]
fn data_faults_and_heat_replay_from_reset_byte_for_byte() {
    let scene = scene(160, 120);
    for mode in [Mode::Still, Mode::Movie] {
        let mut clean = Preset::new("a520");
        clean.readout.mode = mode;
        clean.readout.resolution = Resolution::MatchInput;
        let mut bent = clean.clone();
        bent.bend.memory = Some(bent_data());
        let mut sim = simulator(bent.clone(), (160, 120));
        let first: Vec<Frame> = (0..4).map(|i| sim.frame(&scene, i).clone()).collect();
        sim.reset();
        let again: Vec<Frame> = (0..4).map(|i| sim.frame(&scene, i).clone()).collect();
        assert_eq!(again, first, "{mode}: from reset");
        let mut fresh = simulator(bent, (160, 120));
        let third: Vec<Frame> = (0..4).map(|i| fresh.frame(&scene, i).clone()).collect();
        assert_eq!(third, first, "{mode}: from a new simulator");
        let mut plain = simulator(clean, (160, 120));
        assert_ne!(plain.frame(&scene, 0), &first[0], "{mode}: the faults show");
    }
}

#[test]
fn data_faults_act_on_the_movie_raw() {
    let camera = small(Direction::TopDown);
    let mut preset = memory_preset(Vec::new(), vec![data(9, FaultKind::High)], None);
    preset.readout.mode = Mode::Movie;
    let (params, raw) = clamped(&camera, &preset, 8, None);
    assert!(
        params.rows < camera.sensor.native_height,
        "a movie raw of {} rows",
        params.rows
    );
    let before = codes_of(&raw);
    let after = codes_of(&run(&params, &mut State::default(), 0, &raw));
    for (site, (&was, &is)) in before.iter().zip(&after).enumerate() {
        assert_eq!(is, was | 512, "site {site}");
    }

    // A movie frame through d9 stuck low loses its highlights.
    let mut clean = Preset::new("a520");
    clean.readout.mode = Mode::Movie;
    clean.readout.resolution = Resolution::MatchInput;
    let mut folded = clean.clone();
    folded.bend.memory = Some(MemoryBend {
        data_faults: vec![data(9, FaultKind::Low)],
        ..MemoryBend::default()
    });
    let white = card((160, 120), 255);
    let plain = frame_mean(simulator(clean, (160, 120)).frame(&white, 0));
    let dimmed = frame_mean(simulator(folded, (160, 120)).frame(&white, 0));
    assert!(
        dimmed < plain - 20.0,
        "{plain} without, {dimmed} with d9 low"
    );
}
