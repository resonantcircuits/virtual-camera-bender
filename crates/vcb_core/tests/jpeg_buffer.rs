//! The compressed JPEG scan in the frame-buffer SDRAM: written and read back through
//! the raw's address pins, data pins, marginal contacts and hot cells, beside the raw.

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::codec;
use vcb_core::isp::Raw;
use vcb_core::memory::{self, Params, State};
use vcb_core::preset::{AddressFault, DataFault, FaultKind, MemoryBend, Mode, Resolution};
use vcb_core::{Camera, Frame, Preset, Reconfigured, Simulator, Timing, noise};

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

fn camera_text() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    std::fs::read_to_string(path).unwrap()
}

fn camera() -> Camera {
    toml::from_str(&camera_text()).unwrap()
}

/// A native-like small camera: a 48 by 32 raw in a 2^12-word SDRAM of 4 column, 1 bank
/// and 7 row bits, its scan at 2^11, the row-bit weight of its top pin `a6`, as the
/// A520's lies at `a11`'s.
///
/// Pins a0 to a3 carry a column and a row bit, a4 to a6 only a row bit.
fn small() -> Camera {
    let mut camera = camera();
    camera.sensor.native_width = 48;
    camera.sensor.native_height = 32;
    camera.memory.column_bits = 4;
    camera.memory.bank_bits = 1;
    camera.memory.row_bits = 7;
    camera.memory.jpeg_base = 1 << 11;
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

fn marginal(fault: AddressFault, on_frac: f64, switch_hz: f64) -> AddressFault {
    AddressFault {
        on_frac: Some(on_frac),
        switch_hz: Some(switch_hz),
        ..fault
    }
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

fn data_marginal(fault: DataFault, on_frac: f64, switch_hz: f64) -> DataFault {
    DataFault {
        on_frac: Some(on_frac),
        switch_hz: Some(switch_hz),
        ..fault
    }
}

/// A preset with faults on the address and data pins and the SDRAM at `temperature_c`.
fn preset(
    faults: Vec<AddressFault>,
    data_faults: Vec<DataFault>,
    temperature_c: Option<f64>,
) -> Preset {
    let mut preset = Preset::new("a520");
    preset.bend.memory = Some(MemoryBend {
        faults,
        data_faults,
        temperature_c,
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

/// The linear bit of pin `pin`'s row address on `camera` at native resolution.
fn row_bit(camera: &Camera, pin: u8) -> u32 {
    u32::from(camera.memory.column_bits) + u32::from(camera.memory.bank_bits) + u32::from(pin)
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

/// A scan of `words` hashed 16-bit words naming `key`, the low byte first.
fn tagged(words: usize, key: u32) -> Vec<u8> {
    (0..words as u32)
        .flat_map(|i| (noise::hash_u32(i, key, noise::salt("scan test"), 3) as u16).to_le_bytes())
        .collect()
}

/// The 16-bit words of `scan`, the low byte first, an odd last byte padded with zero.
fn words(scan: &[u8]) -> Vec<u16> {
    scan.chunks(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair.get(1).copied().unwrap_or(0)]))
        .collect()
}

/// Runs frame `index` through the stage: the raw, then the scan; returns both as read back.
fn run(params: &Params, state: &mut State, index: u64, raw: &Raw, scan: &[u8]) -> (Raw, Vec<u8>) {
    let mut raw = raw.clone();
    memory::bend(Some(params), state, index, &mut raw);
    let mut scan = scan.to_vec();
    memory::bend_scan(Some(params), state, index, &mut scan);
    (raw, scan)
}

#[test]
fn without_faults_at_the_reference_temperature_the_scan_comes_back_unchanged() {
    let mut scan = tagged(100, 0);
    let before = scan.clone();
    memory::bend_scan(None, &mut State::default(), 0, &mut scan);
    assert_eq!(
        scan, before,
        "without a memory section the scan passes untouched"
    );

    for camera in [small(), camera()] {
        let params = native(&camera, &preset(Vec::new(), Vec::new(), None));
        assert!(params.retention.is_inert());
        let mut state = State::default();
        for frame in 0..3u32 {
            let raw = distinct(&params, frame);
            // Lengths that vary from frame to frame, the odd ones padded in their last word.
            let mut scan = tagged(700 + 61 * frame as usize, frame);
            scan.truncate(scan.len() - frame as usize % 2);
            let (raw_out, scan_out) = run(&params, &mut state, frame.into(), &raw, &scan);
            assert_eq!(raw_out, raw, "frame {frame}");
            assert_eq!(scan_out, scan, "frame {frame}: {} bytes", scan.len());
        }
    }
}

#[test]
fn a_solid_a11_low_on_a_native_still_lays_the_scan_on_the_raws_words_and_leaves_the_raw_alone() {
    let camera = camera();
    let params = native(
        &camera,
        &preset(vec![solid(11, FaultKind::Low)], Vec::new(), None),
    );
    let fold = 1u64 << row_bit(&camera, 11);
    assert_eq!(params.jpeg_base, fold, "the scan lies at a11's row bit");
    assert!(
        params.address(params.pitch - 1, params.rows - 1) < fold,
        "the still raw ends below it"
    );
    let mut scan = tagged(3 * params.pitch as usize + 17, 5);
    scan.pop();
    let input = words(&scan);
    let (mut bent, mut plain) = (State::default(), State::default());
    for index in 0..2u32 {
        let raw = distinct(&params, index);
        let (with, scan_out) = run(&params, &mut bent, index.into(), &raw, &scan);
        let mut without = raw.clone();
        memory::bend(Some(&params), &mut plain, index.into(), &mut without);
        assert_eq!(
            with, without,
            "frame {index}: the raw with and without the scan"
        );
        assert_eq!(with, raw, "frame {index}: a11 reaches no word of the raw");
        // The fold is the same at the write and the read: the scan comes back whole.
        assert_eq!(scan_out, scan, "frame {index}");
        // The scan's words lie on the raw's first words, after the raw's read; its own
        // words hold the power-on content.
        let last = input.len() - 1;
        for i in [0, 1, params.pitch as usize, last / 2, last] {
            let (word, own) = (i as u64, fold + i as u64);
            assert_eq!(bent.scan_word(word), Some(input[i]), "word {i}");
            assert_eq!(
                bent.word(word),
                Some(
                    (u32::from(input[i]) & params.power_on.max_code) as f32
                        - params.power_on.black_code
                ),
                "the raw's processor reads word {i} as its low bits above black"
            );
            assert_eq!(
                plain.word(word),
                Some(raw.plane().data()[i]),
                "word {i} without the scan"
            );
            assert_eq!(
                bent.scan_word(own),
                Some(params.power_on.bits(own)),
                "word {own}"
            );
        }
    }
}

#[test]
fn a_solid_low_pin_below_the_scans_length_folds_the_scan_onto_itself() {
    // a3 low clears column bit 3 and row bit 8 of every address: the scan's words that
    // differ only there share one, and each reads the one of them written last.
    let camera = small();
    let params = native(
        &camera,
        &preset(vec![solid(3, FaultKind::Low)], Vec::new(), None),
    );
    let mask = params.faults[0]
        .bits
        .iter()
        .flatten()
        .fold(0u64, |mask, &bit| mask | 1 << bit);
    assert_eq!(mask, 1 << 3 | 1 << row_bit(&camera, 3));
    let scan = tagged(1000, 0);
    let input = words(&scan);
    let (_, out) = run(
        &params,
        &mut State::default(),
        0,
        &distinct(&params, 0),
        &scan,
    );
    let out = words(&out);
    let at = |i: usize| (params.jpeg_base + i as u64) & !mask;
    let mut moved = 0;
    for (i, &word) in out.iter().enumerate() {
        let last = (0..input.len()).rev().find(|&j| at(j) == at(i)).unwrap();
        assert_eq!(word, input[last], "word {i} reads word {last}");
        moved += usize::from(last != i);
    }
    assert!(moved > input.len() / 2, "{moved} of {} words", input.len());
}

#[test]
fn a_solid_d3_sets_bit_3_of_every_low_byte_and_d11_of_every_high_byte() {
    let camera = small();
    let mut scan = tagged(500, 1);
    scan.pop();
    for (pin, kind) in [
        (3, FaultKind::High),
        (3, FaultKind::Low),
        (11, FaultKind::High),
        (11, FaultKind::Low),
    ] {
        let params = native(&camera, &preset(Vec::new(), vec![data(pin, kind)], None));
        assert!(
            !params.scan_data_faults[0].is_inert(),
            "d{pin} carries a bit of the scan"
        );
        assert_eq!(
            params.data_faults[0].is_inert(),
            u32::from(pin) >= params.sample_bits,
            "d{pin} carries a bit of a raw sample only below dsp_bits"
        );
        let (_, out) = run(
            &params,
            &mut State::default(),
            0,
            &distinct(&params, 0),
            &scan,
        );
        let (parity, bit) = (usize::from(pin / 8), 1u8 << (pin % 8));
        let mut changed = 0;
        for (i, (&was, &is)) in scan.iter().zip(&out).enumerate() {
            let expected = match (i % 2 == parity, kind) {
                (false, _) => was,
                (true, FaultKind::High) => was | bit,
                (true, _) => was & !bit,
            };
            assert_eq!(is, expected, "d{pin} {kind}: byte {i}");
            changed += usize::from(is != was);
        }
        assert!(changed > scan.len() / 8, "d{pin} {kind}: {changed} bytes");
    }
}

#[test]
fn a_marginal_address_contact_reads_the_last_frames_bytes_where_its_write_went_astray() {
    // a6 low folds the scan, from 2^11 on, onto the raw's words while it is faulted. A
    // burst written aside and read in place finds what the frame before left in the
    // burst's own words, or the power-on content.
    let camera = small();
    let fault = marginal(solid(6, FaultKind::Low), 0.5, 300.0);
    let (mut stale, mut power_on) = (0, 0);
    for seed in 0..40 {
        let mut preset = preset(vec![fault], Vec::new(), None);
        preset.seed = seed;
        let params = native(&camera, &preset);
        let pitch = params.pitch as usize;
        let bursts = 20;
        let mut state = State::default();
        let (mut scans, mut outs, mut samples) = (Vec::new(), Vec::new(), Vec::new());
        for frame in 0..3u32 {
            let scan = tagged(bursts * pitch, frame);
            let (_, out) = run(
                &params,
                &mut state,
                frame.into(),
                &distinct(&params, frame),
                &scan,
            );
            let (write, read) = state.scan_contact_bursts(6).unwrap();
            assert_eq!((write.len(), read.len()), (bursts, bursts));
            samples.push((write.to_vec(), read.to_vec()));
            scans.push(words(&scan));
            outs.push(words(&out));
        }
        for frame in 0..3 {
            let (write, read) = &samples[frame];
            for burst in 0..bursts {
                if !write[burst] || read[burst] {
                    continue;
                }
                let words = burst * pitch..(burst + 1) * pitch;
                let found = &outs[frame][words.clone()];
                // Written aside now, read in place: the burst's words as the frame
                // before left them.
                match frame.checked_sub(1) {
                    Some(before) if !samples[before].0[burst] => {
                        assert_eq!(
                            found, &scans[before][words],
                            "seed {seed}, frame {frame}, burst {burst}: the frame before shows"
                        );
                        stale += 1;
                    }
                    None => {
                        let expected: Vec<u16> = words
                            .map(|i| params.power_on.bits(params.jpeg_base + i as u64))
                            .collect();
                        assert_eq!(found, expected, "seed {seed}, burst {burst}");
                        power_on += 1;
                    }
                    Some(_) => {}
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
fn the_scans_bursts_share_the_contacts_samples_with_the_raws_lines_and_leave_the_raw_alone() {
    let camera = small();
    let mut preset = preset(
        vec![marginal(solid(4, FaultKind::Low), 0.5, 300.0)],
        vec![data_marginal(data(7, FaultKind::High), 0.4, 200.0)],
        None,
    );
    preset.seed = 5;
    let params = native(&camera, &preset);
    let (rows, pitch) = (params.rows as usize, params.pitch as usize);
    // A scan within the frame's lines, and one past them.
    for bursts in [rows / 2, rows + 9] {
        let shared = bursts.min(rows);
        let (mut with, mut without) = (State::default(), State::default());
        let mut predicted: Option<[Vec<bool>; 2]> = None;
        for frame in 0..4u32 {
            let raw = distinct(&params, frame);
            let scan = tagged(bursts * pitch, frame);
            let (out, _) = run(&params, &mut with, frame.into(), &raw, &scan);
            let mut plain = raw.clone();
            memory::bend(Some(&params), &mut without, frame.into(), &mut plain);
            assert_eq!(
                out, plain,
                "{bursts} bursts, frame {frame}: the raw as without the scan"
            );
            assert_eq!(with.contact_lines(4), without.contact_lines(4));
            assert_eq!(with.data_contact_lines(7), without.data_contact_lines(7));
            let lines = [with.contact_lines(4), with.data_contact_lines(7)].map(Option::unwrap);
            let scans = [
                with.scan_contact_bursts(4),
                with.scan_data_contact_bursts(7),
            ]
            .map(Option::unwrap);
            for ((_, line_reads), (burst_writes, burst_reads)) in lines.iter().zip(&scans) {
                assert_eq!((burst_writes.len(), burst_reads.len()), (bursts, bursts));
                // Burst k is written as line k is read.
                assert_eq!(
                    burst_writes[..shared],
                    line_reads[..shared],
                    "frame {frame}"
                );
            }
            // Burst k of the last scan was read as this frame's line k.
            if let Some(predicted) = &predicted {
                for (reads, (_, line_reads)) in predicted.iter().zip(&lines) {
                    assert_eq!(reads[..shared], line_reads[..shared], "frame {frame}");
                }
            }
            predicted = Some(scans.map(|(_, reads)| reads.to_vec()));
        }
    }
}

#[test]
fn a_raw_line_read_through_a_marginal_a6_high_finds_the_last_scans_low_bits() {
    // a6 high sends the raw's addresses into the scan's words from 2^11 on. A line
    // written in place and read aside finds the last frame's scan there, read as a
    // sample: its low dsp_bits bits above the nominal black; before any scan, the
    // power-on content.
    let camera = small();
    let fault = marginal(solid(6, FaultKind::High), 0.5, 300.0);
    let away = 1u64 << row_bit(&camera, 6);
    let (mut from_scan, mut from_power_on) = (0, 0);
    for seed in 0..40 {
        let mut preset = preset(vec![fault], Vec::new(), None);
        preset.seed = seed;
        let params = native(&camera, &preset);
        assert_eq!(params.jpeg_base, away);
        let sites = (params.pitch * params.rows) as usize;
        let mut state = State::default();
        let mut last_scan: Option<Vec<u16>> = None;
        for frame in 0..4u32 {
            let raw = distinct(&params, frame);
            let scan = tagged(sites, frame + 7);
            let (out, _) = run(&params, &mut state, frame.into(), &raw, &scan);
            let (write, read) = state.contact_lines(6).unwrap();
            for line in 0..params.rows as usize {
                if write[line] || !read[line] {
                    continue;
                }
                let y = params.line(line as u32);
                for x in 0..params.pitch {
                    let site = (y * params.pitch + x) as usize;
                    let address = params.address(x, y) | away;
                    let expected = if let Some(scan) = &last_scan {
                        from_scan += 1;
                        let word = u32::from(scan[(address - params.jpeg_base) as usize]);
                        (word & params.power_on.max_code) as f32 - params.power_on.black_code
                    } else {
                        from_power_on += 1;
                        params.power_on.word(address)
                    };
                    assert_eq!(
                        out.plane().data()[site],
                        expected,
                        "seed {seed}, frame {frame}, line {line}, site {site}"
                    );
                }
            }
            last_scan = Some(words(&scan));
        }
    }
    assert!(
        from_scan > 0 && from_power_on > 0,
        "from the scan {from_scan}, from power-on {from_power_on}"
    );
}

#[test]
fn the_scans_store_covers_the_scan_and_widens_with_a_longer_one() {
    let camera = small();
    let params = native(&camera, &preset(Vec::new(), Vec::new(), None));
    let mut state = State::default();
    let mut spans = Vec::new();
    for (frame, length) in [(0u32, 300), (1, 900), (2, 500)] {
        let scan = tagged(length, frame);
        let (_, out) = run(
            &params,
            &mut state,
            frame.into(),
            &distinct(&params, frame),
            &scan,
        );
        assert_eq!(out, scan);
        let (span, needed) = (state.scan_span(), params.scan_span(scan.len()));
        assert_eq!(needed, params.jpeg_base..params.jpeg_base + length as u64);
        assert!(
            span.start <= needed.start && needed.end <= span.end,
            "frame {frame}: {span:?} covers {needed:?}"
        );
        assert!(state.span().end <= span.start, "beside the raw's store");
        spans.push(span);
    }
    assert!(
        spans[1].end > spans[0].end,
        "a longer scan widens the store: {spans:?}"
    );
    assert_eq!(spans[2], spans[1], "a shorter one keeps it");
    // The last scan's words, and the longer one's beyond it.
    let (last, longer) = (words(&tagged(500, 2)), words(&tagged(900, 1)));
    for i in [0, 499, 500, 899] {
        let expected = if i < 500 { last[i] } else { longer[i] };
        assert_eq!(
            state.scan_word(params.jpeg_base + i as u64),
            Some(expected),
            "word {i}"
        );
    }

    // a6's row bit is the scan's base: the raw's store then covers every word the scan
    // reaches, and the scan needs no store of its own.
    let folding = native(
        &camera,
        &preset(vec![solid(6, FaultKind::Low)], Vec::new(), None),
    );
    let mut state = State::default();
    let scan = tagged(300, 0);
    let (_, out) = run(&folding, &mut state, 0, &distinct(&folding, 0), &scan);
    assert_eq!(out, scan);
    assert_eq!(state.span(), folding.scan_span(scan.len()));
    assert!(state.scan_span().is_empty(), "{:?}", state.scan_span());
}

#[test]
fn the_state_keeps_the_scans_words_across_frames_and_presets() {
    // a5 high sets row bit 10: the scan's first 1024 words move to the next 1024.
    let camera = small();
    let plain_preset = preset(Vec::new(), Vec::new(), None);
    let raised_preset = preset(vec![solid(5, FaultKind::High)], Vec::new(), None);
    let (plain, raised) = (
        native(&camera, &plain_preset),
        native(&camera, &raised_preset),
    );
    let bit = 1u64 << row_bit(&camera, 5);
    let (first, second) = (tagged(600, 0), tagged(600, 1));
    let mut state = State::default();
    let (_, out) = run(&plain, &mut state, 0, &distinct(&plain, 0), &first);
    assert_eq!(out, first);
    let (_, out) = run(&raised, &mut state, 1, &distinct(&raised, 1), &second);
    assert_eq!(out, second, "written and read through the same stuck pin");
    for (i, (&a, &b)) in words(&first).iter().zip(&words(&second)).enumerate() {
        let address = plain.jpeg_base + i as u64;
        assert_eq!(
            state.scan_word(address),
            Some(a),
            "word {i} of the first frame"
        );
        assert_eq!(
            state.scan_word(address | bit),
            Some(b),
            "word {i} of the second"
        );
    }
    // Back to the plain preset, the first frame's scan lies where it reads.
    let mut third = tagged(600, 2);
    memory::bend_scan(Some(&plain), &mut state, 2, &mut third);
    assert_eq!(third, tagged(600, 2));

    // A running camera keeps the stage's state, the scan's store with it, on such a change.
    let size = (64, 48);
    let scene = vcb_core::Scene::from_srgb8(size.0, size.1, &vec![90; 64 * 48 * 3]);
    let mut running = plain_preset.clone();
    running.readout.resolution = Resolution::MatchInput;
    let mut sim = Simulator::builder((self::camera(), running.clone()))
        .input_size(size.0, size.1)
        .build()
        .unwrap();
    sim.frame(&scene, 0);
    let before = sim.memory_state().clone();
    let mut changed = raised_preset;
    changed.readout.resolution = Resolution::MatchInput;
    assert_eq!(sim.set_preset(changed).unwrap(), KEPT);
    assert_eq!(sim.memory_state(), &before);
}

/// The camera's codec at its still quality, without restart markers.
fn still_codec(camera: &Camera) -> codec::Params {
    let quality = camera.still.jpeg;
    codec::Params {
        quality,
        tables: camera.jpeg.tables.get(quality).clone(),
        subsampling: camera.jpeg.subsampling,
        chroma_upsample: camera.jpeg.chroma_upsample,
        restart_interval: 0,
    }
}

/// Smooth gradients with a few hard edges and a little texture.
fn picture(width: u32, height: u32) -> Frame {
    let rgb = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let edge = if (x / 40 + y / 30) % 2 == 0 { 40 } else { 0 };
                let grain = noise::hash_u32(x, y, noise::salt("picture"), 1) % 7;
                [
                    (x * 200 / width + edge + grain) as u8,
                    (y * 180 / height + 30 + grain) as u8,
                    ((x + y) * 100 / (width + height) + 90 - edge / 2) as u8,
                ]
            })
        })
        .collect();
    Frame::from_rgb(width, height, rgb)
}

/// The scan of `frame`.
fn encoded(params: &codec::Params, frame: &Frame) -> Vec<u8> {
    let mut scan = Vec::new();
    codec::encode_scan(params, &mut codec::State::default(), frame, &mut scan);
    scan
}

/// The frame `scan` decodes to at `width` by `height`.
fn decoded(params: &codec::Params, scan: &[u8], width: u32, height: u32) -> Frame {
    let mut frame = Frame::new(width, height);
    codec::decode_scan(params, &mut codec::State::default(), scan, &mut frame);
    frame
}

#[test]
fn a_data_contact_corrupts_the_decoded_picture_from_its_first_flip_and_no_fault_round_trips() {
    let camera = camera();
    let codec = still_codec(&camera);
    let (width, height) = (160, 120);
    let picture = picture(width, height);
    let scan = encoded(&codec, &picture);
    let mut direct = picture.clone();
    codec::encode(&codec, &mut codec::State::default(), &mut direct);
    let mcu_height = 8 * codec.subsampling.factors().1;
    // For each MCU row r, the length of the scan of the rows before it: the bytes
    // before offsets[r] hold every bit of those rows.
    let offsets: Vec<usize> = (0..height / mcu_height)
        .map(|r| {
            let rows = r * mcu_height;
            let bytes = (width * rows * 3) as usize;
            encoded(
                &codec,
                &Frame::from_rgb(width, rows, picture.rgb()[..bytes].to_vec()),
            )
            .len()
        })
        .collect();
    let row_bytes = (width * mcu_height * 3) as usize;

    // Without a fault the scan, and so the picture, come back exactly.
    let clean = matched(
        &camera,
        &preset(Vec::new(), Vec::new(), None),
        width,
        height,
    );
    let (_, out) = run(
        &clean,
        &mut State::default(),
        0,
        &distinct(&clean, 0),
        &scan,
    );
    assert_eq!(out, scan);
    assert_eq!(decoded(&codec, &out, width, height), direct);

    // A flaky d3 that holds clear over the first bursts: the picture holds to the first
    // flipped bit and runs out of step after it.
    let mut checked = 0;
    for seed in 0..60 {
        let mut flaky = preset(
            Vec::new(),
            vec![data_marginal(data(3, FaultKind::High), 0.5, 20.0)],
            None,
        );
        flaky.seed = seed;
        let params = matched(&camera, &flaky, width, height);
        let (_, out) = run(
            &params,
            &mut State::default(),
            0,
            &distinct(&params, 0),
            &scan,
        );
        let Some(first) = out.iter().zip(&scan).position(|(a, b)| a != b) else {
            continue;
        };
        let intact = offsets.iter().rposition(|&offset| offset <= first).unwrap();
        if !(2..offsets.len() - 2).contains(&intact) {
            continue;
        }
        let frame = decoded(&codec, &out, width, height);
        let split = intact * row_bytes;
        assert_eq!(
            frame.rgb()[..split],
            direct.rgb()[..split],
            "seed {seed}: the {intact} MCU rows before byte {first}"
        );
        assert_ne!(
            frame.rgb()[split..],
            direct.rgb()[split..],
            "seed {seed}: the rows from byte {first} on"
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "no seed held the contact clear over the first rows"
    );

    // A solid d3 destroys the scan from its first bytes.
    let stuck = matched(
        &camera,
        &preset(Vec::new(), vec![data(3, FaultKind::High)], None),
        width,
        height,
    );
    let (_, out) = run(
        &stuck,
        &mut State::default(),
        0,
        &distinct(&stuck, 0),
        &scan,
    );
    let first = out.iter().zip(&scan).position(|(a, b)| a != b).unwrap();
    assert!(first < offsets[1], "byte {first}, within the first MCU row");
    let frame = decoded(&codec, &out, width, height);
    let last = direct.rgb().len() - row_bytes;
    assert_ne!(
        frame.rgb()[last..],
        direct.rgb()[last..],
        "the last MCU row"
    );
}

#[test]
fn a_hot_board_flips_the_scans_bits_where_their_cells_decay() {
    // In the scan's own store, and laid on the raw's words by a6 stuck low.
    let camera = small();
    let scan = tagged(1500, 3);
    let input = words(&scan);
    for faults in [Vec::new(), vec![solid(6, FaultKind::Low)]] {
        let params = native(&camera, &preset(faults.clone(), Vec::new(), Some(120.0)));
        let retention = params.retention;
        assert!(!retention.is_inert());
        let mut state = State::default();
        let (_, out) = run(&params, &mut state, 0, &distinct(&params, 0), &scan);
        let out = words(&out);
        let all = (1u32 << params.faults.len()) - 1;
        let (mut flipped, mut high) = (0, 0);
        for (i, (&was, &is)) in input.iter().zip(&out).enumerate() {
            let address = params.decode(params.jpeg_base + i as u64, all);
            let decay = retention.word_decay(address);
            let (sample, raw) = ((1u16 << params.sample_bits) - 1, retention.decay(address));
            assert_eq!(
                (decay.fall & sample, decay.rise & sample),
                (raw.fall, raw.rise),
                "word {i}: the sample bits' cells are the raw's"
            );
            assert_eq!(
                u32::from(is),
                decay.apply(u32::from(was)),
                "{faults:?}: word {i} at {address}"
            );
            flipped += usize::from(is != was);
            high += usize::from((is ^ was) >> params.sample_bits != 0);
        }
        assert!(
            flipped > input.len() / 20 && high > 0,
            "{faults:?}: {flipped} of {} words flipped, {high} above the sample bits",
            input.len()
        );
    }
}

#[test]
fn on_the_movie_raw_a9_to_a11_reach_only_the_scan() {
    let camera = camera();
    for pin in [9, 10, 11] {
        let mut preset = preset(vec![solid(pin, FaultKind::High)], Vec::new(), None);
        preset.readout.mode = Mode::Movie;
        let params = native(&camera, &preset);
        let bit = params.faults[0].bits[1].unwrap();
        let end = params.address(params.pitch - 1, params.rows - 1) + 1;
        assert!(
            end <= 1 << bit,
            "a{pin}: the movie raw ends at {end}, below bit {bit}"
        );
        assert!(
            params.jpeg_base >= 1 << bit,
            "a{pin}: the scan lies above it"
        );
        let raw = distinct(&params, 0);
        let scan = tagged(5000, 0);
        let mut state = State::default();
        let (out, scan_out) = run(&params, &mut state, 0, &raw, &scan);
        assert_eq!(out, raw, "a{pin} leaves the movie raw alone");
        assert_eq!(scan_out, scan, "a{pin}: read back where it was written");
        for (i, &word) in words(&scan).iter().enumerate().step_by(97) {
            let address = (params.jpeg_base + i as u64) | 1 << bit;
            assert_eq!(state.scan_word(address), Some(word), "a{pin}: word {i}");
        }
    }
}

#[test]
fn a_jpeg_base_beyond_the_sdram_is_rejected_and_a_negative_one_does_not_parse() {
    let mut camera = camera();
    camera.validate().unwrap();
    camera.memory.jpeg_base = 1 << camera.memory.address_bits();
    let error = camera.validate().unwrap_err();
    assert!(
        error.is_config() && error.to_string().contains("memory.jpeg_base"),
        "{error}"
    );
    camera.memory.jpeg_base -= 1;
    camera.validate().unwrap();

    let text = camera_text();
    let negative = text.replace("jpeg_base = 4_194_304", "jpeg_base = -1");
    assert_ne!(negative, text);
    let error = toml::from_str::<Camera>(&negative).unwrap_err();
    assert!(error.to_string().contains("jpeg_base"), "{error}");
    let without = text
        .lines()
        .filter(|line| !line.starts_with("jpeg_base"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = toml::from_str::<Camera>(&without).unwrap_err();
    assert!(error.to_string().contains("jpeg_base"), "{error}");
}
