//! The sensor bend, `[bend.sensor]`: heat doubles the dark current, its hot photosites
//! and the amplifier's glow with it, the substrate bias moves the well, the drain and
//! the movie shutter, and the camera's own temperature and bias change no byte.

#![cfg(feature = "serde")]

use std::ops::Range;

use vcb_core::preset::{Mode, Resolution, SensorBend, SensorOverride};
use vcb_core::readout::{self, Direction};
use vcb_core::sensor::{self, Charge};
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

/// Frames rendered from reset in each comparison.
const FRAMES: u64 = 3;

/// A camera key and a way to set it out of range.
type Break = (&'static str, fn(&mut Camera));

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

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

/// An unmodified preset in `mode` whose sensor takes the scene's size.
fn bare(mode: Mode) -> Preset {
    let mut preset = Preset::new("a520");
    preset.readout.mode = mode;
    preset.readout.resolution = Resolution::MatchInput;
    preset
}

/// `preset` with the sensor at `temperature_c` and `substrate_v`, `None` keeping the camera's.
fn bent(mut preset: Preset, temperature_c: Option<f64>, substrate_v: Option<f64>) -> Preset {
    preset.bend.sensor = Some(SensorBend {
        temperature_c,
        substrate_v,
    });
    preset
}

/// `preset` with the exposure time set to `exposure_s`.
fn exposed(mut preset: Preset, exposure_s: f64) -> Preset {
    preset.sensor = Some(SensorOverride {
        exposure_s: Some(exposure_s),
    });
    preset
}

fn build(camera: Camera, preset: Preset) -> Simulator {
    Simulator::builder((camera, preset))
        .input_size(WIDTH, HEIGHT)
        .build()
        .unwrap()
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

/// Frames `0..FRAMES` in order.
fn render(sim: &mut Simulator, scene: &Scene) -> Vec<Frame> {
    (0..FRAMES)
        .map(|index| sim.frame(scene, index).clone())
        .collect()
}

/// Resolves the sensor of `camera` and a match-input `preset` for a `width` by `height` sensor.
fn sensor_params(camera: &Camera, preset: &Preset, width: u32, height: u32) -> sensor::Params {
    let timing = Timing::derive(camera, preset, width, height).unwrap();
    sensor::Params::resolve(camera, preset, &timing).unwrap()
}

/// Exposes `scene` as frame `index`.
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

/// Mean charge of the photosites in columns `xs` and rows `ys`, in electrons.
fn mean(charge: &Charge, xs: Range<u32>, ys: Range<u32>) -> f64 {
    let count = f64::from(xs.len() as u32 * ys.len() as u32);
    let plane = charge.plane();
    ys.flat_map(|y| xs.clone().map(move |x| f64::from(plane.get(x, y))))
        .sum::<f64>()
        / count
}

/// Arrhenius factor of the camera's dark current at `temperature_c`, from its doubling rule.
fn arrhenius(camera: &Camera, temperature_c: f64) -> f64 {
    ((temperature_c - camera.sensor.reference_temp_c) / camera.sensor.dark_doubling_c).exp2()
}

#[test]
fn the_camera_s_own_temperature_and_bias_render_byte_identically() {
    let camera = camera();
    let scene = scene();
    let own = SensorBend {
        temperature_c: Some(camera.sensor.reference_temp_c),
        substrate_v: Some(camera.sensor.substrate_v),
    };
    for mode in [Mode::Still, Mode::Movie] {
        // A long exposure too, where dark charge, hot photosites and clipping all count.
        for plain in [clean(mode), exposed(clean(mode), 0.25)] {
            let expected = render(&mut build(camera.clone(), plain.clone()), &scene);
            for bend in [SensorBend::default(), own] {
                let mut with = plain.clone();
                with.bend.sensor = Some(bend);
                let mut sim = build(camera.clone(), with);
                assert_eq!(
                    render(&mut sim, &scene),
                    expected,
                    "{mode}: {bend:?} changed the frames"
                );
                sim.reset();
                assert_eq!(
                    render(&mut sim, &scene),
                    expected,
                    "{mode}: {bend:?} changed the frames after a reset"
                );
            }
        }
    }
}

#[test]
fn the_dark_current_doubles_every_dark_doubling_c() {
    let mut camera = camera();
    // The glow and the hot photosites are tested on their own; here the bulk dark counts.
    camera.sensor.amp_glow_e_per_s = 0.0;
    camera.sensor.hot_pixels_per_mpix = 0.0;
    let (width, height) = (256, 192);
    let reference_c = camera.sensor.reference_temp_c;
    let doubling_c = camera.sensor.dark_doubling_c;
    // Forty electrons at the reference: well above the fixed offsets and the clip at zero.
    let exposure_s = 40.0 / camera.sensor.dark_e_per_s;
    let black = Scene::new(width, height);
    let at = |temperature_c: f64| {
        let preset = bent(
            exposed(bare(Mode::Still), exposure_s),
            Some(temperature_c),
            None,
        );
        let params = sensor_params(&camera, &preset, width, height);
        let timing = Timing::derive(&camera, &preset, width, height).unwrap();
        let optical_black = readout::Params::resolve(&camera, &preset, &timing)
            .unwrap()
            .dark_e;
        let charge = expose(&params, &black, 0);
        (mean(&charge, 0..width, 0..height), f64::from(optical_black))
    };
    let (cold, cold_ob) = at(reference_c - doubling_c);
    let (room, room_ob) = at(reference_c);
    let (warm, warm_ob) = at(reference_c + doubling_c);
    for (low, high, what) in [(cold, room, "cooled"), (room, warm, "heated")] {
        assert!(
            (high / low - 2.0).abs() < 0.04,
            "one doubling {what}: {low:.2} e to {high:.2} e"
        );
    }
    assert!(
        (room / (40.0) - 1.0).abs() < 0.02,
        "{room:.2} e at the reference temperature"
    );
    // The masked photosites share the temperature, so the black clamp follows it.
    for (low, high) in [(cold_ob, room_ob), (room_ob, warm_ob)] {
        assert!(
            (high / low - 2.0).abs() < 1e-5,
            "optical black {low} e to {high} e"
        );
    }
    assert!(
        (room_ob / exposure_s / camera.sensor.dark_e_per_s - 1.0).abs() < 1e-5,
        "the optical black integrates the set exposure: {room_ob} e"
    );
}

#[test]
fn hot_photosites_keep_their_multiples_and_their_count() {
    let mut camera = camera();
    camera.sensor.amp_glow_e_per_s = 0.0;
    // Enough hot photosites on a small sensor to count them.
    camera.sensor.hot_pixels_per_mpix = 2000.0;
    let (width, height) = (256, 192);
    let reference_c = camera.sensor.reference_temp_c;
    // Fifteen electrons of mean dark at the reference: the hottest photosites stay
    // below the well one doubling up, the coolest stand far above the rest.
    let exposure_s = 15.0 / camera.sensor.dark_e_per_s;
    let black = Scene::new(width, height);
    let hot_sites = |temperature_c: f64| {
        let preset = bent(
            exposed(bare(Mode::Still), exposure_s),
            Some(temperature_c),
            None,
        );
        let params = sensor_params(&camera, &preset, width, height);
        let threshold = 20.0 * params.dark_e_per_s * params.exposure_s;
        let charge = expose(&params, &black, 0);
        let sites: Vec<(u32, u32, f32)> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .filter_map(|(x, y)| {
                let e = charge.plane().get(x, y);
                (e > threshold).then_some((x, y, e))
            })
            .collect();
        (sites, params.well_e())
    };
    let (room, well) = hot_sites(reference_c);
    let (warm, _) = hot_sites(reference_c + camera.sensor.dark_doubling_c);
    assert!(room.len() > 20, "only {} hot photosites", room.len());
    assert_eq!(
        room.iter().map(|&(x, y, _)| (x, y)).collect::<Vec<_>>(),
        warm.iter().map(|&(x, y, _)| (x, y)).collect::<Vec<_>>(),
        "heat changed which photosites are hot"
    );
    let (mut sum_room, mut sum_warm) = (0.0f64, 0.0f64);
    for (&(x, y, cool_e), &(_, _, warm_e)) in room.iter().zip(&warm) {
        assert!(warm_e < well, "hot photosite ({x}, {y}) filled its well");
        assert!(
            (warm_e / cool_e - 2.0).abs() < 0.1,
            "hot photosite ({x}, {y}): {cool_e:.0} e to {warm_e:.0} e"
        );
        sum_room += f64::from(cool_e);
        sum_warm += f64::from(warm_e);
    }
    assert!(
        (sum_warm / sum_room - 2.0).abs() < 0.02,
        "the hot photosites together went from {sum_room:.0} e to {sum_warm:.0} e"
    );
}

#[test]
fn the_amplifier_glows_from_the_readout_s_first_corner() {
    let mut camera = camera();
    // No hot photosite in the corners measured, and no spread of the dark current,
    // which at this heat would vary more between two small patches than the glow.
    camera.sensor.hot_pixels_per_mpix = 0.0;
    camera.sensor.dark_nonuniformity = 0.0;
    let glow_e_per_s = camera.sensor.amp_glow_e_per_s;
    assert!(glow_e_per_s > 0.0, "the camera's amplifier does not glow");
    let (width, height) = (1024, 768);
    let black = Scene::new(width, height);
    // Hot enough for about 2000 e of glow at the corner in 2 s.
    let temperature_c = camera.sensor.reference_temp_c
        + camera.sensor.dark_doubling_c * (1.0 + 1000.0 / glow_e_per_s).log2();
    let k = arrhenius(&camera, temperature_c);
    let patch = 4;
    let (left, right) = (0..patch, width - patch..width);
    let (top, bottom) = (0..patch, height - patch..height);
    for direction in [Direction::TopDown, Direction::BottomUp] {
        camera.still.direction = direction;
        let (first, last) = match direction {
            Direction::TopDown => (top.clone(), bottom.clone()),
            Direction::BottomUp => (bottom.clone(), top.clone()),
        };
        // The corner's, the first row's far end's and the last row's far end's charge.
        let corners = |exposure_s: f64| {
            let preset = bent(
                exposed(bare(Mode::Still), exposure_s),
                Some(temperature_c),
                None,
            );
            let params = sensor_params(&camera, &preset, width, height);
            let charge = expose(&params, &black, 0);
            let corner = mean(&charge, left.clone(), first.clone());
            assert!(corner < f64::from(params.well_e()), "the corner clipped");
            (
                corner,
                mean(&charge, right.clone(), first.clone()),
                mean(&charge, right.clone(), last.clone()),
            )
        };
        let (corner, row_end, opposite) = corners(2.0);
        let glow = corner - opposite;
        let expected = glow_e_per_s * (k - 1.0) * 2.0;
        assert!(
            glow > 0.9 * expected,
            "{direction:?}: the corner glows {glow:.0} e over the opposite one, \
             expected about {expected:.0} e"
        );
        assert!(
            row_end - opposite < 0.5 * glow,
            "{direction:?}: the far end of the first row glows {:.0} e of the corner's {glow:.0} e",
            row_end - opposite
        );
        let (corner, _, opposite) = corners(4.0);
        assert!(
            ((corner - opposite) / glow - 2.0).abs() < 0.1,
            "{direction:?}: twice the exposure glowed {:.3} times as much",
            (corner - opposite) / glow
        );
    }
    // At the reference temperature the camera is as characterised, without a glow.
    let preset = exposed(bare(Mode::Still), 2.0);
    let params = sensor_params(&camera, &preset, width, height);
    assert!(params.amp_glow_e_per_s.abs() < f32::EPSILON);
}

#[test]
fn a_raised_substrate_bias_clips_a_grey_card_the_nominal_bias_holds() {
    let camera = camera();
    let (width, height) = (128, 96);
    let mut card = Scene::new(width, height);
    for plane in card.planes_mut() {
        plane.fill(0.9);
    }
    let nominal = sensor_params(&camera, &bare(Mode::Still), width, height);
    let raised_v = camera.sensor.substrate_v + 1.5;
    let raised = sensor_params(
        &camera,
        &bent(bare(Mode::Still), None, Some(raised_v)),
        width,
        height,
    );
    assert!(raised.well_e() < nominal.well_e());
    assert!(
        (raised.white_e() - nominal.white_e()).abs() < f32::EPSILON,
        "the bias moved reference white's charge, and with it the metering"
    );
    let held = expose(&nominal, &card, 0);
    let brightest = held.plane().data().iter().copied().fold(0.0, f32::max);
    assert!(
        brightest < nominal.well_e(),
        "the nominal well clipped the card at {brightest} e"
    );
    let clipped = expose(&raised, &card, 0);
    let at_well = clipped
        .plane()
        .data()
        .iter()
        .filter(|&&e| (e - raised.well_e()).abs() < f32::EPSILON * raised.well_e())
        .count();
    assert!(
        clipped.plane().data().iter().all(|&e| e <= raised.well_e()),
        "charge above the raised bias's well"
    );
    assert!(
        at_well > (width * height / 2) as usize,
        "only {at_well} photosites of the card clipped at the raised bias"
    );
}

#[test]
fn a_lowered_bias_blooms_a_bright_disc_along_its_column_and_a_raised_one_does_not() {
    let camera = camera();
    let (width, height) = (32, 256);
    let (cx, cy, radius) = (16u32, 64u32, 3u32);
    let nominal_v = camera.sensor.substrate_v;
    let at = |substrate_v: f64| {
        let preset = bent(bare(Mode::Still), None, Some(substrate_v));
        sensor_params(&camera, &preset, width, height)
    };
    let (nominal, low, raised) = (at(nominal_v), at(nominal_v - 2.0), at(nominal_v + 1.5));
    // A disc collecting `wells` nominal full wells.
    let disc = |wells: f32| {
        let radiance = wells * nominal.full_well_e / nominal.white_e();
        let mut scene = Scene::new(width, height);
        for plane in scene.planes_mut() {
            for y in cy - radius..=cy + radius {
                for x in cx - radius..=cx + radius {
                    if x.abs_diff(cx).pow(2) + y.abs_diff(cy).pow(2) <= radius * radius {
                        plane.set(x, y, radiance);
                    }
                }
            }
        }
        scene
    };
    // Photosites below the disc filled to their well, along the disc's centre column.
    let spill = |params: &sensor::Params, scene: &Scene| {
        let charge = expose(params, scene, 0);
        assert!(
            (0..width)
                .filter(|&x| x.abs_diff(cx) > radius)
                .all(|x| (0..height).all(|y| charge.plane().get(x, y) < 0.01 * params.well_e())),
            "charge left the disc's columns"
        );
        (cy + radius + 1..height)
            .take_while(|&y| charge.plane().get(cx, y) >= 0.999 * params.well_e())
            .count()
    };

    // Ten wells' worth: the drain takes the excess at the nominal bias and above it,
    // and two volts below, with the barrier raised, the drain is gone.
    let bright = disc(10.0);
    let (at_nominal, at_low, at_raised) = (
        spill(&nominal, &bright),
        spill(&low, &bright),
        spill(&raised, &bright),
    );
    assert_eq!(at_nominal, 0, "the nominal drain let the disc spill");
    assert_eq!(at_raised, 0, "the raised bias let the disc spill");
    assert!(
        at_low >= 10,
        "two volts low the disc spilled only {at_low} photosites"
    );

    // Twice what the nominal drain absorbs: it spills at every bias, the raised one no
    // further, the lowered one down the whole column.
    let gross = disc(2.0 * (1.0 + nominal.antibloom_ratio));
    let (at_nominal, at_low, at_raised) = (
        spill(&nominal, &gross),
        spill(&low, &gross),
        spill(&raised, &gross),
    );
    assert!(at_nominal >= 1, "the gross disc did not spill");
    assert!(
        at_raised <= at_nominal,
        "the raised bias spilled {at_raised} photosites, the nominal {at_nominal}"
    );
    assert!(
        at_low > 10 * at_nominal,
        "two volts low spilled {at_low} photosites, the nominal {at_nominal}"
    );
}

#[test]
fn a_low_bias_weakens_the_movie_shutter_and_leaves_stills_alone() {
    let camera = camera();
    let (width, height) = (64, 96);
    // A bright bar across the middle rows of the central columns, twice reference
    // white: it clips, but stays below the well two volts low, so nothing blooms and
    // the rows above it hold only dark charge and smear.
    let (lit, far) = (16..48, 0..16);
    let mut bar = Scene::new(width, height);
    for plane in bar.planes_mut() {
        for y in 40..56 {
            for x in lit.clone() {
                plane.set(x, y, 2.0);
            }
        }
    }
    let nominal_v = camera.sensor.substrate_v;
    for mode in [Mode::Movie, Mode::Still] {
        let charge = |substrate_v: f64| {
            let preset = bent(bare(mode), None, Some(substrate_v));
            expose(&sensor_params(&camera, &preset, width, height), &bar, 0)
        };
        // Charge above the bar in its columns over that in the unlit columns: the smear.
        let streak = |charge: &Charge| {
            let unlit = f64::midpoint(
                mean(charge, 0..16, far.clone()),
                mean(charge, 48..64, far.clone()),
            );
            mean(charge, lit.clone(), far.clone()) - unlit
        };
        // The rows above the bar, which hold only dark charge and smear.
        let far_rows =
            |charge: &Charge| charge.plane().data()[..(far.end * width) as usize].to_vec();
        let (nominal, low, slight) = (
            charge(nominal_v),
            charge(nominal_v - 2.0),
            charge(nominal_v - 0.5),
        );
        assert_eq!(
            far_rows(&slight),
            far_rows(&nominal),
            "{mode}: half a volt below nominal moved the smear"
        );
        match mode {
            Mode::Movie => {
                let (plain, weak) = (streak(&nominal), streak(&low));
                assert!(plain > 1.0, "no smear above the bar: {plain:.2} e");
                assert!(
                    weak > 1.5 * plain,
                    "two volts low smeared {weak:.1} e against {plain:.1} e"
                );
            }
            Mode::Still => {
                assert_eq!(
                    far_rows(&low),
                    far_rows(&nominal),
                    "the bias moved the still's dark rows"
                );
                assert!(streak(&nominal).abs() < 0.5, "a still smeared");
            }
        }
    }
}

#[test]
fn the_read_noise_rises_with_temperature_from_the_camera_s_own() {
    let camera = camera();
    let reference_c = camera.sensor.reference_temp_c;
    let scale = |temperature_c: Option<f64>| {
        sensor_params(
            &camera,
            &bent(bare(Mode::Still), temperature_c, None),
            WIDTH,
            HEIGHT,
        )
        .read_noise_scale()
    };
    let unbent = sensor_params(&camera, &bare(Mode::Still), WIDTH, HEIGHT).read_noise_scale();
    assert!((unbent - 1.0).abs() < f64::EPSILON, "{unbent}");
    assert!((scale(None) - 1.0).abs() < f64::EPSILON);
    assert!((scale(Some(reference_c)) - 1.0).abs() < f64::EPSILON);
    let (up_10, up_20) = (
        scale(Some(reference_c + 10.0)),
        scale(Some(reference_c + 20.0)),
    );
    assert!(up_10 > 1.0, "ten degrees warmer: {up_10}");
    assert!(
        ((up_20 - 1.0) / (up_10 - 1.0) - 2.0).abs() < 1e-9,
        "the rise is not linear: {up_10}, {up_20}"
    );
    let coldest = scale(Some(-40.0));
    assert!((0.0..1.0).contains(&coldest), "{coldest}");
}

#[cfg(feature = "test-util")]
#[test]
fn a_hot_sensor_reads_out_noisier_by_the_read_noise_scale() {
    let mut camera = camera();
    // Without dark charge the AFE's read noise is all that moves with the heat.
    camera.sensor.dark_e_per_s = 0.0;
    camera.sensor.dsnu_e = 0.0;
    camera.sensor.hot_pixels_per_mpix = 0.0;
    camera.sensor.amp_glow_e_per_s = 0.0;
    let (width, height) = (256, 192);
    let black = Scene::new(width, height);
    let hot_c = camera.sensor.reference_temp_c + 100.0;
    // The temporal noise of the ADC's words: half the variance of two frames' difference.
    let noise = |temperature_c: Option<f64>| {
        let preset = bent(clean(Mode::Still), temperature_c, None);
        let mut sim = Simulator::builder((camera.clone(), preset))
            .input_size(width, height)
            .build()
            .unwrap();
        sim.frame(&black, 0);
        let first: Vec<f64> = sim.codes().words().iter().map(|&w| f64::from(w)).collect();
        sim.frame(&black, 1);
        let differences: Vec<f64> = sim
            .codes()
            .words()
            .iter()
            .zip(&first)
            .map(|(&w, &v)| f64::from(w) - v)
            .collect();
        let count = differences.len() as f64;
        let mean = differences.iter().sum::<f64>() / count;
        let variance = differences.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / count;
        (variance / 2.0).sqrt()
    };
    let (cool, hot) = (noise(None), noise(Some(hot_c)));
    let scale = sensor_params(
        &camera,
        &bent(bare(Mode::Still), Some(hot_c), None),
        WIDTH,
        HEIGHT,
    )
    .read_noise_scale();
    assert!(
        scale > 1.1,
        "a hundred degrees raised the read noise by {scale}"
    );
    assert!(cool > 1.0, "the cool sensor reads {cool:.2} codes of noise");
    assert!(
        (hot / cool / scale - 1.0).abs() < 0.02,
        "the hot sensor reads {hot:.3} codes of noise, the cool {cool:.3}, for a scale of {scale:.3}"
    );
}

/// The fixed pattern and the mean of two exposures of a black scene, in electrons.
///
/// The pattern is what the two frames share: the root of their covariance across the
/// photosites, the shot noise of each frame being its own.
fn dark_pattern(params: &sensor::Params, black: &Scene) -> (f64, f64) {
    let (a, b) = (expose(params, black, 0), expose(params, black, 1));
    let (a, b) = (a.plane().data(), b.plane().data());
    let count = a.len() as f64;
    let mean_a = a.iter().map(|&e| f64::from(e)).sum::<f64>() / count;
    let mean_b = b.iter().map(|&e| f64::from(e)).sum::<f64>() / count;
    let covariance = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| (f64::from(x) - mean_a) * (f64::from(y) - mean_b))
        .sum::<f64>()
        / count;
    (covariance.sqrt(), mean_a)
}

#[test]
fn the_dark_frame_s_fixed_pattern_grows_in_proportion_to_its_dark_charge() {
    let mut camera = camera();
    camera.sensor.amp_glow_e_per_s = 0.0;
    camera.sensor.hot_pixels_per_mpix = 0.0;
    // The readout chain's offset pattern is tested apart; here the dark current's own
    // spread is the whole pattern.
    camera.sensor.dsnu_e = 0.0;
    let (width, height) = (256, 192);
    let black = Scene::new(width, height);
    let at = |camera: &Camera, temperature_c: f64, exposure_s: f64| {
        let preset = bent(
            exposed(bare(Mode::Still), exposure_s),
            Some(temperature_c),
            None,
        );
        sensor_params(camera, &preset, width, height)
    };
    let reference_c = camera.sensor.reference_temp_c;
    // Two hundred electrons of dark charge, then twice as long, then two doublings hotter.
    let base_s = 200.0 / camera.sensor.dark_e_per_s;
    let (room, room_mean) = dark_pattern(&at(&camera, reference_c, base_s), &black);
    let (longer, longer_mean) = dark_pattern(&at(&camera, reference_c, 2.0 * base_s), &black);
    let hotter_c = reference_c + 2.0 * camera.sensor.dark_doubling_c;
    let (hotter, hotter_mean) = dark_pattern(&at(&camera, hotter_c, base_s), &black);
    assert!(room > 0.05 * room_mean, "no dark pattern: {room:.2} e");
    // The pattern is a fixed fraction of the dark charge, whatever brought the charge.
    for (pattern_e, mean_e, what) in [
        (longer, longer_mean, "twice the exposure"),
        (hotter, hotter_mean, "two doublings hotter"),
    ] {
        assert!(
            (pattern_e / mean_e / (room / room_mean) - 1.0).abs() < 0.03,
            "{what}: a pattern of {pattern_e:.1} e on {mean_e:.0} e, against {room:.1} e on \
             {room_mean:.0} e"
        );
    }
    assert!(
        (longer / room - 2.0).abs() < 0.06,
        "{room:.1} e to {longer:.1} e"
    );
    assert!(
        (hotter / room - 4.0).abs() < 0.12,
        "{room:.1} e to {hotter:.1} e"
    );
    // At the camera's own exposure even a hot sensor collects too little dark charge
    // for a pattern worth an electron.
    let (short, _) = dark_pattern(&at(&camera, 70.0, camera.still.exposure_s), &black);
    assert!(
        short < 1.0,
        "a 70 C frame at the camera's exposure: {short:.2} e"
    );

    // Without dark current, the readout chain's offset pattern ignores the heat.
    let mut offsets = self::camera();
    offsets.sensor.dark_e_per_s = 0.0;
    offsets.sensor.amp_glow_e_per_s = 0.0;
    let charge = |temperature_c: f64| expose(&at(&offsets, temperature_c, base_s), &black, 0);
    assert!(offsets.sensor.dsnu_e > 0.0);
    assert_eq!(
        charge(reference_c),
        charge(hotter_c),
        "the heat moved the offset pattern"
    );
}

#[test]
fn a_hot_sensor_on_a_turned_bias_replays_from_reset() {
    let camera = camera();
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        let hot = bent(exposed(clean(mode), 0.05), Some(70.0), Some(13.5));
        let mut sim = build(camera.clone(), hot.clone());
        let first = render(&mut sim, &scene);
        sim.reset();
        assert_eq!(render(&mut sim, &scene), first, "{mode}: from reset");
        assert_eq!(
            render(&mut build(camera.clone(), hot), &scene),
            first,
            "{mode}: from a new simulator"
        );
        let low = bent(exposed(clean(mode), 0.05), Some(40.0), Some(9.0));
        let mut sim = build(camera.clone(), low.clone());
        let first = render(&mut sim, &scene);
        sim.reset();
        assert_eq!(
            render(&mut sim, &scene),
            first,
            "{mode}: a low bias from reset"
        );
    }
}

