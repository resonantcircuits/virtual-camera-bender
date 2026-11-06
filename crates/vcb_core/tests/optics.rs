//! The IR-cut filter slid off the sensor, `[bend.optics]`: the scene's estimated near
//! infrared, its defocus, the filter's shadow, the charge it adds through the dyes, and
//! the camera's reaction (auto-exposure, white balance).

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::isp::{AeMode, WbMode};
use vcb_core::preset::{IspOverride, Mode, OpticsBend, Resolution};
use vcb_core::scene::{self, Channel, Optics};
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

/// Mid-grey in 8-bit sRGB: 18 % reflectance.
const GREY: [u8; 3] = [118, 118, 118];

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A still at the scene's size with the camera's processing and seed 1.
fn still() -> Preset {
    let mut preset = Preset::new("a520");
    preset.seed = 1;
    preset.readout.mode = Mode::Still;
    preset.readout.resolution = Resolution::MatchInput;
    preset
}

/// `preset` with `[bend.optics]` set to `bend`.
fn with_optics(mut preset: Preset, bend: OpticsBend) -> Preset {
    preset.bend.optics = Some(bend);
    preset
}

/// The filter slid off the top `frac` of the frame, with the default estimate.
fn pulled(frac: f64) -> OpticsBend {
    OpticsBend {
        ir_cut_pulled_frac: frac,
        ..OpticsBend::default()
    }
}

/// `preset` with its `[isp]` section replaced by `isp`.
fn with_isp(mut preset: Preset, isp: IspOverride) -> Preset {
    preset.isp = Some(isp);
    preset
}

/// The resolved scene parameters of `preset` for a `width` by `height` input.
fn scene_params(camera: &Camera, preset: &Preset, width: u32, height: u32) -> scene::Params {
    let timing = Timing::derive(camera, preset, width, height).unwrap();
    scene::Params::resolve(camera, preset, &timing).unwrap()
}

/// The resolved optics of `bend` on the camera for a `width` by `height` sensor.
fn optics(camera: &Camera, bend: OpticsBend, width: u32, height: u32) -> Optics {
    scene_params(camera, &with_optics(still(), bend), width, height)
        .optics
        .expect("a pulled filter resolves its optics")
}

/// The linear-light value of an 8-bit sRGB colour.
fn linear(rgb: [u8; 3]) -> [f32; 3] {
    let scene = Scene::from_srgb8(1, 1, &rgb);
    Channel::ALL.map(|c| scene.plane(c).get(0, 0))
}

fn uniform(width: u32, height: u32, rgb: [u8; 3]) -> Scene {
    Scene::from_srgb8(width, height, &rgb.repeat((width * height) as usize))
}

/// Foliage green, sky blue and warm red above a gradient, without black.
fn landscape(width: u32, height: u32) -> Scene {
    let rgb: Vec<u8> = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let v = ((x * 255 / width + y * 96 / height) % 256) as u8;
                match (x * 3 / width, y * 2 / height) {
                    (0, 0) => [60, 140, 40],
                    (1, 0) => [90, 150, 230],
                    (2, 0) => [220, 70, 40],
                    _ => [v, v / 2 + 60, 255 - v / 2],
                }
            })
        })
        .collect();
    Scene::from_srgb8(width, height, &rgb)
}

fn build(camera: Camera, preset: Preset, width: u32, height: u32) -> Simulator {
    Simulator::builder((camera, preset))
        .input_size(width, height)
        .build()
        .unwrap()
}

/// Renders frame 0 of `scene` from power-on.
fn render(camera: Camera, preset: Preset, scene: &Scene) -> Frame {
    build(camera, preset, scene.width(), scene.height())
        .frame(scene, 0)
        .clone()
}

