//! Properties of the entropy-coded scan: an exact round trip, and libjpeg's damage.

#![cfg(feature = "serde")]

use vcb_core::codec::{self, ChromaUp, Params, Quality, State, Subsampling};
use vcb_core::{Camera, Frame};

/// Every subsampling the codec supports.
const SUBSAMPLINGS: [Subsampling; 3] = [Subsampling::S444, Subsampling::S422, Subsampling::S420];

/// Every quality setting of the camera.
const QUALITIES: [Quality; 3] = [Quality::Superfine, Quality::Fine, Quality::Normal];

/// The grey of an all-zero block after the level shift.
const GREY: [u8; 3] = [128, 128, 128];

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The camera's tables of `quality` with the given scan layout.
fn params(
    quality: Quality,
    subsampling: Subsampling,
    chroma_upsample: ChromaUp,
    restart_interval: u32,
) -> Params {
    Params {
        quality,
        tables: camera().jpeg.tables.get(quality).clone(),
        subsampling,
        chroma_upsample,
        restart_interval,
    }
}

/// The A520's own layout at fine quality: 4:2:2, the triangle filter.
fn a520(restart_interval: u32) -> Params {
    params(
        Quality::Fine,
        Subsampling::S422,
        ChromaUp::Bilinear,
        restart_interval,
    )
}

/// Saturated colour bars with hard edges under strong deterministic noise.
fn noisy(width: u32, height: u32) -> Frame {
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    let mut state = 0x9e37_79b9_u32;
    for y in 0..height {
        for x in 0..width {
            let bar = [[230, 40, 40], [30, 200, 60], [40, 50, 220], [240, 240, 240]]
                [((x / 11 + y / 7) % 4) as usize];
            for value in bar {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                rgb.push((value + (state % 61).cast_signed() - 30).clamp(0, 255) as u8);
            }
        }
    }
    Frame::from_rgb(width, height, rgb)
}

/// A frame of one colour.
fn flat(width: u32, height: u32) -> Frame {
    Frame::from_rgb(
        width,
        height,
        [180, 120, 60].repeat((width * height) as usize),
    )
}

/// The scan of `frame`.
fn scan(params: &Params, frame: &Frame) -> Vec<u8> {
    let mut scan = Vec::new();
    codec::encode_scan(params, &mut State::default(), frame, &mut scan);
    scan
}

/// The frame `scan` decodes to at the size of `like`.
fn decode(params: &Params, scan: &[u8], like: &Frame) -> Frame {
    let mut frame = Frame::new(like.width(), like.height());
    codec::decode_scan(params, &mut State::default(), scan, &mut frame);
    frame
}

/// The frame of the in-place round trip.
fn encoded(params: &Params, frame: &Frame) -> Frame {
    let mut out = frame.clone();
    codec::encode(params, &mut State::default(), &mut out);
    out
}

/// Height of an MCU row in pixels.
fn mcu_height(params: &Params) -> u32 {
    8 * params.subsampling.factors().1
}

/// The first `rows` MCU rows of `frame`.
fn crop(params: &Params, frame: &Frame, rows: u32) -> Frame {
    let height = rows * mcu_height(params);
    let bytes = (frame.width() * height * 3) as usize;
    Frame::from_rgb(frame.width(), height, frame.rgb()[..bytes].to_vec())
}

/// For each MCU row `r`, the length of the scan of the frame's first `r` MCU rows.
///
/// Those rows code to the same bits in the whole frame's scan, so every byte of the
/// whole scan at or after `offsets[r]` codes MCU row `r` or a later one, and every
/// byte before `offsets[r] - 2` an earlier one.
fn row_offsets(params: &Params, frame: &Frame) -> Vec<usize> {
    let rows = frame.height().div_ceil(mcu_height(params));
    (0..rows)
        .map(|r| scan(params, &crop(params, frame, r)).len())
        .collect()
}

/// The MCU row whose bytes contain byte `index` of the scan, by its row offsets.
fn row_of(offsets: &[usize], index: usize) -> usize {
    offsets.iter().rposition(|&offset| offset <= index).unwrap()
}

/// The pixels of MCU row `row`.
fn mcu_row<'a>(params: &Params, frame: &'a Frame, row: u32) -> &'a [u8] {
    let bytes_per_row = (frame.width() * mcu_height(params) * 3) as usize;
    let start = row as usize * bytes_per_row;
    &frame.rgb()[start..(start + bytes_per_row).min(frame.rgb().len())]
}

