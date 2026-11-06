//! Properties of the JPEG simulation.

#![cfg(feature = "serde")]

use vcb_core::codec::{self, ChromaUp, Params, Quality, QuantTables, State, Subsampling};
use vcb_core::{Camera, Frame};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn unit_tables() -> QuantTables {
    QuantTables {
        luma: [[1; 8]; 8],
        chroma: [[1; 8]; 8],
    }
}

fn params(tables: QuantTables, subsampling: Subsampling, chroma_upsample: ChromaUp) -> Params {
    Params {
        quality: Quality::Normal,
        tables,
        subsampling,
        chroma_upsample,
    }
}

fn encode(params: &Params, frame: &Frame) -> Frame {
    let mut out = frame.clone();
    codec::encode(params, &mut State::default(), &mut out);
    out
}

/// A deterministic pseudo-random value in `[0, 1)` for a pixel and a channel.
fn hash(x: u32, y: u32, k: u32) -> f64 {
    let v = (f64::from(x) * 12.9898 + f64::from(y) * 78.233 + f64::from(k) * 37.719).sin();
    (v * 43_758.545).fract().abs()
}

/// A frame from a function of `(x, y)` returning `[r, g, b]` on the 8-bit scale.
fn frame(width: u32, height: u32, f: impl Fn(u32, u32) -> [f64; 3]) -> Frame {
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            rgb.extend(f(x, y).map(|v| v.round().clamp(0.0, 255.0) as u8));
        }
    }
    Frame::from_rgb(width, height, rgb)
}

/// Mid-grey texture with photo-like pixel-scale detail in every channel.
fn texture(width: u32, height: u32) -> Frame {
    frame(width, height, |x, y| {
        let base = 128.0 + 40.0 * (0.21 * f64::from(x)).sin() * (0.17 * f64::from(y)).cos();
        [0, 1, 2].map(|k| base + 24.0 * (hash(x, y, k) - 0.5))
    })
}

fn max_difference(a: &Frame, b: &Frame) -> u8 {
    a.rgb()
        .iter()
        .zip(b.rgb())
        .map(|(p, q)| p.abs_diff(*q))
        .max()
        .unwrap_or(0)
}

/// BT.601 chroma `(cb, cr)` of a pixel.
fn chroma(frame: &Frame, column: u32, row: u32) -> (f64, f64) {
    let [r, g, b] = frame.pixel(column, row).map(f64::from);
    (
        -0.168_736 * r - 0.331_264 * g + 0.5 * b,
        0.5 * r - 0.418_688 * g - 0.081_312 * b,
    )
}

/// Energy of the luma DCT coefficients with `u + v >= 10` over all aligned 8 by 8 blocks.
fn high_frequency_energy(frame: &Frame) -> f64 {
    let basis = |k: usize, n: usize| {
        let c = if k == 0 { (0.125f64).sqrt() } else { 0.5 };
        c * ((2 * n + 1) as f64 * k as f64 * std::f64::consts::PI / 16.0).cos()
    };
    let luma = |x: u32, y: u32| {
        let [r, g, b] = frame.pixel(x, y).map(f64::from);
        0.299 * r + 0.587 * g + 0.114 * b
    };
    let mut energy = 0.0;
    for by in (0..frame.height() - 7).step_by(8) {
        for bx in (0..frame.width() - 7).step_by(8) {
            for v in 0..8 {
                for u in 0..8 {
                    if u + v < 10 {
                        continue;
                    }
                    let mut f = 0.0;
                    for y in 0..8 {
                        for x in 0..8 {
                            f += basis(v, y as usize) * basis(u, x as usize) * luma(bx + x, by + y);
                        }
                    }
                    energy += f * f;
                }
            }
        }
    }
    energy
}

#[test]
fn unit_tables_without_subsampling_reproduce_the_frame() {
    let p = params(unit_tables(), Subsampling::S444, ChromaUp::Bilinear);
    for input in [
        texture(64, 48),
        texture(37, 21),
        frame(40, 24, |x, y| {
            [
                f64::from(x) * 6.0,
                f64::from(y) * 10.0,
                255.0 - f64::from(x + y) * 4.0,
            ]
        }),
    ] {
        let output = encode(&p, &input);
        let error = max_difference(&input, &output);
        assert!(
            error <= 2,
            "{}x{}: error {error}",
            input.width(),
            input.height()
        );
    }
}

