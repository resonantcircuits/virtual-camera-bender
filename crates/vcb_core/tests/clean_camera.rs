//! Physical behaviour of the clean camera: scene, sensor and image processor.

#![cfg(feature = "serde")]

use vcb_core::adc::Codes;
use vcb_core::codec::Quality;
use vcb_core::isp::{self, AeMode, Demosaic, Raw, Tone};
use vcb_core::preset::{Mode, Resolution};
use vcb_core::readout::{Direction, Layout};
use vcb_core::scene::{self, Channel};
use vcb_core::sensor::{self, Charge};
use vcb_core::{Camera, Frame, Preset, Scene, Simulator, Timing};

/// A preset whose sensor takes the scene's size, as these tests assume.
fn match_input(camera: &str) -> Preset {
    let mut preset = Preset::new(camera);
    preset.readout.resolution = Resolution::MatchInput;
    preset
}

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The A520 with neutral processing: tone (1, 0, 0), no noise reduction or sharpening,
/// no fixed-pattern noise, an even lens and unit JPEG steps. Shot noise stays.
fn neutral_camera() -> Camera {
    let mut camera = camera();
    let sensor = &mut camera.sensor;
    sensor.prnu = 0.0;
    sensor.dsnu_e = 0.0;
    sensor.dark_e_per_s = 0.0;
    sensor.hot_pixels_per_mpix = 0.0;
    sensor.read_noise_e = 0.0;
    sensor.cte = 1.0;
    camera.lens.vignette_stops = 0.0;
    let isp = &mut camera.isp;
    isp.tone_contrast = 1.0;
    isp.tone_toe = 0.0;
    isp.tone_shoulder = 0.0;
    isp.chroma_nr_frac = 0.0;
    isp.luma_nr_frac = 0.0;
    isp.sharpen_gain = 0.0;
    let tables = &mut camera.jpeg.tables;
    for pair in [&mut tables.superfine, &mut tables.fine, &mut tables.normal] {
        pair.luma = [[1; 8]; 8];
        pair.chroma = [[1; 8]; 8];
    }
    camera
}

/// A still preset that takes the input as scene radiance, without highlight headroom.
fn neutral_preset() -> Preset {
    let mut preset = match_input("a520");
    preset.seed = 1;
    preset.scene.headroom = 1.0;
    preset
}

fn render(camera: Camera, preset: Preset, scene: &Scene) -> Frame {
    let mut sim = Simulator::builder((camera, preset))
        .input_size(scene.width(), scene.height())
        .build()
        .unwrap();
    sim.frame(scene, 0).clone()
}

fn timing(camera: &Camera, preset: &Preset, width: u32, height: u32) -> Timing {
    Timing::derive(camera, preset, width, height).unwrap()
}

/// Interleaved sRGB bytes from a function of `(x, y)` returning encoded `[r, g, b]` in `[0, 1]`.
fn image(width: u32, height: u32, f: impl Fn(f64, f64) -> [f64; 3]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let pixel = f(f64::from(x), f64::from(y));
            rgb.extend(pixel.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
    }
    rgb
}

/// Smooth, moderately saturated colour waves.
fn smooth_scene(width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (f64::from(width), f64::from(height));
    let tau = std::f64::consts::TAU;
    image(width, height, |x, y| {
        [
            0.5 + 0.35 * (tau * x / w + 0.3).sin(),
            0.5 + 0.35 * (tau * y / h + 1.1).sin(),
            0.5 + 0.3 * (2.0 * tau * (x + y) / (w + h) + 2.0).sin(),
        ]
    })
}

/// Mid-tone texture with pixel-scale luma and chroma detail from a fixed hash.
fn textured_scene(width: u32, height: u32) -> Vec<u8> {
    let w = f64::from(width);
    let hash = |x: f64, y: f64, k: f64| {
        ((x * 12.9898 + y * 78.233 + k * 37.719).sin() * 43_758.545)
            .fract()
            .abs()
    };
    image(width, height, |x, y| {
        let base = 0.35 + 0.25 * (x / w) + 0.2 * (0.15 * x).sin() * (0.11 * y).cos();
        let luma = 0.12 * (hash(x, y, 1.0) - 0.5);
        let tint = [hash(x, y, 2.0), hash(x, y, 3.0), hash(x, y, 4.0)].map(|t| 0.10 * (t - 0.5));
        [
            base + luma + tint[0],
            base + luma + tint[1],
            base + luma + tint[2],
        ]
    })
}

