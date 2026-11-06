//! The frame-buffer SDRAM's address pins: folds in powers of two, marginal contacts,
//! stale content, power-on content, resolution scaling and the running camera's state.

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::isp::Raw;
use vcb_core::memory::{self, Params, State};
use vcb_core::preset::{AddressFault, FaultKind, MemoryBend, Mode, Resolution};
use vcb_core::readout::Direction;
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing};

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
    preset.bend.memory = Some(MemoryBend { faults });
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
