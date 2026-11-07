//! The light leak, `[bend.leak]`: light through the body glows onto the sensor from one
//! edge, falls by `1/e` over its spread, takes the scene's colour path and the near
//! infrared the IR-cut filter would have stopped, smears in stills as in movies, is
//! taken back by scene auto-exposure, and without light changes no byte.

#![cfg(feature = "serde")]

use std::f64::consts::E;
use std::ops::Range;

use vcb_core::isp::AeMode;
use vcb_core::preset::{Edge, IspOverride, LeakBend, Mode, Resolution, SensorOverride};
use vcb_core::sensor::{self, Ae, Charge};
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing};

/// Size of the scenes rendered through the whole camera.
const WIDTH: u32 = 96;
const HEIGHT: u32 = 72;

/// Frames rendered from reset in each comparison.
const FRAMES: u64 = 3;

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

/// The shipped clean preset in `mode`, with the sensor set to the scene's size.
fn clean(mode: Mode) -> Preset {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../presets/a520-clean.toml");
    let mut preset: Preset = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    preset.readout.mode = mode;
    preset.readout.resolution = Resolution::MatchInput;
    preset
}

/// `preset` with `[bend.leak]` set to `leak`.
fn leaky(mut preset: Preset, leak: LeakBend) -> Preset {
    preset.bend.leak = Some(leak);
    preset
}

/// A leak of white light without near infrared from `edge`.
fn white(edge: Edge, radiance: f64, spread_frac: f64) -> LeakBend {
    LeakBend {
        edge,
        radiance,
        spread_frac,
        colour: [1.0; 3],
        nir: 0.0,
    }
}

/// `preset` with auto-exposure in `mode`.
fn metered(mut preset: Preset, mode: AeMode) -> Preset {
    preset.isp = Some(IspOverride {
        ae_mode: Some(mode),
        ..IspOverride::default()
    });
    preset
}

fn build(preset: Preset, width: u32, height: u32) -> Simulator {
    Simulator::builder((camera(), preset))
        .input_size(width, height)
        .build()
        .unwrap()
}

/// Frames `0..FRAMES` in order.
fn render(sim: &mut Simulator, scene: &Scene) -> Vec<Frame> {
    (0..FRAMES)
        .map(|index| sim.frame(scene, index).clone())
        .collect()
}

/// A gradient with a clipped white patch, saturated colours and a black corner.
fn scene() -> Scene {
    let rgb: Vec<u8> = (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).flat_map(move |x| {
                let v = ((x * 255 / WIDTH + y * 96 / HEIGHT) % 256) as u8;
                match (x * 4 / WIDTH, y * 2 / HEIGHT) {
                    (0, 0) => [0, 0, 0],
                    (1, 0) => [255, 255, 255],
                    (2, 0) => [220, 70, 40],
                    _ => [v, v / 2 + 60, 255 - v],
                }
            })
        })
        .collect();
    Scene::from_srgb8(WIDTH, HEIGHT, &rgb)
}

fn uniform(width: u32, height: u32, rgb: [u8; 3]) -> Scene {
    Scene::from_srgb8(width, height, &rgb.repeat((width * height) as usize))
}

/// Resolves the sensor of a match-input `preset` for a `width` by `height` sensor.
fn sensor_params(camera: &Camera, preset: &Preset, width: u32, height: u32) -> sensor::Params {
    let timing = Timing::derive(camera, preset, width, height).unwrap();
    sensor::Params::resolve(camera, preset, &timing).unwrap()
}

/// Exposes `scene` as frame 0 from a fresh state.
fn expose(params: &sensor::Params, scene: &Scene) -> Charge {
    let mut charge = Charge::new(scene.width(), scene.height());
    sensor::expose(params, &mut sensor::State::default(), scene, 0, &mut charge);
    charge
}

/// Mean charge of the photosites in columns `xs` and rows `ys`, in electrons.
fn mean(charge: &Charge, xs: Range<u32>, ys: Range<u32>) -> f64 {
    let count = f64::from(xs.len() as u32 * ys.len() as u32);
    let plane = charge.plane();
    ys.flat_map(|y| xs.clone().map(move |x| f64::from(plane.get(x, y))))
        .sum::<f64>()
        / count
}