/// Mean 8-bit red, green and blue of the frame's rows `rows`.
fn mean_rgb(frame: &Frame, rows: std::ops::Range<u32>) -> [f64; 3] {
    let width = frame.width() as usize;
    let mut sums = [0.0; 3];
    let count = rows.len() * width;
    for y in rows {
        let start = y as usize * width * 3;
        for pixel in frame.rgb()[start..start + width * 3].as_chunks::<3>().0 {
            for (sum, &v) in sums.iter_mut().zip(pixel) {
                *sum += f64::from(v);
            }
        }
    }
    sums.map(|sum| sum / count as f64)
}

/// Linear luminance of a mean 8-bit sRGB colour, reference white 1.
fn luminance(rgb: [f64; 3]) -> f64 {
    let decode = |v: f64| {
        let v = v / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * decode(rgb[0]) + 0.7152 * decode(rgb[1]) + 0.0722 * decode(rgb[2])
}

/// Returns the largest difference between the mean channels of a colour.
fn cast(rgb: [f64; 3]) -> f64 {
    let max = rgb.iter().copied().fold(f64::MIN, f64::max);
    let min = rgb.iter().copied().fold(f64::MAX, f64::min);
    max - min
}

/// Row `y` of a frame as RGB bytes.
fn row(frame: &Frame, y: u32) -> &[u8] {
    let stride = frame.width() as usize * 3;
    &frame.rgb()[y as usize * stride..(y as usize + 1) * stride]
}

#[test]
fn the_estimate_keeps_blue_dark_greys_at_the_weighted_sum_and_leaves_to_choice() {
    let camera = camera();
    let bend = OpticsBend {
        nir_scale: 1.3,
        ..OpticsBend::default()
    };
    let plain = optics(&camera, bend, WIDTH, HEIGHT);
    let white = plain.nir([1.0; 3]);
    let blue = plain.nir([0.0, 0.0, 1.0]);
    assert!(
        blue < 0.05 * white,
        "pure blue carries {blue}, white {white}"
    );

    let sum: f64 = bend.nir_weights.iter().sum();
    for level in [0.02, 0.18, 0.9, 2.5] {
        let expected = bend.nir_scale * sum * level;
        let grey = f64::from(plain.nir([level as f32; 3]));
        assert!(
            (grey - expected).abs() < 1e-5 * expected,
            "grey card at {level}: {grey}, expected {expected}"
        );
    }

    // The Wood effect is off by default: a leaf is any other surface.
    let leaf = linear([86, 125, 46]);
    let weighted: f64 = bend
        .nir_weights
        .iter()
        .zip(leaf)
        .map(|(w, v)| w * f64::from(v))
        .sum::<f64>()
        * bend.nir_scale;
    assert!((f64::from(plain.nir(leaf)) - weighted).abs() < 1e-6);
    assert!(
        plain.nir(leaf) < leaf[1],
        "without the Wood effect a leaf's NIR stays below its green"
    );

    // Turned on, foliage glows several times above its visible green.
    let wood = optics(
        &camera,
        OpticsBend {
            foliage_nir: 4.0,
            ..bend
        },
        WIDTH,
        HEIGHT,
    );
    for green in [[86, 125, 46], [40, 70, 30], [30, 50, 35], [0, 255, 0]] {
        let leaf = linear(green);
        assert!(
            wood.nir(leaf) > 2.0 * leaf[1],
            "leaf {green:?}: NIR {} against green {}",
            wood.nir(leaf),
            leaf[1]
        );
        let brighter = wood.nir(leaf.map(|v| 3.0 * v));
        assert!(
            (brighter - 3.0 * wood.nir(leaf)).abs() < 1e-5 * brighter,
            "leaf {green:?}: the estimate scales with the light"
        );
    }
    // Greys, blues, reds, sky, skin and cyan water (the turquoise shallows of
    // `samples/sample-01.jpg`, which absorb the NIR) are not taken for leaves.
    for colour in [
        [118, 118, 118],
        [0, 0, 255],
        [255, 0, 0],
        [90, 150, 230],
        [220, 170, 140],
        [40, 120, 120],
        [21, 112, 104],
    ] {
        let rgb = linear(colour);
        assert!(
            (wood.nir(rgb) - plain.nir(rgb)).abs() < 1e-7,
            "{colour:?} gained NIR from the foliage term"
        );
    }
}

#[test]
fn the_photosites_collect_the_nir_through_their_dyes_on_top_of_the_visible() {
    let mut camera = camera();
    camera.lens.vignette_stops = 0.0;
    let mut preset = still();
    preset.scene.headroom = 1.0;
    let input = uniform(WIDTH, HEIGHT, GREY);
    let mut clean = Scene::new(WIDTH, HEIGHT);
    scene::prepare(
        &scene_params(&camera, &preset, WIDTH, HEIGHT),
        &mut scene::State::default(),
        &input,
        &mut clean,
    );
    let params = scene_params(&camera, &with_optics(preset, pulled(1.0)), WIDTH, HEIGHT);
    let nir = params.optics.as_ref().unwrap().nir(linear(GREY));
    let mut bent = Scene::new(WIDTH, HEIGHT);
    scene::prepare(&params, &mut scene::State::default(), &input, &mut bent);
    for (channel, response) in Channel::ALL.into_iter().zip(camera.sensor.nir_response) {
        let expected = response as f32 * nir;
        for (with, without) in bent
            .plane(channel)
            .data()
            .iter()
            .zip(clean.plane(channel).data())
        {
            assert!(
                (with - without - expected).abs() < 1e-5,
                "{channel:?}: {with} - {without} is not {expected}"
            );
        }
    }
}

#[test]
fn daylight_balance_turns_the_nir_pink_and_grey_world_takes_it_back() {
    let camera = camera();
    let card = uniform(WIDTH, HEIGHT, GREY);
    let all = 0..HEIGHT;

    let plain = mean_rgb(&render(camera.clone(), still(), &card), all.clone());
    assert!(
        cast(plain) < 3.0,
        "without the bend a grey card develops neutral: {plain:?}"
    );

    let bent = mean_rgb(
        &render(camera.clone(), with_optics(still(), pulled(1.0)), &card),
        all.clone(),
    );
    assert!(
        bent[0] > bent[2] + 10.0 && bent[0] > bent[1] + 10.0,
        "with the filter removed a grey card develops pink: {bent:?}"
    );

    let auto = IspOverride {
        wb: Some(WbMode::Auto),
        ..IspOverride::default()
    };
    let balanced = mean_rgb(
        &render(
            camera,
            with_isp(with_optics(still(), pulled(1.0)), auto),
            &card,
        ),
        all,
    );
    assert!(
        cast(balanced) < 3.0,
        "grey world takes the cast back: {balanced:?}"
    );
}

#[test]
fn scene_auto_exposure_takes_back_the_extra_light() {
    let camera = camera();
    let card = uniform(WIDTH, HEIGHT, GREY);
    let metered = IspOverride {
        ae_mode: Some(AeMode::Scene),
        ..IspOverride::default()
    };
    let stops = |isp: IspOverride| {
        let mut plain = build(
            camera.clone(),
            with_isp(still(), isp.clone()),
            WIDTH,
            HEIGHT,
        );
        let before = luminance(mean_rgb(plain.frame(&card, 0), 0..HEIGHT));
        let preset = with_isp(with_optics(still(), pulled(1.0)), isp);
        let mut bent = build(camera.clone(), preset, WIDTH, HEIGHT);
        let after = luminance(mean_rgb(bent.frame(&card, 0), 0..HEIGHT));
        ((after / before).log2(), bent.exposure_scale())
    };
    let (fixed, _) = stops(IspOverride::default());
    assert!(
        fixed > 0.5,
        "at a fixed exposure the NIR brightens the card by {fixed} stops"
    );
    let (scene, scale) = stops(metered);
    assert!(
        scene.abs() < 0.25,
        "scene auto-exposure leaves {scene} stops of the extra light"
    );
    assert!(scale < 1.0, "auto-exposure stopped down to {scale}");
}

#[test]
fn a_half_pulled_filter_leaves_the_covered_rows_untouched() {
    let (width, height) = (160, 120);
    let mut camera = camera();
    camera.lens.filter_edge_blur_frac = 0.05;
    let scene = landscape(width, height);
    let plain = render(camera.clone(), still(), &scene);
    let half = render(camera.clone(), with_optics(still(), pulled(0.5)), &scene);
    let edge = 0.5 * f64::from(height);
    let shadow = camera.lens.filter_edge_blur_frac * f64::from(height);
    let below = (edge + 3.0 * shadow).ceil() as u32;
    for y in below..height {
        assert_eq!(row(&half, y), row(&plain, y), "row {y} below the edge");
    }
    let above = (edge - 3.0 * shadow).floor() as u32;
    for y in 0..above {
        assert_ne!(row(&half, y), row(&plain, y), "row {y} above the edge");
    }
}

#[test]
fn the_filter_shadow_steps_smoothly_and_closes_exactly_at_both_ends() {
    let mut camera = camera();
    camera.lens.filter_edge_blur_frac = 0.2;
    let rows = 100;
    let half = optics(&camera, pulled(0.5), WIDTH, rows);
    let fracs: Vec<f32> = (0..rows).map(|y| half.uncovered_frac(y, rows)).collect();
    assert!(fracs.windows(2).all(|pair| pair[1] <= pair[0]));
    assert!((fracs[0] - 1.0).abs() < f32::EPSILON);
    assert!(fracs[rows as usize - 1].abs() < f32::EPSILON);
    let steepest = fracs
        .windows(2)
        .map(|pair| pair[0] - pair[1])
        .fold(0.0f32, f32::max);
    assert!(
        steepest < 0.1,
        "a 20-row shadow changes by at most a tenth per row, not {steepest}"
    );
    // The edge crosses the middle at half.
    assert!((fracs[49] + fracs[50] - 1.0).abs() < 1e-6);

    for frac in [0.0, 1.0] {
        let bend = pulled(frac);
        let ends = scene_params(&camera, &with_optics(still(), bend), WIDTH, rows).optics;
        if frac > 0.0 {
            let removed = ends.expect("a removed filter has optics");
            assert!(
                (0..rows).all(|y| (removed.uncovered_frac(y, rows) - 1.0).abs() < f32::EPSILON)
            );
        } else {
            assert!(ends.is_none(), "a seated filter has no optics to compute");
        }
    }
}

#[test]
fn a_seated_filter_leaks_nothing_even_through_a_wide_shadow() {
    let mut camera = camera();
    camera.lens.filter_edge_blur_frac = 0.45;
    let scene = landscape(WIDTH, HEIGHT);
    for mode in [Mode::Still, Mode::Movie] {
        let mut preset = still();
        preset.readout.mode = mode;
        let plain = render(camera.clone(), preset.clone(), &scene);
        let seated = render(camera.clone(), with_optics(preset, pulled(0.0)), &scene);
        assert_eq!(seated, plain, "{mode}");
    }
}

#[test]
fn the_defocus_spreads_a_point_of_nir_as_a_gaussian() {
    let mut camera = camera();
    camera.lens.vignette_stops = 0.0;
    let mut preset = still();
    preset.scene.headroom = 1.0;
    let (cx, cy) = (WIDTH / 2, HEIGHT / 2);
    let mut dot = Scene::new(WIDTH, HEIGHT);
    for c in Channel::ALL {
        dot.plane_mut(c).set(cx, cy, 0.5);
    }
    let prepare = |camera: &Camera, preset: &Preset| {
        let mut out = Scene::new(WIDTH, HEIGHT);
        scene::prepare(
            &scene_params(camera, preset, WIDTH, HEIGHT),
            &mut scene::State::default(),
            &dot,
            &mut out,
        );
        out
    };
    let clean = prepare(&camera, &preset);
    let mut spread = Vec::new();
    for sigma_px in [1.5f64, 3.0] {
        camera.lens.nir_defocus_frac = sigma_px / f64::from(WIDTH);
        let bent = prepare(&camera, &with_optics(preset.clone(), pulled(1.0)));
        let nir = |x: u32, y: u32| {
            f64::from(bent.plane(Channel::Red).get(x, y) - clean.plane(Channel::Red).get(x, y))
        };
        let centre = nir(cx, cy);
        for (dx, dy) in [(1, 0), (2, 0), (0, 3), (2, 2), (4, 1)] {
            let expected = (-f64::from(dx * dx + dy * dy) / (2.0 * sigma_px * sigma_px)).exp();
            let ratio = nir(cx + dx, cy + dy) / centre;
            assert!(
                (ratio - expected).abs() < 2e-3 * expected.max(0.05),
                "sigma {sigma_px} px at ({dx}, {dy}): {ratio}, a Gaussian gives {expected}"
            );
            assert!((nir(cx - dx, cy - dy) - nir(cx + dx, cy + dy)).abs() < 1e-7);
        }
        // The blur moves the NIR without adding or losing any.
        let total: f64 = (0..HEIGHT)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .map(|(x, y)| nir(x, y))
            .sum();
        let point =
            camera.sensor.nir_response[0] * f64::from(bent_optics(&camera, &preset).nir([0.5; 3]));
        assert!(
            (total - point).abs() < 1e-4 * point,
            "sigma {sigma_px} px: {total} spread, {point} in the point"
        );
        spread.push(nir(cx + 3, cy) / centre);
    }
    assert!(
        spread[1] > 4.0 * spread[0],
        "the wider defocus reaches further"
    );
}

/// The optics of the removed filter for `preset` on `camera`.
fn bent_optics(camera: &Camera, preset: &Preset) -> Optics {
    scene_params(
        camera,
        &with_optics(preset.clone(), pulled(1.0)),
        WIDTH,
        HEIGHT,
    )
    .optics
    .unwrap()
}

#[test]
fn the_nir_image_keeps_its_size_relative_to_the_frame() {
    let mut camera = camera();
    camera.lens.vignette_stops = 0.0;
    camera.lens.nir_defocus_frac = 0.05;
    camera.lens.filter_edge_blur_frac = 0.2;
    let mut preset = still();
    preset.scene.headroom = 1.0;
    let (width, height) = (2 * WIDTH, 2 * HEIGHT);
    // A bright vertical bar on black, against the filter's edge across the middle.
    let mut input = Scene::new(width, height);
    for c in Channel::ALL {
        let plane = input.plane_mut(c);
        for y in 0..height {
            for x in width / 4..width / 2 {
                plane.set(x, y, 0.6);
            }
        }
    }
    // The red charge the NIR adds, at a sensor of `w` by `h`.
    let nir = |w: u32, h: u32| {
        let mut clean = Scene::new(w, h);
        scene::prepare(
            &scene_params(&camera, &preset, w, h),
            &mut scene::State::default(),
            &input,
            &mut clean,
        );
        let mut bent = Scene::new(w, h);
        let half = with_optics(preset.clone(), pulled(0.5));
        scene::prepare(
            &scene_params(&camera, &half, w, h),
            &mut scene::State::default(),
            &input,
            &mut bent,
        );
        let (with, without) = (bent.plane(Channel::Red), clean.plane(Channel::Red));
        let added: Vec<f64> = with
            .data()
            .iter()
            .zip(without.data())
            .map(|(a, b)| f64::from(a - b))
            .collect();
        move |x: u32, y: u32| added[(y * w + x) as usize]
    };
    let fine = nir(width, height);
    let coarse = nir(WIDTH, HEIGHT);
    let mut peak = 0.0f64;
    let mut worst = 0.0f64;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let averaged = 0.25
                * (fine(2 * x, 2 * y)
                    + fine(2 * x + 1, 2 * y)
                    + fine(2 * x, 2 * y + 1)
                    + fine(2 * x + 1, 2 * y + 1));
            peak = peak.max(averaged.abs());
            worst = worst.max((coarse(x, y) - averaged).abs());
        }
    }
    assert!(
        worst < 0.02 * peak,
        "the half-size NIR image departs by {worst} from the full one averaged (peak {peak})"
    );
}