#[test]
fn heating_the_running_camera_keeps_its_state_and_shows_in_the_next_frame() {
    let camera = camera();
    let black = Scene::new(WIDTH, HEIGHT);
    // One movie frame period, which both readouts integrate in full.
    let exposure_s = 1.0 / camera.movie.fps;
    // Hot enough for about 2000 e of glow at the corner over that exposure.
    let temperature_c = camera.sensor.reference_temp_c
        + camera.sensor.dark_doubling_c
            * (1.0 + 2000.0 / (camera.sensor.amp_glow_e_per_s * exposure_s)).log2();
    for mode in [Mode::Still, Mode::Movie] {
        let cool = exposed(clean(mode), exposure_s);
        let mut heated = build(camera.clone(), cool.clone());
        let mut kept_cool = build(camera.clone(), cool.clone());
        render(&mut heated, &black);
        render(&mut kept_cool, &black);
        let hot = bent(cool.clone(), Some(temperature_c), None);
        assert_eq!(heated.set_preset(hot.clone()).unwrap(), KEPT, "{mode}");
        assert_eq!(heated.preset(), &hot);
        // The mean code of a sixteenth of the frame at its top left, the corner the
        // readout samples first, or at its bottom right.
        let sixteenth = |frame: &Frame, first: bool| {
            let (columns, rows) = ((frame.width() / 4) as usize, (frame.height() / 4) as usize);
            let (x0, y0) = if first {
                (0, 0)
            } else {
                (
                    frame.width() as usize - columns,
                    frame.height() as usize - rows,
                )
            };
            let rgb = frame.rgb();
            let stride = frame.width() as usize * 3;
            (y0..y0 + rows)
                .flat_map(|y| {
                    (x0 * 3..(x0 + columns) * 3).map(move |i| f64::from(rgb[y * stride + i]))
                })
                .sum::<f64>()
                / (columns * rows * 3) as f64
        };
        let warm = heated.frame(&black, FRAMES).clone();
        let cool_frame = kept_cool.frame(&black, FRAMES).clone();
        assert_ne!(
            warm, cool_frame,
            "{mode}: the heat did not reach the next frame"
        );
        // The heat makes the whole frame grainier; the glow lights the first corner.
        let (glowing, opposite) = (sixteenth(&warm, true), sixteenth(&warm, false));
        let still_cool = sixteenth(&cool_frame, true);
        assert!(
            glowing > still_cool + 20.0,
            "{mode}: the heated corner reads {glowing:.1}, the cool one {still_cool:.1}"
        );
        assert!(
            glowing > opposite + 15.0,
            "{mode}: the heated frame reads {glowing:.1} at its first corner, {opposite:.1} \
             at the opposite one"
        );
        // Turning the heat back off is a knob too.
        assert_eq!(heated.set_preset(cool).unwrap(), KEPT, "{mode}");
        heated.frame(&black, FRAMES + 1);
    }
}