/// Mean charge of each pair of lines across the leak's axis, from the first line on.
///
/// Columns for a left or right leak, rows for a top or bottom one; a pair spans one
/// period of the colour filter array, so every pair holds the same mix of filters.
fn pair_profile(charge: &Charge, edge: Edge) -> Vec<f64> {
    let (width, height) = (charge.plane().width(), charge.plane().height());
    match edge {
        Edge::Left | Edge::Right => (0..width / 2)
            .map(|i| mean(charge, 2 * i..2 * i + 2, 0..height))
            .collect(),
        Edge::Top | Edge::Bottom => (0..height / 2)
            .map(|i| mean(charge, 0..width, 2 * i..2 * i + 2))
            .collect(),
    }
}

/// Mean linear red, green and blue of the frame's columns `xs` and rows `ys`.
fn mean_rgb(frame: &Frame, xs: Range<u32>, ys: Range<u32>) -> [f64; 3] {
    let width = frame.width() as usize;
    let mut sums = [0.0; 3];
    for y in ys.clone() {
        for x in xs.clone() {
            let start = (y as usize * width + x as usize) * 3;
            for (sum, &v) in sums.iter_mut().zip(&frame.rgb()[start..start + 3]) {
                *sum += decode(f64::from(v));
            }
        }
    }
    let count = xs.len() as f64 * ys.len() as f64;
    sums.map(|sum| sum / count)
}

/// Linear value of an 8-bit sRGB code, reference white 1.
fn decode(code: f64) -> f64 {
    let v = code / 255.0;
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Mean linear luminance of a frame, decoded pixel by pixel, reference white 1.
fn luminance(frame: &Frame) -> f64 {
    let pixels = frame.rgb().as_chunks::<3>().0;
    pixels
        .iter()
        .map(|p| {
            0.2126 * decode(f64::from(p[0]))
                + 0.7152 * decode(f64::from(p[1]))
                + 0.0722 * decode(f64::from(p[2]))
        })
        .sum::<f64>()
        / pixels.len() as f64
}

#[test]
fn a_leak_from_the_left_is_brightest_in_column_0_and_falls_by_1_over_e_at_its_spread() {
    let camera = camera();
    let (width, height) = (320, 240);
    let (radiance, spread_frac) = (0.5, 0.3);
    let preset = leaky(clean(Mode::Still), white(Edge::Left, radiance, spread_frac));
    let params = sensor_params(&camera, &preset, width, height);
    let charge = expose(&params, &Scene::new(width, height));
    let spread_px = spread_frac * f64::from(width);
    assert!(
        (spread_px - spread_px.round()).abs() < 1e-9
            && (spread_px.round() as u32).is_multiple_of(2),
        "the spread must be an even number of columns, {spread_px}"
    );
    let spread_col = spread_px.round() as u32;

    // White light at the opening fills green as reference white would, the colour
    // matrix mapping white to white and the daylight balance leaving green alone.
    let odd_rows: Vec<u32> = (1..height).step_by(2).collect();
    let column_0_green = odd_rows
        .iter()
        .map(|&y| f64::from(charge.plane().get(0, y)))
        .sum::<f64>()
        / odd_rows.len() as f64;
    let expected = radiance * f64::from(params.white_e()) * (-0.5 / spread_px).exp();
    assert!(
        (column_0_green / expected - 1.0).abs() < 0.01,
        "column 0's green holds {column_0_green:.0} e, reference white's share {expected:.0} e"
    );

    // The pair of columns at the edge is the brightest, and the glow falls all the way.
    let pairs = pair_profile(&charge, Edge::Left);
    let brightest = (0..pairs.len())
        .max_by(|&a, &b| pairs[a].total_cmp(&pairs[b]))
        .unwrap();
    assert_eq!(brightest, 0, "the brightest columns are pair {brightest}");
    for (i, step) in pairs
        .iter()
        .step_by(4)
        .collect::<Vec<_>>()
        .windows(2)
        .enumerate()
    {
        assert!(
            step[1] < step[0],
            "the glow rose from {:.1} e to {:.1} e at pair {}",
            step[0],
            step[1],
            4 * (i + 1)
        );
    }
    // Same filter phase, `spread_px` apart: one `1/e`.
    let at_spread = mean(&charge, spread_col..spread_col + 1, 0..height);
    let ratio = at_spread / mean(&charge, 0..1, 0..height);
    assert!(
        (ratio - 1.0 / E).abs() < 0.005,
        "the glow fell to {ratio:.4} of column 0's at spread_frac of the width, not 1/e"
    );
}

#[test]
fn each_edge_puts_the_maximum_on_its_edge() {
    let camera = camera();
    let (width, height) = (320, 240);
    let spread_frac = 0.3;
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        // The default colour and daylight's near infrared, below the well at the edge.
        let leak = LeakBend {
            edge,
            radiance: 0.5,
            spread_frac,
            ..LeakBend::default()
        };
        let mut params = sensor_params(&camera, &leaky(clean(Mode::Still), leak), width, height);
        // Without the smear, which lifts every row of a column by the same charge and
        // so flattens a top or bottom leak's far rows; it has tests of its own.
        params.smear_db = f32::NEG_INFINITY;
        let charge = expose(&params, &Scene::new(width, height));
        let mut pairs = pair_profile(&charge, edge);
        if matches!(edge, Edge::Right | Edge::Bottom) {
            pairs.reverse();
        }
        let brightest = (0..pairs.len())
            .max_by(|&a, &b| pairs[a].total_cmp(&pairs[b]))
            .unwrap();
        assert_eq!(
            brightest, 0,
            "{edge}: the brightest pair is {brightest} from the edge"
        );
        let extent = match edge {
            Edge::Left | Edge::Right => width,
            Edge::Top | Edge::Bottom => height,
        };
        let far = pairs[pairs.len() - 1] / pairs[0];
        let expected = (-f64::from(extent - 2) / (spread_frac * f64::from(extent))).exp();
        assert!(
            (far / expected - 1.0).abs() < 0.05,
            "{edge}: the far edge holds {far:.4} of the near one's charge, expected {expected:.4}"
        );
    }
}