#[test]
fn the_working_planes_carry_nothing_from_frame_to_frame() {
    let camera = camera();
    let input = landscape(WIDTH, HEIGHT);
    let removed = with_optics(still(), pulled(1.0));
    let mut defocus = camera.clone();
    defocus.lens.nir_defocus_frac *= 3.0;
    // Each frame: the camera, the preset and the sensor size.
    let frames = [
        (&camera, &removed, 2 * WIDTH, 2 * HEIGHT),
        (&camera, &removed, WIDTH, HEIGHT),
        (&camera, &still(), WIDTH, HEIGHT),
        (&defocus, &with_optics(still(), pulled(0.5)), WIDTH, HEIGHT),
        (&camera, &removed, WIDTH / 2, HEIGHT / 2),
        (&camera, &removed, WIDTH, HEIGHT),
    ];
    let mut kept = scene::State::default();
    for (i, &(camera, preset, width, height)) in frames.iter().enumerate() {
        let params = scene_params(camera, preset, width, height);
        let mut reused = Scene::new(width, height);
        scene::prepare(&params, &mut kept, &input, &mut reused);
        let mut fresh = Scene::new(width, height);
        scene::prepare(&params, &mut scene::State::default(), &input, &mut fresh);
        assert_eq!(reused, fresh, "frame {i} at {width} x {height}");
    }
}