/// Whether every pixel of MCU rows `from` to the end is grey.
fn grey_from(params: &Params, frame: &Frame, from: u32) -> bool {
    let rows = frame.height().div_ceil(mcu_height(params));
    (from..rows).all(|r| {
        mcu_row(params, frame, r)
            .as_chunks::<3>()
            .0
            .iter()
            .all(|&p| p == GREY)
    })
}

/// The mean BT.601 luma of MCU rows `from` to the end.
fn mean_luma_from(params: &Params, frame: &Frame, from: u32) -> f64 {
    let start = (from * mcu_height(params) * frame.width() * 3) as usize;
    let pixels = frame.rgb()[start..].as_chunks::<3>().0;
    let sum: f64 = pixels
        .iter()
        .map(|&[r, g, b]| 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b))
        .sum();
    sum / pixels.len() as f64
}

/// The byte offsets of the restart markers in `scan` and their numbers.
fn markers(scan: &[u8]) -> Vec<(usize, u8)> {
    scan.windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] == 0xFF && (0xD0..=0xD7).contains(&w[1]))
        .map(|(i, w)| (i, w[1] - 0xD0))
        .collect()
}

/// The first byte at or after `from` that a flip of `bit` turns into data, not a marker.
///
/// Skips `0xFF` bytes, their stuffed zeros and bytes that the flip would make `0xFF`.
fn flippable(scan: &[u8], from: usize, bit: u32) -> usize {
    (from..scan.len())
        .find(|&i| scan[i] != 0xFF && scan[i - 1] != 0xFF && scan[i] ^ (1 << bit) != 0xFF)
        .unwrap()
}

/// The pixels of MCU `index`, MCUs in raster order.
fn mcu_pixels(params: &Params, frame: &Frame, index: u32) -> Vec<[u8; 3]> {
    let (sx, sy) = params.subsampling.factors();
    let (mcu_w, mcu_h) = (8 * sx, 8 * sy);
    let columns = frame.width().div_ceil(mcu_w);
    let (x0, y0) = ((index % columns) * mcu_w, (index / columns) * mcu_h);
    let mut pixels = Vec::with_capacity((mcu_w * mcu_h) as usize);
    for y in y0..(y0 + mcu_h).min(frame.height()) {
        for x in x0..(x0 + mcu_w).min(frame.width()) {
            pixels.push(frame.pixel(x, y));
        }
    }
    pixels
}

/// Whether MCU rows `rows` of `a` and `b` are identical.
fn same_rows(params: &Params, a: &Frame, b: &Frame, rows: std::ops::Range<u32>) -> bool {
    rows.into_iter()
        .all(|r| mcu_row(params, a, r) == mcu_row(params, b, r))
}

#[test]
fn decoding_the_scan_gives_the_frame_of_encode_for_every_subsampling_and_quality() {
    // One state and one scan buffer serve every size and layout in turn, as in a video.
    let mut state = State::default();
    let mut buffer = Vec::new();
    for subsampling in SUBSAMPLINGS {
        for quality in QUALITIES {
            let p = params(quality, subsampling, ChromaUp::Bilinear, 0);
            for input in [noisy(77, 45), flat(77, 45), noisy(64, 32), noisy(1, 1)] {
                codec::encode_scan(&p, &mut state, &input, &mut buffer);
                let mut decoded = Frame::new(input.width(), input.height());
                codec::decode_scan(&p, &mut state, &buffer, &mut decoded);
                assert!(
                    decoded == encoded(&p, &input),
                    "{subsampling:?} {quality:?} {}x{}",
                    input.width(),
                    input.height()
                );
            }
        }
    }
    let empty = Frame::new(0, 0);
    codec::encode_scan(&a520(0), &mut state, &empty, &mut buffer);
    assert_eq!(buffer, Vec::<u8>::new());
}

#[test]
fn restart_markers_count_modulo_8_and_keep_the_round_trip_exact() {
    // 77 by 45 pixels at 4:2:2: 5 by 6 MCUs of 16 by 8.
    let input = noisy(77, 45);
    let mcus = 30u32;
    for interval in [1, 3, 7, 29, 30, 31] {
        let p = a520(interval);
        let scan = scan(&p, &input);
        let numbers: Vec<u8> = markers(&scan).into_iter().map(|(_, n)| n).collect();
        let expected: Vec<u8> = (0..mcus.div_ceil(interval) - 1)
            .map(|i| (i % 8) as u8)
            .collect();
        assert_eq!(numbers, expected, "interval {interval}");
        assert!(
            decode(&p, &scan, &input) == encoded(&p, &input),
            "interval {interval}"
        );
    }
    assert_eq!(markers(&scan(&a520(0), &input)), Vec::new());
}