#[test]
fn a_leak_without_light_renders_the_camera_s_bytes() {
    let scene = scene();
    let dark = [
        LeakBend {
            radiance: 0.0,
            ..LeakBend::default()
        },
        LeakBend {
            colour: [0.0; 3],
            nir: 0.0,
            ..LeakBend::default()
        },
    ];
    for mode in [Mode::Still, Mode::Movie] {
        for ae in [AeMode::Fixed, AeMode::Scene] {
            let plain = metered(clean(mode), ae);
            let expected = render(&mut build(plain.clone(), WIDTH, HEIGHT), &scene);
            for leak in dark {
                let mut sim = build(leaky(plain.clone(), leak), WIDTH, HEIGHT);
                assert_eq!(
                    render(&mut sim, &scene),
                    expected,
                    "{mode}, {ae:?}: {leak:?} changed the frames"
                );
            }
        }
    }
}

#[test]
fn a_white_leak_develops_neutral_and_its_near_infrared_pink() {
    let black = Scene::new(WIDTH, HEIGHT);
    // The rows nearest the opening, bright but below the clip. From the top, where the
    // camera develops a scene's white light as neutral; it leaves a few per cent of red
    // and blue in the bright last rows and first columns of a frame, from a scene as
    // from a leak.
    let near = 0..HEIGHT / 8;
    let colour = |nir: f64| {
        let leak = LeakBend {
            nir,
            ..white(Edge::Top, 0.3, 0.3)
        };
        let mut sim = build(leaky(clean(Mode::Still), leak), WIDTH, HEIGHT);
        mean_rgb(sim.frame(&black, 0), 0..WIDTH, near.clone())
    };
    let [r, g, b] = colour(0.0);
    assert!(g > 0.1, "the leak is too dark to judge: {g:.3}");
    for (name, channel) in [("red", r), ("blue", b)] {
        assert!(
            (channel / g - 1.0).abs() < 0.03,
            "a white leak developed {name} at {:.3} of green: {:?}",
            channel / g,
            [r, g, b]
        );
    }
    let [r, g, b] = colour(1.0);
    assert!(
        r > 1.2 * g && r > 1.2 * b,
        "daylight's near infrared did not turn the leak pink: {:?}",
        [r, g, b]
    );
}