#[test]
fn normal_tables_remove_the_highest_frequencies() {
    let camera = camera();
    let input = texture(64, 64);
    let unit = encode(
        &params(unit_tables(), Subsampling::S422, ChromaUp::Bilinear),
        &input,
    );
    let normal = encode(
        &params(
            camera.jpeg.tables.normal.clone(),
            camera.jpeg.subsampling,
            camera.jpeg.chroma_upsample,
        ),
        &input,
    );
    let (before, kept, after) = (
        high_frequency_energy(&input),
        high_frequency_energy(&unit),
        high_frequency_energy(&normal),
    );
    assert!(
        kept > 0.9 * before,
        "unit steps lost detail: {before:.0} -> {kept:.0}"
    );
    assert!(
        after < 0.2 * before,
        "normal tables kept {after:.0} of {before:.0}"
    );
    let superfine = encode(
        &params(
            camera.jpeg.tables.superfine.clone(),
            camera.jpeg.subsampling,
            camera.jpeg.chroma_upsample,
        ),
        &input,
    );
    let fine_energy = high_frequency_energy(&superfine);
    assert!(fine_energy > after, "superfine kept less than normal");
}

/// A grey frame crossed by a one-pixel line of colour at column `column` or row `row`.
fn chroma_line(column: Option<u32>, row: Option<u32>) -> Frame {
    frame(32, 16, |x, y| {
        if Some(x) == column || Some(y) == row {
            [180.0, 100.0, 140.0]
        } else {
            [128.0, 128.0, 128.0]
        }
    })
}

/// Columns, or rows, whose chroma moved away from grey by more than `threshold`.
fn coloured(frame: &Frame, along_rows: bool, threshold: f64) -> Vec<u32> {
    let (count, probe) = if along_rows {
        (frame.height(), frame.width() / 2 + 3)
    } else {
        (frame.width(), frame.height() / 2 + 3)
    };
    (0..count)
        .filter(|&i| {
            let (cb, cr) = if along_rows {
                chroma(frame, probe, i)
            } else {
                chroma(frame, i, probe)
            };
            cb.abs().max(cr.abs()) > threshold
        })
        .collect()
}

#[test]
fn subsampling_halves_chroma_resolution_along_its_axes() {
    let line = chroma_line(Some(10), None);
    let nearest = |s| params(unit_tables(), s, ChromaUp::Nearest);
    assert_eq!(
        coloured(&encode(&nearest(Subsampling::S444), &line), false, 5.0),
        vec![10]
    );
    // 4:2:2 averages columns 10 and 11 into one chroma sample, at half the strength.
    let wide = encode(&nearest(Subsampling::S422), &line);
    assert_eq!(coloured(&wide, false, 5.0), vec![10, 11]);
    let (cb_line, cr_line) = chroma(&line, 10, 8);
    let (cb, cr) = chroma(&wide, 11, 8);
    assert!((cb - cb_line / 2.0).abs() < 1.5 && (cr - cr_line / 2.0).abs() < 1.5);

    // 4:2:2 keeps full vertical chroma resolution; 4:2:0 halves it too.
    let row = chroma_line(None, Some(6));
    assert_eq!(
        coloured(&encode(&nearest(Subsampling::S422), &row), true, 5.0),
        vec![6]
    );
    assert_eq!(
        coloured(&encode(&nearest(Subsampling::S420), &row), true, 5.0),
        vec![6, 7]
    );

    // The decoder's triangle filter spreads the sample over its neighbours as well.
    let smooth = encode(
        &params(unit_tables(), Subsampling::S422, ChromaUp::Bilinear),
        &line,
    );
    assert_eq!(coloured(&smooth, false, 2.0), vec![9, 10, 11, 12]);
    // Luma keeps its full resolution throughout.
    let luma = |f: &Frame, x| {
        let [r, g, b] = f.pixel(x, 8).map(f64::from);
        0.299 * r + 0.587 * g + 0.114 * b
    };
    assert!((luma(&wide, 10) - luma(&line, 10)).abs() < 1.5);
    assert!((luma(&wide, 11) - luma(&line, 11)).abs() < 1.5);
}

#[test]
fn flat_frames_of_any_size_stay_flat_at_normal_quality() {
    let camera = camera();
    let p = params(
        camera.jpeg.tables.normal.clone(),
        Subsampling::S420,
        ChromaUp::Bilinear,
    );
    for (width, height) in [(13, 11), (16, 8), (1, 1), (33, 17)] {
        let input = frame(width, height, |_, _| [200.0, 90.0, 40.0]);
        let output = encode(&p, &input);
        let first = output.pixel(0, 0);
        assert!(
            output
                .rgb()
                .as_chunks::<3>()
                .0
                .iter()
                .all(|&pixel| pixel == first),
            "{width}x{height}"
        );
        assert!(
            max_difference(&input, &output) <= 2,
            "{width}x{height}: {first:?}"
        );
    }
}

#[test]
fn encoding_is_deterministic() {
    let camera = camera();
    let p = params(
        camera.jpeg.tables.normal.clone(),
        Subsampling::S422,
        ChromaUp::Bilinear,
    );
    let input = texture(48, 40);
    let mut state = State::default();
    let mut first = input.clone();
    codec::encode(&p, &mut state, &mut first);
    let mut again = input;
    codec::encode(&p, &mut state, &mut again);
    assert_eq!(first, again);
}