#[test]
fn a_flat_frame_codes_to_a_far_shorter_scan_than_a_noisy_one() {
    // 256 by 128 pixels at 4:2:2: 256 MCUs.
    let mcus = 256;
    for quality in QUALITIES {
        let p = params(quality, Subsampling::S422, ChromaUp::Bilinear, 0);
        let flat = scan(&p, &flat(256, 128)).len();
        let noisy = scan(&p, &noisy(256, 128)).len();
        // A flat MCU is four zero DC differences and four ends of block: 20 bits.
        assert!(
            flat < 3 * mcus,
            "{quality:?}: {flat} bytes for {mcus} flat MCUs"
        );
        assert!(20 * flat < noisy, "{quality:?}: flat {flat}, noisy {noisy}");
    }
}

#[test]
fn every_ff_in_the_scan_is_stuffed_or_starts_a_restart_marker() {
    let input = noisy(128, 64);
    let mut stuffed = 0;
    for quality in QUALITIES {
        for interval in [0, 5] {
            let p = params(quality, Subsampling::S422, ChromaUp::Bilinear, interval);
            let scan = scan(&p, &input);
            assert_ne!(scan.last(), Some(&0xFF), "{quality:?} interval {interval}");
            for (i, pair) in scan.windows(2).enumerate() {
                match pair {
                    [0xFF, 0x00] => stuffed += 1,
                    [0xFF, 0xD0..=0xD7] if interval != 0 => {}
                    [0xFF, code] => {
                        panic!("{quality:?} interval {interval}: 0xFF {code:#04x} at {i}")
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(stuffed > 0, "no scan coded an 0xFF byte");
}

#[test]
fn a_flipped_bit_keeps_the_mcus_before_it_and_disturbs_all_or_none_after_it() {
    let p = a520(0);
    let input = noisy(128, 96);
    let clean_scan = scan(&p, &input);
    let clean = decode(&p, &clean_scan, &input);
    let offsets = row_offsets(&p, &input);
    let rows = offsets.len() as u32;
    let row = rows / 2;
    let (start, end) = (offsets[row as usize], offsets[row as usize + 1] - 2);
    // A flip in a code or a DC difference throws the decoder out of step for the rest
    // of the scan; a flip in an AC value's extra bits changes that one coefficient.
    let (mut out_of_step, mut local) = (0, 0);
    for bit in 0..8 {
        for k in 0..8 {
            let at = flippable(&clean_scan, start + (end - start) * k / 8, bit);
            assert!(at < end, "no flippable byte in MCU row {row}");
            let mut damaged = clean_scan.clone();
            damaged[at] ^= 1 << bit;
            let out = decode(&p, &damaged, &input);
            assert!(
                same_rows(&p, &out, &clean, 0..row),
                "a flip of bit {bit} at byte {at} changed an MCU row before {row}"
            );
            let disturbed = (row + 1..rows)
                .filter(|&r| mcu_row(&p, &out, r) != mcu_row(&p, &clean, r))
                .count() as u32;
            if disturbed == rows - row - 1 {
                out_of_step += 1;
            } else {
                assert_eq!(disturbed, 0, "a flip of bit {bit} at byte {at}");
                local += 1;
            }
        }
    }
    assert!(
        out_of_step >= 16 && local >= 16,
        "{out_of_step} flips disturbed every later row, {local} none"
    );
}

#[test]
fn a_flip_that_emulates_a_marker_greys_everything_after_it() {
    let p = a520(0);
    let input = noisy(128, 96);
    let clean_scan = scan(&p, &input);
    let clean = decode(&p, &clean_scan, &input);
    let offsets = row_offsets(&p, &input);
    let rows = offsets.len() as u32;
    // A byte one bit short of 0xFF whose successor makes the pair a marker.
    let at = (clean_scan.len() / 2..clean_scan.len() - 1)
        .find(|&i| clean_scan[i].count_ones() == 7 && !matches!(clean_scan[i + 1], 0x00 | 0xFF))
        .unwrap();
    let mut damaged = clean_scan.clone();
    damaged[at] = 0xFF;
    let out = decode(&p, &damaged, &input);
    // The MCU the marker cuts short finishes from zero bits, possibly one MCU more.
    let row = row_of(&offsets, at) as u32;
    assert!(
        row + 4 <= rows,
        "the marker at byte {at} lies in MCU row {row}"
    );
    assert!(same_rows(&p, &out, &clean, 0..row));
    assert!(grey_from(&p, &out, row + 2));
    assert!((mean_luma_from(&p, &out, row + 2) - 128.0).abs() < 1e-9);
}

#[test]
fn a_scan_cut_in_half_greys_its_second_half() {
    for subsampling in SUBSAMPLINGS {
        // At 4:2:0 the triangle filter blends chroma across MCU rows; repeated chroma
        // keeps each MCU row's pixels its own.
        let p = params(Quality::Fine, subsampling, ChromaUp::Nearest, 0);
        let input = noisy(128, 192);
        let clean_scan = scan(&p, &input);
        let clean = decode(&p, &clean_scan, &input);
        let offsets = row_offsets(&p, &input);
        let rows = offsets.len() as u32;
        let cut = clean_scan.len() / 2;
        let out = decode(&p, &clean_scan[..cut], &input);
        let row = row_of(&offsets, cut) as u32;
        assert!(
            (rows / 3..2 * rows / 3).contains(&row),
            "{subsampling:?}: the middle byte lies in MCU row {row} of {rows}"
        );
        assert!(same_rows(&p, &out, &clean, 0..row), "{subsampling:?}");
        assert!(grey_from(&p, &out, row + 2), "{subsampling:?}");
    }
}

#[test]
fn with_restart_markers_a_flip_in_one_interval_leaves_the_other_intervals_identical() {
    // Repeated chroma keeps every MCU's pixels its own; 8 by 12 MCUs, 24 intervals.
    let interval = 4;
    let p = params(
        Quality::Fine,
        Subsampling::S422,
        ChromaUp::Nearest,
        interval,
    );
    let input = noisy(128, 96);
    let clean_scan = scan(&p, &input);
    let clean = decode(&p, &clean_scan, &input);
    let found = markers(&clean_scan);
    // Interval k lies between markers k - 1 and k.
    let k = found.len() / 2;
    let (start, end) = (found[k - 1].0 + 2, found[k].0);
    let mut disturbed = 0;
    for bit in 0..8 {
        let at = flippable(&clean_scan, start + (end - start) / 2, bit);
        assert!(at < end);
        let mut damaged = clean_scan.clone();
        damaged[at] ^= 1 << bit;
        let out = decode(&p, &damaged, &input);
        for mcu in 0..8 * 12 {
            let same = mcu_pixels(&p, &out, mcu) == mcu_pixels(&p, &clean, mcu);
            if (mcu / interval) as usize == k {
                disturbed += usize::from(!same);
            } else {
                assert!(
                    same,
                    "a flip of bit {bit} in interval {k} changed MCU {mcu}"
                );
            }
        }
    }
    assert!(disturbed > 0, "no flip changed interval {k}");
}

#[test]
fn a_lost_restart_marker_greys_one_interval_and_the_scan_heals() {
    let interval = 4;
    let p = params(
        Quality::Fine,
        Subsampling::S422,
        ChromaUp::Nearest,
        interval,
    );
    let input = noisy(128, 96);
    let clean_scan = scan(&p, &input);
    let clean = decode(&p, &clean_scan, &input);
    let found = markers(&clean_scan);
    // Marker k ends interval k. Without it the decoder finds RST(k + 1) where RST(k)
    // was due and leaves it unread: interval k + 1 decodes from zero bits, grey after
    // its first MCU, and the marker then comes due and is taken.
    let k = found.len() / 2;
    let mut damaged = clean_scan.clone();
    damaged.drain(found[k].0..found[k].0 + 2);
    let out = decode(&p, &damaged, &input);
    for mcu in 0..8 * 12 {
        let pixels = mcu_pixels(&p, &out, mcu);
        if (mcu / interval) as usize == k + 1 {
            if mcu % interval != 0 {
                assert!(pixels.iter().all(|&px| px == GREY), "MCU {mcu}");
            }
        } else {
            assert!(pixels == mcu_pixels(&p, &clean, mcu), "MCU {mcu}");
        }
    }
}

#[test]
fn without_a_restart_interval_a_restart_marker_ends_the_scan() {
    // Two MCU rows per interval: RST0 follows the second row.
    let input = noisy(128, 96);
    let with = a520(16);
    let scan = scan(&with, &input);
    let clean = decode(&with, &scan, &input);
    let without = a520(0);
    let out = decode(&without, &scan, &input);
    assert!(same_rows(&without, &out, &clean, 0..2));
    assert!(grey_from(&without, &out, 3));
}