/// A bright leak from the top falling within a few rows, and the rows it does not reach.
fn top_leak() -> (LeakBend, Range<u32>) {
    (white(Edge::Top, 50.0, 0.03), 64..96)
}

/// A bar twice reference white across the columns `lit` of the middle rows.
fn bar(width: u32, height: u32, lit: Range<u32>) -> Scene {
    let mut bar = Scene::new(width, height);
    for plane in bar.planes_mut() {
        for y in 40..56 {
            for x in lit.clone() {
                plane.set(x, y, 2.0);
            }
        }
    }
    bar
}

/// The leak's mean charge down column `x`, from its resolved edge charge and spread.
fn leak_column_mean(params: &sensor::Params, x: u32, height: u32) -> f64 {
    let leak = params.leak.expect("the leak lets light in");
    (0..height)
        .map(|y| {
            let channel = params.cfa.channel(x, y).index();
            let d = f64::from(y) + 0.5;
            f64::from(leak.edge_e[channel]) * (-d / f64::from(leak.spread_px)).exp()
        })
        .sum::<f64>()
        / f64::from(height)
}

#[test]
fn a_leak_smears_a_still_where_the_scene_does_not() {
    let camera = camera();
    let (width, height) = (64, 96);
    let (leak, far) = top_leak();
    let black = Scene::new(width, height);
    let plain = clean(Mode::Still);
    let params = sensor_params(&camera, &plain, width, height);
    let far_rows = |charge: &Charge| charge.plane().data()[(far.start * width) as usize..].to_vec();
    // Behind the mechanical shutter the scene does not smear.
    let dark = expose(&params, &black);
    assert!(
        params.smear_per_electron() == 0.0,
        "a still smears the scene"
    );
    assert_eq!(
        far_rows(&expose(&params, &bar(width, height, 16..48))),
        far_rows(&dark),
        "a still's bright bar reached the rows it does not light"
    );
    // The leak shines on through the transfer and streaks every column.
    let bent = sensor_params(&camera, &leaky(plain, leak), width, height);
    let leaked = expose(&bent, &black);
    let per_electron = f64::from(bent.leak_smear_per_electron());
    assert!(per_electron > 0.0, "the leak does not smear a still");
    for x in [0, 1, width / 2, width - 1] {
        let streak = mean(&leaked, x..x + 1, far.clone()) - mean(&dark, x..x + 1, far.clone());
        let expected = per_electron * leak_column_mean(&bent, x, height);
        assert!(
            expected > 10.0,
            "column {x}: the smear is too weak to measure, {expected:.1} e"
        );
        assert!(
            (streak / expected - 1.0).abs() < 0.05,
            "column {x}: the leak streaked the dark rows by {streak:.1} e, expected {expected:.1} e"
        );
    }
}

#[test]
fn in_movie_mode_the_leak_s_smear_adds_to_the_scene_s() {
    let camera = camera();
    let (width, height) = (64, 96);
    let (leak, far) = top_leak();
    let lit = 16..48;
    let plain = clean(Mode::Movie);
    let params = sensor_params(&camera, &plain, width, height);
    let bent = sensor_params(&camera, &leaky(plain, leak), width, height);
    assert!(
        (bent.leak_smear_per_electron() - bent.smear_per_electron()).abs()
            <= f32::EPSILON * bent.smear_per_electron(),
        "the electronic shutter smears the leak as the scene"
    );
    let (black, lit_bar) = (Scene::new(width, height), bar(width, height, lit.clone()));
    let streak = |params: &sensor::Params, scene: &Scene| {
        mean(&expose(params, scene), lit.clone(), far.clone())
    };
    let none = streak(&params, &black);
    let scene_only = streak(&params, &lit_bar) - none;
    let leak_only = streak(&bent, &black) - none;
    let both = streak(&bent, &lit_bar) - none;
    assert!(
        scene_only > 10.0 && leak_only > 10.0,
        "the bar smeared {scene_only:.1} e and the leak {leak_only:.1} e"
    );
    assert!(
        (both / (scene_only + leak_only) - 1.0).abs() < 0.03,
        "together they smeared {both:.1} e, apart {scene_only:.1} e and {leak_only:.1} e"
    );
}

