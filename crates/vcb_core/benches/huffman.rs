//! Timings of the JPEG scan on a native A520 still: the entropy coder and the decoder.
//!
//! The benches code one 2272 by 1704 frame, saturated colour bars with hard edges
//! under strong pixel noise, the scan's worst case, through the camera's own layout
//! (`cameras/a520.toml`: 4:2:2, the triangle filter) and its tables at each quality
//! setting. `encode` times the in-place round trip of [`codec::encode`] for reference;
//! `encode_scan` times its first half followed by the entropy coder, and `decode_scan`
//! the entropy decoder followed by its second half, so the coder's own cost is what
//! the two add to `encode`. The scan buffer, the frame and the codec state are reused
//! from iteration to iteration, as in a video; `encode` gets a fresh copy of the frame
//! for every iteration, made outside the timed region.
//!
//! Run with `cargo bench -p vcb_core --bench huffman`.

use std::fs;
use std::path::Path;

use divan::Bencher;
use vcb_core::codec::{self, Params, Quality, State};
use vcb_core::{Camera, Frame};

fn main() {
    divan::main();
}

/// Quality settings benchmarked, as named in the camera file.
const QUALITIES: [&str; 3] = ["superfine", "fine", "normal"];

/// Width of the frame: the A520's native still width.
const WIDTH: u32 = 2272;

/// Height of the frame: the A520's native still height.
const HEIGHT: u32 = 1704;

/// The camera's codec parameters with the tables of `quality`, without restart markers.
fn params(quality: &str) -> Params {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cameras/a520.toml");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let camera: Camera = toml::from_str(&text)
        .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()));
    let quality = match quality {
        "superfine" => Quality::Superfine,
        "fine" => Quality::Fine,
        "normal" => Quality::Normal,
        other => panic!("unknown quality {other}"),
    };
    Params {
        quality,
        tables: camera.jpeg.tables.get(quality).clone(),
        subsampling: camera.jpeg.subsampling,
        chroma_upsample: camera.jpeg.chroma_upsample,
        restart_interval: 0,
    }
}

/// Saturated colour bars with hard edges under strong deterministic noise.
fn noisy() -> Frame {
    let mut rgb = Vec::with_capacity(WIDTH as usize * HEIGHT as usize * 3);
    let mut state = 0x9e37_79b9_u32;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
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
    Frame::from_rgb(WIDTH, HEIGHT, rgb)
}

/// The in-place round trip, for reference.
#[divan::bench(args = QUALITIES, sample_size = 1)]
fn encode(bencher: Bencher<'_, '_>, quality: &str) {
    let params = params(quality);
    let input = noisy();
    let mut state = State::default();
    bencher
        .with_inputs(|| input.clone())
        .bench_local_refs(|frame| codec::encode(&params, &mut state, frame));
}

/// Quantisation and the entropy coder, into a reused scan buffer.
#[divan::bench(args = QUALITIES)]
fn encode_scan(bencher: Bencher<'_, '_>, quality: &str) {
    let params = params(quality);
    let input = noisy();
    let mut state = State::default();
    let mut scan = Vec::new();
    bencher.bench_local(|| codec::encode_scan(&params, &mut state, &input, &mut scan));
}

/// The entropy decoder and the decoder's reconstruction, into a reused frame.
#[divan::bench(args = QUALITIES)]
fn decode_scan(bencher: Bencher<'_, '_>, quality: &str) {
    let params = params(quality);
    let input = noisy();
    let mut state = State::default();
    let mut scan = Vec::new();
    codec::encode_scan(&params, &mut state, &input, &mut scan);
    let mut frame = Frame::new(WIDTH, HEIGHT);
    bencher.bench_local(|| codec::decode_scan(&params, &mut state, &scan, &mut frame));
}