fn uniform(width: u32, height: u32, rgb: [u8; 3]) -> Scene {
    let bytes: Vec<u8> = (0..width * height).flat_map(|_| rgb).collect();
    Scene::from_srgb8(width, height, &bytes)
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse = a
        .iter()
        .zip(b)
        .map(|(&p, &q)| (f64::from(p) - f64::from(q)).powi(2))
        .sum::<f64>()
        / a.len() as f64;
    10.0 * (255.0f64.powi(2) / mse.max(1e-12)).log10()
}

/// BT.601 luma and chroma planes of interleaved RGB bytes.
fn ycc(rgb: &[u8]) -> [Vec<f64>; 3] {
    let mut planes = [Vec::new(), Vec::new(), Vec::new()];
    for p in rgb.chunks_exact(3) {
        let [r, g, b] = [p[0], p[1], p[2]].map(f64::from);
        planes[0].push(0.299 * r + 0.587 * g + 0.114 * b);
        planes[1].push(-0.168_736 * r - 0.331_264 * g + 0.5 * b);
        planes[2].push(0.5 * r - 0.418_688 * g - 0.081_312 * b);
    }
    planes
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn std_dev(values: &[f64]) -> f64 {
    let m = mean(values);
    (values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
}

/// Mean standard deviation within 16 by 16 tiles: contrast without slow gradients.
fn local_contrast(plane: &[f64], width: usize) -> f64 {
    let height = plane.len() / width;
    let mut deviations = Vec::new();
    for ty in (0..height - 15).step_by(16) {
        for tx in (0..width - 15).step_by(16) {
            let tile: Vec<f64> = (0..16)
                .flat_map(|dy| (0..16).map(move |dx| (ty + dy) * width + tx + dx))
                .map(|i| plane[i])
                .collect();
            deviations.push(std_dev(&tile));
        }
    }
    mean(&deviations)
}

/// Energy of horizontal pixel-to-pixel differences.
fn horizontal_detail(plane: &[f64], width: usize) -> f64 {
    plane
        .chunks_exact(width)
        .flat_map(|row| row.windows(2).map(|pair| (pair[1] - pair[0]).powi(2)))
        .sum()
}

/// Fraction of the high-frequency DCT coefficients (u or v at least 4) below 1 in
/// magnitude, over the 8 by 8 blocks of a grid that starts at `offset`.
fn empty_coefficients(plane: &[f64], width: usize, offset: usize) -> f64 {
    let height = plane.len() / width;
    let basis = |k: usize, n: usize| {
        let c = if k == 0 { (0.125f64).sqrt() } else { 0.5 };
        c * ((2 * n + 1) as f64 * k as f64 * std::f64::consts::PI / 16.0).cos()
    };
    let (mut empty, mut total) = (0usize, 0usize);
    for by in (offset..height - 7).step_by(8) {
        for bx in (offset..width - 7).step_by(8) {
            for v in 0..8 {
                for u in 0..8 {
                    if u < 4 && v < 4 {
                        continue;
                    }
                    let mut f = 0.0;
                    for y in 0..8 {
                        for x in 0..8 {
                            f += basis(v, y) * basis(u, x) * plane[(by + y) * width + bx + x];
                        }
                    }
                    total += 1;
                    empty += usize::from(f.abs() < 1.0);
                }
            }
        }
    }
    empty as f64 / total as f64
}

#[test]
fn neutral_camera_round_trips_a_smooth_scene() {
    let (width, height) = (256, 192);
    let input = smooth_scene(width, height);
    let scene = Scene::from_srgb8(width, height, &input);
    for (method, floor) in [(Demosaic::Malvar, 38.0), (Demosaic::Bilinear, 35.0)] {
        let mut camera = neutral_camera();
        camera.isp.demosaic = method;
        let frame = render(camera, neutral_preset(), &scene);
        let quality = psnr(&input, frame.rgb());
        assert!(quality > floor, "{method:?}: PSNR {quality:.2} dB");
    }
}

#[test]
fn neutral_camera_keeps_flat_fields_flat() {
    let (width, height) = (128, 96);
    for colour in [[128, 128, 128], [200, 120, 60]] {
        for method in [Demosaic::Malvar, Demosaic::Bilinear] {
            let mut camera = neutral_camera();
            camera.isp.demosaic = method;
            let frame = render(camera, neutral_preset(), &uniform(width, height, colour));
            for (c, &want) in colour.iter().enumerate() {
                let channel: Vec<f64> = frame
                    .rgb()
                    .iter()
                    .skip(c)
                    .step_by(3)
                    .map(|&v| f64::from(v))
                    .collect();
                let m = mean(&channel);
                assert!(
                    (m - f64::from(want)).abs() < 1.0,
                    "{method:?} {colour:?}: channel {c} mean {m:.2}"
                );
                assert!(
                    std_dev(&channel) < 3.0,
                    "{method:?} {colour:?}: channel {c} is grainy"
                );
                // No mosaic pattern: the four Bayer phases develop to the same level.
                for phase in 0..4 {
                    let (px, py) = (phase % 2, phase / 2);
                    let sites: Vec<f64> = (0..height as usize)
                        .filter(|y| y % 2 == py)
                        .flat_map(|y| {
                            (0..width as usize)
                                .filter(move |x| x % 2 == px)
                                .map(move |x| y * width as usize + x)
                        })
                        .map(|i| channel[i])
                        .collect();
                    let phase_mean = mean(&sites);
                    assert!(
                        (phase_mean - m).abs() < 0.3,
                        "{method:?} {colour:?}: Bayer phase {phase} of channel {c} at {phase_mean:.2}, frame {m:.2}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_a520_adds_contrast_softens_chroma_and_blocks_at_normal_quality() {
    let (width, height) = (256, 192);
    let input = textured_scene(width, height);
    let scene = Scene::from_srgb8(width, height, &input);
    let mut camera = camera();
    camera.still.jpeg = Quality::Normal;
    let mut preset = match_input("a520");
    preset.seed = 1;
    let frame = render(camera, preset, &scene);
    let w = width as usize;
    let [luma_in, cb_in, cr_in] = ycc(&input);
    let [luma_out, cb_out, cr_out] = ycc(frame.rgb());

    let (before, after) = (local_contrast(&luma_in, w), local_contrast(&luma_out, w));
    assert!(
        after > 1.05 * before,
        "local luma contrast {before:.2} -> {after:.2}"
    );

    let chroma_in = horizontal_detail(&cb_in, w) + horizontal_detail(&cr_in, w);
    let chroma_out = horizontal_detail(&cb_out, w) + horizontal_detail(&cr_out, w);
    assert!(
        chroma_out < 0.5 * chroma_in,
        "chroma detail {chroma_in:.0} -> {chroma_out:.0}"
    );

    // JPEG quantisation empties high frequencies of the blocks it coded, not of blocks
    // straddling its grid.
    let (aligned, shifted) = (
        empty_coefficients(&luma_out, w, 0),
        empty_coefficients(&luma_out, w, 4),
    );
    assert!(
        empty_coefficients(&luma_in, w, 0) < 0.3,
        "the input is already quantised"
    );
    assert!(
        aligned > 0.5 && aligned > 2.0 * shifted,
        "empty high frequencies: aligned {aligned:.2}, shifted {shifted:.2}"
    );
}

/// Sensor parameters of a camera at a size, with the fixed pattern and dark signal off.
fn quiet_sensor(camera: &Camera, preset: &Preset, width: u32, height: u32) -> sensor::Params {
    let t = timing(camera, preset, width, height);
    let mut params = sensor::Params::resolve(camera, preset, &t).unwrap();
    params.prnu = 0.0;
    params.dsnu_e = 0.0;
    params.dark_e_per_s = 0.0;
    params.hot_pixels_per_mpix = 0.0;
    params
}

fn expose(params: &sensor::Params, scene: &Scene, index: u64) -> Charge {
    let mut charge = Charge::new(scene.width(), scene.height());
    sensor::expose(
        params,
        &mut sensor::State::default(),
        scene,
        index,
        &mut charge,
    );
    charge
}

/// A scene whose every channel collects `electrons` per photosite.
fn flat_charge_scene(params: &sensor::Params, width: u32, height: u32, electrons: f32) -> Scene {
    let mut scene = Scene::new(width, height);
    for plane in scene.planes_mut() {
        plane.fill(electrons / (params.white_fraction * params.full_well_e));
    }
    scene
}

fn plane_stats(charge: &Charge) -> (f64, f64) {
    let values: Vec<f64> = charge
        .plane()
        .data()
        .iter()
        .map(|&v| f64::from(v))
        .collect();
    (mean(&values), std_dev(&values))
}

#[test]
fn shot_noise_has_the_variance_of_the_charge() {
    let camera = camera();
    let preset = match_input("a520");
    let params = quiet_sensor(&camera, &preset, 128, 128);
    let low = expose(&params, &flat_charge_scene(&params, 128, 128, 1000.0), 0);
    let (low_mean, low_std) = plane_stats(&low);
    assert!((low_mean - 1000.0).abs() < 2.0, "mean {low_mean:.1}");
    assert!(
        (low_std / 1000f64.sqrt() - 1.0).abs() < 0.1,
        "std {low_std:.2} at 1000 e"
    );
    let high = expose(&params, &flat_charge_scene(&params, 128, 128, 16_000.0), 0);
    let (high_mean, high_std) = plane_stats(&high);
    assert!(
        high_std / high_mean < 0.5 * low_std / low_mean,
        "relative noise does not fall with charge"
    );
    let next = expose(&params, &flat_charge_scene(&params, 128, 128, 1000.0), 1);
    assert_ne!(
        next.plane().data(),
        low.plane().data(),
        "shot noise repeats between frames"
    );
}

/// Positions of the photosites holding more than `threshold` electrons.
fn bright_sites(charge: &Charge, threshold: f32) -> Vec<(u32, u32)> {
    let plane = charge.plane();
    (0..plane.height())
        .flat_map(|y| (0..plane.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| plane.get(x, y) > threshold)
        .collect()
}

fn correlation(a: &Charge, b: &Charge) -> f64 {
    let a: Vec<f64> = a.plane().data().iter().map(|&v| f64::from(v)).collect();
    let b: Vec<f64> = b.plane().data().iter().map(|&v| f64::from(v)).collect();
    let (ma, mb) = (mean(&a), mean(&b));
    let cov: f64 = a.iter().zip(&b).map(|(x, y)| (x - ma) * (y - mb)).sum();
    cov / (std_dev(&a) * std_dev(&b) * a.len() as f64)
}

#[test]
fn hot_pixels_and_prnu_belong_to_the_unit_not_the_frame_or_seed() {
    let (width, height) = (256, 192);
    let mut camera = camera();
    camera.sensor.hot_pixels_per_mpix = 2000.0;
    camera.sensor.prnu = 0.0;
    camera.still.exposure_s = 10.0;
    let preset = match_input("a520");
    let t = timing(&camera, &preset, width, height);
    let base = sensor::Params::resolve(&camera, &preset, &t).unwrap();
    let dark = Scene::new(width, height);
    // Ten seconds at 1 e/s leave ordinary photosites near 10 e; hot ones hold 500 e or more.
    let hot = bright_sites(&expose(&base, &dark, 0), 200.0);
    assert!(hot.len() > 40, "only {} hot photosites", hot.len());
    let mut reseeded = base.clone();
    reseeded.seed ^= 0x5eed;
    assert_eq!(bright_sites(&expose(&base, &dark, 7), 200.0), hot);
    assert_eq!(bright_sites(&expose(&reseeded, &dark, 0), 200.0), hot);
    let mut other_unit = base.clone();
    other_unit.unit_seed += 1;
    let other = bright_sites(&expose(&other_unit, &dark, 0), 200.0);
    assert!(!other.is_empty() && other != hot);

    let mut prnu = base.clone();
    prnu.prnu = 0.05;
    prnu.hot_pixels_per_mpix = 0.0;
    let lit = flat_charge_scene(&prnu, width, height, 8000.0);
    let first = expose(&prnu, &lit, 0);
    let mut later = prnu.clone();
    later.seed ^= 0x5eed;
    assert!(correlation(&first, &expose(&later, &lit, 3)) > 0.9);
    let mut other_unit = prnu.clone();
    other_unit.unit_seed += 1;
    assert!(correlation(&first, &expose(&other_unit, &lit, 0)).abs() < 0.1);
}

#[test]
fn movie_smear_lifts_the_columns_of_a_bright_bar_and_stills_do_not_smear() {
    let (width, height) = (256, 192);
    let (bar_x, bar_y) = (100..140u32, 40..60u32);
    let radiance = 50.0f32;
    let mut scene = Scene::new(width, height);
    for plane in scene.planes_mut() {
        for y in bar_y.clone() {
            for x in bar_x.clone() {
                plane.set(x, y, radiance);
            }
        }
    }
    let mut camera = camera();
    camera.sensor.smear_db = -40.0;
    let mut preset = match_input("a520");
    preset.readout.mode = Mode::Movie;
    let t = timing(&camera, &preset, width, height);
    let params = quiet_sensor(&camera, &preset, width, height);
    assert!(params.smear, "movie readout uses the electronic shutter");
    let charge = expose(&params, &scene, 0);

    // V/10 smear: the leak ratio over a window a tenth of the height, scaled by the
    // time a packet takes to cross the array relative to the exposure.
    let electrons = f64::from(radiance) * camera.sensor.white_fraction * camera.sensor.full_well_e;
    let column_mean = electrons * bar_y.len() as f64 / f64::from(height);
    let readout_s = f64::from(t.rows_read) * t.line_period_s;
    let expected = 10f64.powf(-40.0 / 20.0) / 0.1 * column_mean * readout_s * camera.movie.fps;
    for x in 0..width {
        let in_bar = bar_x.contains(&x);
        let column: Vec<f64> = (0..height)
            .filter(|y| !bar_y.contains(y))
            .map(|y| f64::from(charge.plane().get(x, y)))
            .collect();
        if in_bar {
            let m = mean(&column);
            assert!(
                (m / expected - 1.0).abs() < 0.01,
                "column {x}: {m:.1} e, expected {expected:.1}"
            );
            assert!(
                column
                    .iter()
                    .all(|v| (v - expected).abs() < 6.0 * expected.sqrt()),
                "column {x} is not uniform"
            );
        } else {
            assert!(
                column.iter().all(|&v| v == 0.0),
                "column {x} picked up smear"
            );
        }
    }

    let still = quiet_sensor(&camera, &match_input("a520"), width, height);
    assert!(
        !still.smear,
        "the mechanical shutter closes during the still readout"
    );
    let charge = expose(&still, &scene, 0);
    for x in bar_x {
        assert!(
            (0..height)
                .filter(|y| !bar_y.contains(y))
                .all(|y| charge.plane().get(x, y) == 0.0)
        );
    }
}

#[test]
fn only_gross_overexposure_blooms_and_only_along_the_column() {
    let (width, height) = (32, 64);
    let mut camera = camera();
    camera.sensor.antibloom_ratio = 100.0;
    let preset = match_input("a520");
    let params = quiet_sensor(&camera, &preset, width, height);
    let full_well = params.full_well_e;
    let point = |overexposure: f32| {
        let mut scene = Scene::new(width, height);
        for plane in scene.planes_mut() {
            plane.set(16, 32, overexposure / params.white_fraction);
        }
        expose(&params, &scene, 0)
    };

    let mild = point(10.0);
    assert!((mild.plane().get(16, 32) - full_well).abs() < 1.0);
    let lit = bright_sites(&mild, 0.0);
    assert_eq!(
        lit,
        vec![(16, 32)],
        "a point 10 times over full well spilled"
    );

    let gross = point(1000.0);
    for dy in 1..=4 {
        for y in [32 - dy, 32 + dy] {
            assert!(
                (gross.plane().get(16, y) - full_well).abs() < 1.0,
                "row {y} of the column is not filled"
            );
        }
    }
    assert!(
        bright_sites(&gross, 0.0).iter().all(|&(x, _)| x == 16),
        "charge left its column"
    );
    assert!(
        gross.plane().get(16, 32 - 12) == 0.0 && gross.plane().get(16, 32 + 12) == 0.0,
        "the spill was not absorbed"
    );
}

/// Scene parameters for an even lens and the preset's highlight reconstruction.
fn even_lens_scene(width: u32, height: u32) -> (scene::Params, Preset) {
    let mut camera = camera();
    camera.lens.vignette_stops = 0.0;
    let preset = match_input("a520");
    let t = timing(&camera, &preset, width, height);
    (
        scene::Params::resolve(&camera, &preset, &t).unwrap(),
        preset,
    )
}

#[test]
fn highlight_reconstruction_expands_only_above_the_shoulder() {
    let (params, preset) = even_lens_scene(256, 2);
    let codes: Vec<u8> = (0..2)
        .flat_map(|_| (0..=255u8).flat_map(|v| [v, v, v]))
        .collect();
    let input = Scene::from_srgb8(256, 2, &codes);
    let mut out = Scene::new(256, 2);
    scene::prepare(&params, &input, &mut out);
    // Grey stays grey in the sensor's primaries up to the white-balance gain of green, 1.
    let before = input.plane(Channel::Green).row(0);
    let after = out.plane(Channel::Green).row(0);
    let start = preset.scene.shoulder_start as f32;
    for (code, (&v, &e)) in before.iter().zip(after).enumerate() {
        if v <= start {
            assert!(
                (e - v).abs() < 1e-6,
                "code {code} changed below the shoulder"
            );
        }
    }
    assert!(
        after.windows(2).all(|pair| pair[1] >= pair[0]),
        "the expansion is not monotonic"
    );
    let white = after[255];
    assert!(
        (white - preset.scene.headroom as f32).abs() < 1e-3,
        "clipped white maps to {white}"
    );
    assert!(
        after[250] > 1.0 && after[250] < white,
        "the shoulder is not soft: {}",
        after[250]
    );
}

#[test]
fn a_partly_clipped_sky_keeps_the_colour_of_the_sky_beside_it() {
    let (width, height) = (64, 32);
    let (params, _) = even_lens_scene(width, height);
    // Left: bright but unclipped sky. Right: brighter sky whose blue channel clipped.
    let codes: Vec<u8> = (0..height)
        .flat_map(|_| {
            (0..width).flat_map(|x| {
                if x < width / 2 {
                    [150, 200, 250]
                } else {
                    [170, 225, 255]
                }
            })
        })
        .collect();
    let input = Scene::from_srgb8(width, height, &codes);
    let mut out = Scene::new(width, height);
    scene::prepare(&params, &input, &mut out);
    let colour = |scene: &Scene, x: u32| {
        let rgb = Channel::ALL.map(|c| f64::from(scene.plane(c).get(x, height / 2)));
        let sum: f64 = rgb.iter().sum();
        rgb.map(|v| v / sum)
    };
    let gap = |a: [f64; 3], b: [f64; 3]| {
        a.iter()
            .zip(&b)
            .map(|(p, q)| (p - q).abs())
            .fold(0.0, f64::max)
    };
    assert!(
        gap(colour(&input, 8), colour(&input, 56)) > 0.03,
        "the input skies already match"
    );
    for x in [40, 48, 56, 63] {
        let g = gap(colour(&out, 8), colour(&out, x));
        assert!(
            g < 0.01,
            "column {x} differs in colour from the unclipped sky by {g:.4}"
        );
    }
    let brightness = |x| {
        Channel::ALL
            .map(|c| out.plane(c).get(x, height / 2))
            .iter()
            .sum::<f32>()
    };
    assert!(
        brightness(56) > brightness(8),
        "the clipped sky is not brighter"
    );
}

#[test]
fn the_lens_darkens_the_corners_by_its_falloff_in_stops() {
    let (width, height) = (200, 150);
    let camera = camera();
    let mut preset = match_input("a520");
    preset.scene.headroom = 1.0;
    let t = timing(&camera, &preset, width, height);
    let params = scene::Params::resolve(&camera, &preset, &t).unwrap();
    let mut out = Scene::new(width, height);
    scene::prepare(&params, &uniform(width, height, [118, 118, 118]), &mut out);
    let green = out.plane(Channel::Green);
    let centre = green.get(width / 2, height / 2);
    let corner = green.get(0, 0);
    let stops = (centre / corner).log2();
    assert!(
        (stops - camera.lens.vignette_stops as f32).abs() < 0.03,
        "corner falls off {stops:.3} stops"
    );
    assert!(green.get(width / 4, height / 2) > corner && green.get(width / 4, height / 2) < centre);
}

#[test]
fn tone_curve_is_neutral_at_unity_and_shapes_the_camera_rendering() {
    let white = 1.43;
    let neutral = Tone {
        contrast: 1.0,
        toe: 0.0,
        shoulder: 0.0,
        white,
    };
    for i in 0..=1000 {
        let x = i as f32 * 0.0015;
        assert_eq!(
            neutral.apply(x).to_bits(),
            x.to_bits(),
            "{x} is not kept exactly"
        );
    }
    let canon = Tone {
        contrast: 1.15,
        toe: 0.02,
        shoulder: 0.10,
        white,
    };
    let slope = |tone: &Tone, x: f32| (tone.apply(x + 1e-3) - tone.apply(x - 1e-3)) / 2e-3;
    let mid = slope(&canon, 0.18);
    assert!(mid > 1.1 && mid < 1.2, "slope at middle grey {mid:.3}");
    assert!((canon.apply(0.18) - 0.18).abs() < 1e-3, "middle grey moved");
    assert!(
        canon.apply(0.01) < 0.01 * 0.8,
        "the toe does not deepen the shadows"
    );
    let samples: Vec<f32> = (0..=2000)
        .map(|i| canon.apply(white * i as f32 / 2000.0))
        .collect();
    assert!(
        samples.windows(2).all(|pair| pair[1] > pair[0]),
        "not monotonic"
    );
    assert!(
        samples[..2000].iter().all(|&y| y < 1.0),
        "the shoulder lets highlights clip early"
    );
    assert!(
        (samples[2000] - 1.0).abs() < 1e-5,
        "saturation does not reach white"
    );
}

/// A raw plane whose sites hold an achromatic linear image `level(x)` under daylight balance.
fn achromatic_raw(
    params: &isp::Params,
    width: u32,
    height: u32,
    level: impl Fn(u32) -> f32,
) -> Raw {
    let isp::Wb::Daylight { r_gain, b_gain } = params.wb else {
        panic!("the A520 file uses daylight balance");
    };
    let mut raw = Raw::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let gain = [r_gain, 1.0, b_gain][params.cfa.channel(x, y).index()];
            raw.plane_mut()
                .set(x, y, level(x) * params.white_code / gain);
        }
    }
    raw
}

fn develop(camera: &Camera, method: Demosaic, raw: &Raw) -> Frame {
    let preset = neutral_preset();
    let (width, height) = (raw.plane().width(), raw.plane().height());
    let mut params =
        isp::Params::resolve(camera, &preset, &timing(camera, &preset, width, height)).unwrap();
    params.demosaic = method;
    let mut frame = Frame::new(width, height);
    isp::develop(&params, &mut isp::State::default(), raw, &mut frame);
    frame
}

#[test]
fn demosaic_keeps_flat_fields_flat_and_malvar_cuts_edge_colour() {
    let (width, height) = (64, 32);
    let camera = neutral_camera();
    let preset = neutral_preset();
    let params =
        isp::Params::resolve(&camera, &preset, &timing(&camera, &preset, width, height)).unwrap();
    for method in [Demosaic::Bilinear, Demosaic::Malvar] {
        let frame = develop(
            &camera,
            method,
            &achromatic_raw(&params, width, height, |_| 0.3),
        );
        let first = frame.pixel(0, 0);
        assert!(
            frame.rgb().chunks_exact(3).all(|p| p == first),
            "{method:?} flat field is not flat"
        );
        assert!(
            first.iter().all(|&c| c == first[0]),
            "{method:?} grey turned {first:?}"
        );
    }
    let false_colour = |method| {
        (31..=32)
            .map(|edge| {
                let raw =
                    achromatic_raw(
                        &params,
                        width,
                        height,
                        |x| if x < edge { 0.12 } else { 0.45 },
                    );
                let frame = develop(&camera, method, &raw);
                frame
                    .rgb()
                    .chunks_exact(3)
                    .map(|p| {
                        let [r, g, b] = [p[0], p[1], p[2]].map(f64::from);
                        (r - g).powi(2) + (b - g).powi(2)
                    })
                    .sum::<f64>()
            })
            .sum::<f64>()
    };
    let (bilinear, malvar) = (
        false_colour(Demosaic::Bilinear),
        false_colour(Demosaic::Malvar),
    );
    assert!(bilinear > 0.0, "bilinear leaves no colour on the edge");
    assert!(
        malvar < 0.7 * bilinear,
        "edge colour: bilinear {bilinear:.0}, malvar {malvar:.0}"
    );
}

#[test]
fn black_clamp_follows_each_line_with_a_loop_of_a_few_lines() {
    let camera = camera();
    let preset = match_input("a520");
    let t = timing(&camera, &preset, 160, 120);
    let params = isp::Params::resolve(&camera, &preset, &t).unwrap();
    let shift = camera.adc.dsp_lsb;
    let layout = Layout::from_timing(&t, Direction::TopDown);
    let (ob, line_len) = (layout.ob_cols() as usize, layout.line_len() as usize);
    let mut codes = Codes::new(layout.clone());
    for (line, words) in codes.words_mut().chunks_exact_mut(line_len).enumerate() {
        let black: u16 = if line < 50 { 30 } else { 40 };
        words[..ob].fill(black << shift);
        words[ob..].fill(500 << shift);
    }
    let mut raw = Raw::new(layout.cols(), layout.rows());
    isp::clamp_black(&params, &codes, &mut raw);
    let levels = raw.black_levels();
    assert!(levels[..50].iter().all(|&b| (b - 30.0).abs() < 1e-6));
    assert!(
        levels[50..].windows(2).all(|pair| pair[1] >= pair[0]),
        "the loop does not settle monotonically"
    );
    let after_tau = levels[53];
    assert!(
        after_tau > 35.5 && after_tau < 37.5,
        "after four lines the level is {after_tau:.2}"
    );
    assert!((levels[80] - 40.0).abs() < 0.1);
    for row in [10u32, 53, 100] {
        let level = levels[row as usize];
        assert!((raw.plane().get(5, row) - (500.0 - level)).abs() < 1e-3);
    }
}

#[test]
fn scene_metering_exposes_the_mean_luminance_at_the_target() {
    let (width, height) = (64, 48);
    let mut camera = camera();
    camera.isp.ae_mode = AeMode::Scene;
    camera.lens.vignette_stops = 0.0;
    let preset = match_input("a520");
    let t = timing(&camera, &preset, width, height);
    let scene_params = scene::Params::resolve(&camera, &preset, &t).unwrap();
    let params = sensor::Params::resolve(&camera, &preset, &t).unwrap();
    let meter = |level: f32| {
        let mut input = Scene::new(width, height);
        for plane in input.planes_mut() {
            plane.fill(level);
        }
        let mut prepared = Scene::new(width, height);
        scene::prepare(&scene_params, &input, &mut prepared);
        let mut state = sensor::State::default();
        let mut charge = Charge::new(width, height);
        sensor::expose(&params, &mut state, &prepared, 0, &mut charge);
        state.exposure_scale()
    };
    let target = camera.isp.ae_target as f32;
    let scale = meter(0.06);
    assert!(
        (scale * 0.06 / target - 1.0).abs() < 0.01,
        "a 6 % grey is exposed by {scale:.3}"
    );
    assert!(
        (meter(0.001) - 4.0).abs() < 1e-6,
        "darkness is not clamped at two stops over"
    );
    assert!((meter(0.5) - target / 0.5).abs() < 0.01);

    camera.isp.ae_mode = AeMode::Fixed;
    let fixed = sensor::Params::resolve(&camera, &preset, &t).unwrap();
    let mut state = sensor::State::default();
    let mut charge = Charge::new(width, height);
    sensor::expose(
        &fixed,
        &mut state,
        &Scene::new(width, height),
        0,
        &mut charge,
    );
    assert!((state.exposure_scale() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn the_same_frames_from_reset_are_identical_and_frames_differ_in_grain() {
    let (width, height) = (96, 64);
    let scene = Scene::from_srgb8(width, height, &textured_scene(width, height));
    let mut preset = match_input("a520");
    preset.seed = 3;
    let mut sim = Simulator::builder((camera(), preset.clone()))
        .input_size(width, height)
        .build()
        .unwrap();
    let first: Vec<Frame> = (0..3).map(|i| sim.frame(&scene, i).clone()).collect();
    assert_ne!(
        first[0], first[1],
        "temporal noise did not change between frames"
    );
    sim.reset();
    let again: Vec<Frame> = (0..3).map(|i| sim.frame(&scene, i).clone()).collect();
    assert_eq!(again, first);
    let mut fresh = Simulator::builder((camera(), preset))
        .input_size(width, height)
        .build()
        .unwrap();
    assert_eq!(fresh.frame(&scene, 2), &first[2]);
}