#[test]
fn the_aperture_does_not_dim_the_leak_and_the_exposure_time_does_scale_it() {
    let camera = camera();
    let (width, height) = (128, 96);
    let black = Scene::new(width, height);
    let preset = leaky(clean(Mode::Still), white(Edge::Left, 0.5, 0.3));
    let mut params = sensor_params(&camera, &preset, width, height);
    params.exposure_scale = Some(0.25);
    let stopped_down = expose(&params, &black);
    params.exposure_scale = Some(4.0);
    assert_eq!(
        expose(&params, &black),
        stopped_down,
        "the exposure scale moved the leak's charge"
    );
    // Twice the time collects twice the leak.
    let mut longer = preset;
    longer.sensor = Some(SensorOverride {
        exposure_s: Some(2.0 * camera.still.exposure_s),
    });
    let doubled = expose(&sensor_params(&camera, &longer, width, height), &black);
    let ratio = mean(&doubled, 0..8, 0..height) / mean(&stopped_down, 0..8, 0..height);
    assert!(
        (ratio - 2.0).abs() < 0.02,
        "twice the exposure collected {ratio:.3} times the leak"
    );
}

/// The leak's metered luminance in the scene meter's units, from its resolved charge.
fn leak_luminance(params: &sensor::Params, width: u32) -> f64 {
    let leak = params.leak.expect("the leak lets light in");
    let Ae::Scene { weights, .. } = params.ae else {
        panic!("the preset does not meter the scene")
    };
    let falloff = (0..width)
        .map(|x| (-(f64::from(x) + 0.5) / f64::from(leak.spread_px)).exp())
        .sum::<f64>()
        / f64::from(width);
    let luminance: f64 = (0..3)
        .map(|c| f64::from(weights[c]) * f64::from(leak.edge_e[c]))
        .sum();
    luminance * falloff / f64::from(params.white_fraction * params.full_well_e)
}

#[test]
fn scene_auto_exposure_takes_a_leak_back() {
    let camera = camera();
    let card = uniform(WIDTH, HEIGHT, GREY);
    let target = camera.isp.ae_target;
    let scene_ae = metered(clean(Mode::Still), AeMode::Scene);
    let unit = sensor_params(
        &camera,
        &leaky(scene_ae.clone(), white(Edge::Left, 1.0, 0.3)),
        WIDTH,
        HEIGHT,
    );
    // The metered luminance is proportional to the radiance.
    let per_radiance = leak_luminance(&unit, WIDTH);
    let leak_at = |share: f64| white(Edge::Left, share * target / per_radiance, 0.3);
    let shot = |preset: Preset, scene: &Scene| {
        let mut sim = build(preset, WIDTH, HEIGHT);
        let frame = sim.frame(scene, 0).clone();
        (luminance(&frame), sim.exposure_scale())
    };

    let (clean_mean, clean_scale) = shot(scene_ae.clone(), &card);
    assert!(
        clean_scale > 0.6 && clean_scale < 2.0,
        "the card needs {clean_scale} of the set exposure"
    );
    let (half_mean, half_scale) = shot(leaky(scene_ae.clone(), leak_at(0.5)), &card);
    assert!(
        (half_scale / clean_scale - 0.5).abs() < 0.01,
        "a leak at half the target scaled the scene by {half_scale}, the clean card by {clean_scale}"
    );
    // Within a stop, and in fact within a quarter: the gradient of the leak develops
    // through the tone curve a little apart from the even card it replaces.
    let stops = (half_mean / clean_mean).log2();
    assert!(
        stops.abs() < 0.25,
        "with the leak the frame is {stops:.2} stops off the clean one"
    );
    // At a fixed exposure the same leak brightens the frame further.
    let fixed = metered(clean(Mode::Still), AeMode::Fixed);
    let (fixed_clean, _) = shot(fixed.clone(), &card);
    let (fixed_half, _) = shot(leaky(fixed, leak_at(0.5)), &card);
    let fixed_stops = (fixed_half / fixed_clean).log2();
    assert!(
        fixed_stops > 0.5,
        "at a fixed exposure the leak brightened the card by only {fixed_stops:.2} stops"
    );

    // A leak brighter than the target stops down as far as auto-exposure goes, as far
    // as for a scene far too bright.
    let (_, floor) = shot(scene_ae.clone(), &uniform(WIDTH, HEIGHT, [255; 3]));
    let (_, over_scale) = shot(leaky(scene_ae, leak_at(1.5)), &card);
    assert!(
        floor < 0.5 * clean_scale,
        "the bright scene metered {floor}"
    );
    assert!(
        (over_scale - floor).abs() < f32::EPSILON,
        "a leak beyond the target scaled the scene by {over_scale}, the lower clamp is {floor}"
    );
}