#[test]
fn out_of_range_temperatures_and_biases_are_rejected() {
    let preset: Preset = toml::from_str(
        "camera = \"a520\"\n[bend.sensor]\ntemperature_c = 70.0\nsubstrate_v = 13.5",
    )
    .unwrap();
    preset.validate().unwrap();
    assert_eq!(
        preset.bend.sensor,
        Some(SensorBend {
            temperature_c: Some(70.0),
            substrate_v: Some(13.5),
        })
    );
    let empty: Preset = toml::from_str("camera = \"a520\"\n[bend.sensor]").unwrap();
    assert_eq!(empty.bend.sensor, Some(SensorBend::default()));
    let text = toml::to_string(&empty).unwrap();
    assert!(text.contains("[bend.sensor]"), "{text}");
    assert!(!text.contains("temperature_c"), "{text}");
    assert!(
        !toml::to_string(&Preset::new("a520"))
            .unwrap()
            .contains("sensor")
    );
    let round: Preset = toml::from_str(&toml::to_string(&preset).unwrap()).unwrap();
    assert_eq!(round, preset);
    let unknown =
        toml::from_str::<Preset>("camera = \"a520\"\n[bend.sensor]\nvsub = 12.0").unwrap_err();
    assert!(unknown.to_string().contains("vsub"), "{unknown}");

    let camera = camera();
    let timing = Timing::derive(&camera, &bare(Mode::Still), WIDTH, HEIGHT).unwrap();
    let invalid = [
        (Some(-41.0), None),
        (Some(151.0), None),
        (Some(f64::NAN), None),
        (None, Some(-0.1)),
        (None, Some(30.5)),
        (None, Some(f64::INFINITY)),
    ];
    for (temperature_c, substrate_v) in invalid {
        let preset = bent(bare(Mode::Still), temperature_c, substrate_v);
        assert!(
            preset.validate().unwrap_err().is_config(),
            "{temperature_c:?} {substrate_v:?}"
        );
        assert!(
            sensor::Params::resolve(&camera, &preset, &timing)
                .unwrap_err()
                .is_config()
        );
    }

    let checks: [Break; 9] = [
        ("reference_temp_c", |c| c.sensor.reference_temp_c = 200.0),
        ("dark_doubling_c", |c| c.sensor.dark_doubling_c = 0.0),
        ("read_noise_per_c", |c| c.sensor.read_noise_per_c = -0.01),
        ("amp_glow_e_per_s", |c| c.sensor.amp_glow_e_per_s = f64::NAN),
        ("amp_glow_radius_frac", |c| {
            c.sensor.amp_glow_radius_frac = 0.0;
        }),
        ("substrate_v", |c| c.sensor.substrate_v = 31.0),
        ("full_well_frac_per_v", |c| {
            c.sensor.full_well_frac_per_v = 1.5;
        }),
        ("drain_loss_v", |c| c.sensor.drain_loss_v = 0.0),
        ("dark_nonuniformity", |c| c.sensor.dark_nonuniformity = 1.5),
    ];
    for (field, break_it) in checks {
        let mut broken = camera.clone();
        break_it(&mut broken);
        let error = broken.validate().unwrap_err();
        assert!(error.is_config(), "{field}");
        assert!(error.to_string().contains(field), "{field}: {error}");
    }
}

#[test]
fn the_optical_black_integrates_the_same_exposure_as_the_image_in_both_modes() {
    // The electronic shutter clears masked photosites too, so a hot sensor's black
    // clamp must not subtract a frame period's dark from a shutter-length exposure.
    let camera = camera();
    for mode in [Mode::Still, Mode::Movie] {
        for exposure in [None, Some(0.002)] {
            let mut preset = bent(bare(mode), Some(120.0), None);
            if let Some(exposure_s) = exposure {
                preset = exposed(preset, exposure_s);
            }
            let timing = Timing::derive(&camera, &preset, 64, 48).unwrap();
            let readout = readout::Params::resolve(&camera, &preset, &timing).unwrap();
            let sensor = sensor_params(&camera, &preset, 64, 48);
            let image = f64::from(sensor.dark_e_per_s) * f64::from(sensor.exposure_s);
            let ob = f64::from(readout.dark_e);
            assert!(
                (ob - image).abs() <= 1e-3 * image,
                "{mode:?} with exposure {exposure:?}: optical black {ob} e, image {image} e"
            );
        }
    }
}
