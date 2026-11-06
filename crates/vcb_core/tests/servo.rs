//! The lagging auto-exposure and auto-white-balance servos.

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::isp::{AeMode, WbMode};
use vcb_core::preset::{IspOverride, Resolution};
use vcb_core::{Camera, Frame, Preset, Scene, Simulator};

const WIDTH: u32 = 32;
const HEIGHT: u32 = 24;

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The A520 as a linear meter: no noise, fixed pattern or vignetting, and a well so
/// deep that shot noise vanishes, so the raw is proportional to the exposure.
fn neutral_camera() -> Camera {
    let mut camera = camera();
    let sensor = &mut camera.sensor;
    sensor.full_well_e *= 1e5;
    sensor.conv_gain_uv_per_e /= 1e5;
    sensor.prnu = 0.0;
    sensor.dsnu_e = 0.0;
    sensor.dark_e_per_s = 0.0;
    sensor.hot_pixels_per_mpix = 0.0;
    sensor.read_noise_e = 0.0;
    sensor.cte = 1.0;
    camera.afe.noise_1f_corner_hz = 0.0;
    camera.lens.vignette_stops = 0.0;
    camera
}

/// The clean still preset at the scene's size, with the input taken as scene radiance.
fn preset() -> Preset {
    let mut preset = Preset::new("a520");
    preset.seed = 1;
    preset.readout.resolution = Resolution::MatchInput;
    preset.scene.headroom = 1.0;
    preset
}

fn build(camera: Camera, preset: Preset) -> Simulator {
    Simulator::builder((camera, preset))
        .input_size(WIDTH, HEIGHT)
        .build()
        .unwrap()
}

/// A flat scene of the given linear sRGB value in every channel.
fn flat(linear: [f32; 3]) -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    for (plane, value) in scene.planes_mut().into_iter().zip(linear) {
        plane.data_mut().fill(value);
    }
    scene
}

/// Returns the frame period of the preset on the camera.
fn frame_period_s(camera: &Camera, preset: &Preset) -> f64 {
    build(camera.clone(), preset.clone())
        .timing()
        .frame_period_s
}

/// Returns the mean of every channel of a frame, in 8-bit codes.
fn means(frame: &Frame) -> [f64; 3] {
    let mut sums = [0.0; 3];
    for pixel in frame.rgb().as_chunks::<3>().0 {
        for (sum, &value) in sums.iter_mut().zip(pixel) {
            *sum += f64::from(value);
        }
    }
    let count = f64::from(frame.width() * frame.height());
    sums.map(|sum| sum / count)
}

#[test]
fn the_exposure_follows_a_step_in_brightness_with_its_time_constant() {
    let mut camera = neutral_camera();
    camera.isp.ae_mode = AeMode::Servo;
    let period = frame_period_s(&camera, &preset());
    // Ten frames per time constant resolve the approach.
    camera.isp.ae_tau_s = 10.0 * period;
    let mut sim = build(camera, preset());

    // Settle on a mid-grey scene, then halve its brightness.
    let bright = flat([0.18; 3]);
    let dark = flat([0.09; 3]);
    let mut index = 0;
    for _ in 0..100 {
        sim.frame(&bright, index);
        index += 1;
    }
    let before = sim.exposure_scale();
    assert!(
        (before - 1.0).abs() < 0.03,
        "mid-grey settled at an exposure scale of {before}"
    );
    let mut stops = Vec::new();
    for _ in 0..120 {
        sim.frame(&dark, index);
        index += 1;
        stops.push((sim.exposure_scale() / before).log2());
    }
    let after = *stops.last().unwrap();
    assert!(
        (after - 1.0).abs() < 0.03,
        "half the light settled {after} stops up"
    );
    // The first dark frame is exposed as metered on the last bright one; each frame
    // after it corrects a fixed share of the remaining error.
    assert!(
        stops[0].abs() < 0.01,
        "the servo reacted within the frame: {}",
        stops[0]
    );
    let at_tau = stops[10] / after;
    assert!(
        (at_tau - (1.0 - (-1.0f32).exp())).abs() < 0.02,
        "{at_tau} of the step after one time constant"
    );
    let step = 1.0 - (-period / (10.0 * period)).exp() as f32;
    for (k, &measured) in (0i32..).zip(stops.iter().take(40)) {
        let expected = 1.0 - (1.0 - step).powi(k);
        assert!(
            (measured / after - expected).abs() < 0.02,
            "frame {k}: {measured} stops, expected {expected} of the step"
        );
    }
}