#[test]
fn the_bend_replays_from_reset_and_follows_the_filter_while_running() {
    let camera = camera();
    let scene = landscape(WIDTH, HEIGHT);
    for mode in [Mode::Still, Mode::Movie] {
        let mut preset = with_optics(still(), pulled(0.7));
        preset.readout.mode = mode;
        let mut sim = build(camera.clone(), preset, WIDTH, HEIGHT);
        let first: Vec<Frame> = (0..3).map(|i| sim.frame(&scene, i).clone()).collect();
        sim.reset();
        let again: Vec<Frame> = (0..3).map(|i| sim.frame(&scene, i).clone()).collect();
        assert_eq!(first, again, "{mode}: replay from reset");
    }

    let card = uniform(WIDTH, HEIGHT, GREY);
    let mut sim = build(camera, with_optics(still(), pulled(1.0)), WIDTH, HEIGHT);
    let removed = sim.frame(&card, 0).clone();
    let (top, bottom) = (0..HEIGHT / 4, 3 * HEIGHT / 4..HEIGHT);
    assert!(cast(mean_rgb(&removed, bottom.clone())) > 10.0);
    assert_eq!(
        sim.set_preset(with_optics(still(), pulled(0.5))).unwrap(),
        KEPT,
        "sliding the filter half back is a knob on the running camera"
    );
    let half = sim.frame(&card, 1).clone();
    assert!(
        cast(mean_rgb(&half, top.clone())) > 10.0,
        "the uncovered top stays pink"
    );
    assert!(
        cast(mean_rgb(&half, bottom.clone())) < 3.0,
        "the covered bottom is grey again: {:?}",
        mean_rgb(&half, bottom)
    );
    assert_eq!(sim.set_preset(still()).unwrap(), KEPT, "removing the bend");
    let seated = sim.frame(&card, 2).clone();
    assert!(cast(mean_rgb(&seated, top)) < 3.0, "the top is grey again");
}