/// A knob of the leak and a way to turn it.
type Turn = (&'static str, fn(&mut LeakBend));

#[test]
fn turning_the_leak_keeps_the_running_camera() {
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        let mut preset = leaky(clean(mode), LeakBend::default());
        let mut sim = build(preset.clone(), WIDTH, HEIGHT);
        let before = render(&mut sim, &scene);
        let turns: [Turn; 4] = [
            ("the edge", |leak| leak.edge = Edge::Right),
            ("the radiance", |leak| leak.radiance = 3.0),
            ("the spread and colour", |leak| {
                leak.spread_frac = 1.0;
                leak.colour = [1.0, 0.4, 0.25];
            }),
            ("the near infrared", |leak| leak.nir = 1.5),
        ];
        let mut frames = vec![before.last().unwrap().clone()];
        for (index, (knob, turn)) in (FRAMES..).zip(turns) {
            turn(preset.bend.leak.as_mut().unwrap());
            assert_eq!(
                sim.set_preset(preset.clone()).unwrap(),
                KEPT,
                "{mode}: turning {knob}"
            );
            let frame = sim.frame(&scene, index).clone();
            assert_ne!(
                &frame,
                frames.last().unwrap(),
                "{mode}: turning {knob} changed nothing"
            );
            frames.push(frame);
        }
        // Moved to the right edge, the leak left the frame's left and lit its right.
        let (from_left, from_right) = (&frames[0], &frames[1]);
        let width = from_left.width();
        let (left, right) = (0..width / 8, width - width / 8..width);
        let green = |frame: &Frame, xs: Range<u32>| mean_rgb(frame, xs, 0..frame.height())[1];
        assert!(
            green(from_right, right.clone()) > 1.2 * green(from_left, right.clone())
                && green(from_right, left.clone()) < 0.8 * green(from_left, left.clone()),
            "{mode}: moving the leak right took the left from {:.3} to {:.3} and the right \
             from {:.3} to {:.3}",
            green(from_left, left.clone()),
            green(from_right, left),
            green(from_left, right.clone()),
            green(from_right, right)
        );
    }
}

#[test]
fn a_leak_replays_from_reset() {
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        for ae in [AeMode::Fixed, AeMode::Scene, AeMode::Servo] {
            let leak = LeakBend {
                edge: Edge::Bottom,
                radiance: 2.0,
                ..LeakBend::default()
            };
            let preset = leaky(metered(clean(mode), ae), leak);
            let mut sim = build(preset.clone(), WIDTH, HEIGHT);
            let first = render(&mut sim, &scene);
            sim.reset();
            assert_eq!(
                render(&mut sim, &scene),
                first,
                "{mode}, {ae:?}: from reset"
            );
            assert_eq!(
                render(&mut build(preset, WIDTH, HEIGHT), &scene),
                first,
                "{mode}, {ae:?}: from a new simulator"
            );
        }
    }
}