#[test]
fn the_white_balance_converges_on_a_flat_grey_scene() {
    let camera = neutral_camera();
    let mut preset = preset();
    let period = frame_period_s(&camera, &preset);
    // The processor starts from gains of 1, far from the sensor's daylight response.
    preset.isp = Some(IspOverride {
        wb: Some(WbMode::Servo),
        wb_r_gain: Some(1.0),
        wb_b_gain: Some(1.0),
        wb_tau_s: Some(5.0 * period),
        ..IspOverride::default()
    });
    let mut sim = build(camera.clone(), preset);
    let grey = flat([0.18; 3]);
    let first = means(sim.frame(&grey, 0));
    assert!(
        first[1] - first[0] > 10.0 && first[1] - first[2] > 10.0,
        "the first frame is not green: {first:?}"
    );
    let mut gains = Vec::new();
    let mut last = first;
    for index in 1..80 {
        last = means(sim.frame(&grey, index));
        gains.push(sim.isp_state().servo_gains().unwrap());
    }
    // Grey world on the sensor's raw: the camera's daylight gains.
    let [red, blue] = *gains.last().unwrap();
    assert!(
        (red / camera.isp.wb_r_gain as f32 - 1.0).abs() < 0.01
            && (blue / camera.isp.wb_b_gain as f32 - 1.0).abs() < 0.01,
        "the gains settled at {red}, {blue}"
    );
    let spread =
        last.iter().fold(f64::MIN, |a, &b| a.max(b)) - last.iter().fold(f64::MAX, |a, &b| a.min(b));
    assert!(spread < 2.0, "the grey frame settled at {last:?}");
    // Each frame closes a share of the gap: the gains rise without overshooting.
    let ceiling = [camera.isp.wb_r_gain as f32, camera.isp.wb_b_gain as f32];
    assert!(
        gains
            .iter()
            .all(|g| g[0] <= ceiling[0] * 1.005 && g[1] <= ceiling[1] * 1.005),
        "the gains overshoot"
    );
    assert!(
        gains[..20]
            .windows(2)
            .all(|w| w[1][0] > w[0][0] && w[1][1] > w[0][1]),
        "the gains do not rise steadily"
    );
}

#[test]
fn fixed_exposure_and_daylight_balance_carry_nothing_between_frames() {
    let camera = neutral_camera();
    let mut sim = build(camera.clone(), preset());
    let bright = flat([0.3, 0.2, 0.1]);
    let dark = flat([0.05, 0.1, 0.02]);
    for index in 0..5 {
        sim.frame(&bright, index);
        assert_eq!(sim.exposure_scale().to_bits(), 1.0f32.to_bits());
        assert_eq!(sim.isp_state().servo_exposure(), None);
        assert_eq!(sim.isp_state().servo_gains(), None);
    }
    let after_bright = sim.frame(&dark, 5).clone();
    let mut fresh = build(camera, preset());
    assert_eq!(fresh.frame(&dark, 5), &after_bright);
}

#[test]
fn scene_metering_follows_a_step_without_lag() {
    let mut camera = neutral_camera();
    camera.isp.ae_mode = AeMode::Scene;
    let mut sim = build(camera, preset());
    sim.frame(&flat([0.18; 3]), 0);
    let before = sim.exposure_scale();
    sim.frame(&flat([0.09; 3]), 1);
    let ratio = sim.exposure_scale() / before;
    assert!((ratio - 2.0).abs() < 0.01, "the exposure moved by {ratio}");
}

#[test]
fn a_still_with_servos_is_a_still_with_scene_metering_and_daylight() {
    let scene = Scene::from_srgb8(
        WIDTH,
        HEIGHT,
        &[150, 110, 70].repeat((WIDTH * HEIGHT) as usize),
    );
    let render = |ae_mode, wb| {
        let mut camera = camera();
        camera.isp.ae_mode = ae_mode;
        let mut preset = preset();
        preset.isp = Some(IspOverride {
            wb: Some(wb),
            ..IspOverride::default()
        });
        build(camera, preset).frame(&scene, 0).clone()
    };
    assert_eq!(
        render(AeMode::Servo, WbMode::Servo),
        render(AeMode::Scene, WbMode::Daylight)
    );
}

#[test]
fn the_preset_can_turn_the_exposure_servo_on() {
    let mut preset = preset();
    preset.isp = Some(IspOverride {
        ae_mode: Some(AeMode::Servo),
        ae_tau_s: Some(0.1),
        ..IspOverride::default()
    });
    let mut sim = build(neutral_camera(), preset);
    sim.frame(&flat([0.18; 3]), 0);
    assert!(sim.isp_state().servo_exposure().is_some());

    let mut invalid = sim.preset().clone();
    invalid.isp.as_mut().unwrap().ae_tau_s = Some(0.0);
    assert!(sim.set_preset(invalid).unwrap_err().is_config());
}

#[test]
fn servos_replay_identically_from_reset() {
    let mut camera = camera();
    camera.isp.ae_mode = AeMode::Servo;
    let mut preset = preset();
    preset.isp = Some(IspOverride {
        wb: Some(WbMode::Servo),
        ..IspOverride::default()
    });
    let scenes = [flat([0.4, 0.3, 0.1]), flat([0.05, 0.08, 0.2])];
    let mut sim = build(camera, preset);
    let sequence = |sim: &mut Simulator| {
        (0..12u64)
            .map(|index| {
                let frame = sim.frame(&scenes[(index / 4 % 2) as usize], index).clone();
                (frame, sim.exposure_scale(), sim.isp_state().servo_gains())
            })
            .collect::<Vec<_>>()
    };
    let first = sequence(&mut sim);
    sim.reset();
    assert_eq!(sequence(&mut sim), first);
    assert!(
        first.windows(2).any(|w| (w[0].1 - w[1].1).abs() > 1e-3),
        "the exposure never moved"
    );
}
