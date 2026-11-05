//! Physical behaviour of individual stages, driven through their public functions.

#![cfg(feature = "serde")]

use vcb_core::adc::{self, Codes};
use vcb_core::afe::{self, Analog};
use vcb_core::isp::{self, Raw};
use vcb_core::plane::Plane;
use vcb_core::readout::{self, Direction, Layout, Stream};
use vcb_core::scene::{self, Channel};
use vcb_core::sensor::{self, Cfa, Charge};
use vcb_core::{Camera, Preset, Scene, Timing};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn timing(camera: &Camera, width: u32, height: u32) -> Timing {
    Timing::derive(camera, &Preset::new("a520"), width, height).unwrap()
}

#[test]
fn plane_rows_and_samples_address_the_same_storage() {
    let mut plane = Plane::<u16>::new(5, 3);
    plane.row_mut(2)[4] = 7;
    plane.set(1, 0, 9);
    assert_eq!(plane.get(4, 2), 7);
    assert_eq!(plane.data()[1], 9);
    assert_eq!(plane.rows().len(), 3);
    assert!(plane.rows().all(|row| row.len() == 5));
    for row in plane.rows_mut() {
        row.fill(1);
    }
    assert!(plane.data().iter().all(|&v| v == 1));
}

#[test]
#[should_panic(expected = "needs 6")]
fn plane_from_vec_rejects_a_wrong_length() {
    let _ = Plane::from_vec(2, 3, vec![0.0f32; 5]);
}

#[test]
fn scene_srgb_round_trip_is_exact_for_every_code() {
    let rgb: Vec<u8> = (0..=255u8).flat_map(|v| [v, 255 - v, v / 2]).collect();
    let scene = Scene::from_srgb8(256, 1, &rgb);
    assert_eq!(scene.to_srgb8(), rgb);
}

#[test]
fn bayer_sites_sample_their_own_channel() {
    assert_eq!(Cfa::Rggb.channel(0, 0), Channel::Red);
    assert_eq!(Cfa::Rggb.channel(1, 0), Channel::Green);
    assert_eq!(Cfa::Rggb.channel(0, 1), Channel::Green);
    assert_eq!(Cfa::Rggb.channel(1, 1), Channel::Blue);
    assert_eq!(Cfa::Rggb.channel(6, 4), Cfa::Rggb.channel(0, 0));

    let camera = camera();
    let t = timing(&camera, 4, 4);
    let params = sensor::Params::resolve(&camera, &Preset::new("a520"), &t).unwrap();
    let mut scene = Scene::new(4, 4);
    scene.plane_mut(Channel::Red).fill(0.2);
    scene.plane_mut(Channel::Green).fill(0.4);
    scene.plane_mut(Channel::Blue).fill(0.8);
    let mut charge = Charge::new(4, 4);
    sensor::expose(
        &params,
        &mut sensor::State::default(),
        &scene,
        0,
        &mut charge,
    );
    let site = |x, y| charge.plane().get(x, y);
    assert!((site(1, 0) / site(0, 0) - 2.0).abs() < 1e-5);
    assert!((site(1, 1) / site(0, 1) - 2.0).abs() < 1e-5);
    assert!((site(0, 1) - site(1, 0)).abs() < 1e-3);
    let white = params.white_fraction * params.full_well_e;
    assert!((site(0, 0) - 0.2 * white).abs() < 1e-2);
}

#[test]
fn charge_clips_at_full_well_and_never_goes_negative() {
    let camera = camera();
    let t = timing(&camera, 4, 2);
    let params = sensor::Params::resolve(&camera, &Preset::new("a520"), &t).unwrap();
    let mut scene = Scene::new(4, 2);
    for (i, plane) in scene.planes_mut().into_iter().enumerate() {
        plane.fill(if i == 1 { -0.5 } else { 10.0 });
    }
    let mut charge = Charge::new(4, 2);
    sensor::expose(
        &params,
        &mut sensor::State::default(),
        &scene,
        0,
        &mut charge,
    );
    for y in 0..2 {
        for x in 0..4 {
            let expected = match Cfa::Rggb.channel(x, y) {
                Channel::Green => 0.0,
                _ => params.full_well_e,
            };
            assert!((charge.plane().get(x, y) - expected).abs() < 1e-3);
        }
    }
}

#[test]
fn readout_prefixes_every_line_with_dark_optical_black() {
    let camera = camera();
    let t = timing(&camera, 8, 4);
    let mut charge = Charge::new(8, 4);
    for (i, value) in charge.plane_mut().data_mut().iter_mut().enumerate() {
        *value = 100.0 + i as f32;
    }
    let mut params = readout::Params::resolve(&camera, &Preset::new("a520"), &t).unwrap();
    params.cte = 1.0;
    let layout = params.layout.clone();
    let mut stream = Stream::new(layout.clone());
    readout::read(
        &params,
        &mut readout::State::default(),
        &charge,
        0,
        &mut stream,
    );
    let line_len = layout.line_len() as usize;
    let ob = layout.ob_cols() as usize;
    assert!(params.dark_e > 0.0);
    for (line, samples) in stream.samples().chunks_exact(line_len).enumerate() {
        assert!(
            samples[..ob]
                .iter()
                .all(|&s| (s - params.dark_e).abs() <= f32::EPSILON)
        );
        assert_eq!(&samples[ob..], charge.plane().row(line as u32));
    }
}

#[test]
fn adc_codes_rise_with_voltage_and_clip_at_full_scale() {
    let camera = camera();
    let t = timing(&camera, 4, 2);
    let preset = Preset::new("a520");
    let params = adc::Params::resolve(&camera, &preset, &t).unwrap();
    let layout = Layout::from_timing(&t, Direction::TopDown);
    let mut analog = Analog::new(layout.clone());
    let count = analog.volts().len();
    for (i, volts) in analog.volts_mut().iter_mut().enumerate() {
        *volts = -0.5 + 3.5 * i as f32 / (count - 1) as f32;
    }
    let mut codes = Codes::new(layout);
    adc::convert(&params, &analog, &mut codes);
    assert!(codes.words().windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(codes.words()[0], 0);
    assert_eq!(*codes.words().last().unwrap(), params.max_code());
}

#[test]
fn analog_pedestal_lands_on_the_nominal_black_code() {
    let camera = camera();
    let t = timing(&camera, 4, 2);
    let preset = Preset::new("a520");
    let layout = Layout::from_timing(&t, Direction::TopDown);
    let stream = Stream::new(layout.clone());
    let mut analog = Analog::new(layout.clone());
    let mut afe_params = afe::Params::resolve(&camera, &preset, &t).unwrap();
    afe_params.read_noise_e = 0.0;
    afe_params.noise_1f_corner_hz = 0.0;
    afe::process(
        &afe_params,
        &mut afe::State::default(),
        &stream,
        0,
        &mut analog,
    );
    let mut codes = Codes::new(layout);
    adc::convert(
        &adc::Params::resolve(&camera, &preset, &t).unwrap(),
        &analog,
        &mut codes,
    );
    let black = i32::from(camera.adc.black_level);
    assert!(
        codes
            .words()
            .iter()
            .all(|&c| (i32::from(c) - black).abs() <= 1)
    );
}

#[test]
fn black_clamp_reads_the_bus_as_the_processor_does() {
    let camera = camera();
    let t = timing(&camera, 6, 4);
    let params = isp::Params::resolve(&camera, &Preset::new("a520"), &t).unwrap();
    let shift = camera.adc.dsp_lsb;
    // Bus lines below `dsp_lsb` are not connected to the processor: set them all.
    let unread = (1u16 << shift) - 1;
    let layout = Layout::from_timing(&t, Direction::BottomUp);
    let mut codes = Codes::new(layout.clone());
    let (ob, line_len) = (layout.ob_cols() as usize, layout.line_len() as usize);
    for (line, words) in codes.words_mut().chunks_exact_mut(line_len).enumerate() {
        let black: u16 = if line % 2 == 0 { 30 } else { 34 };
        words[..ob].fill((black << shift) | unread);
        for (x, word) in words[ob..].iter_mut().enumerate() {
            *word = ((250 + 10 * line as u16 + x as u16) << shift) | unread;
        }
    }
    let mut raw = Raw::new(layout.cols(), layout.rows());
    isp::clamp_black(&params, &codes, &mut raw);
    assert!((raw.black_level() - 32.0).abs() < 1e-4);
    // Bottom-up readout: the first line read is the last image row.
    let last = layout.rows() - 1;
    assert!((raw.plane().get(0, last) - (250.0 - 32.0)).abs() < 1e-4);
    let top = 250.0 + 10.0 * last as f32 + 2.0 - 32.0;
    assert!((raw.plane().get(2, 0) - top).abs() < 1e-4);
}

fn checker(width: u32, height: u32) -> Scene {
    let mut scene = Scene::new(width, height);
    for plane in scene.planes_mut() {
        for (y, row) in (0..height).zip(plane.rows_mut()) {
            for (x, value) in (0..width).zip(row.iter_mut()) {
                *value = if (x + y) % 2 == 0 { 1.0 } else { 0.0 };
            }
        }
    }
    scene
}

fn scene_params(camera: &Camera, width: u32, height: u32) -> scene::Params {
    scene::Params::resolve(camera, &Preset::new("a520"), &timing(camera, width, height)).unwrap()
}

#[test]
fn downscaling_averages_the_light_over_each_photosite() {
    let camera = camera();
    let params = scene_params(&camera, 16, 12);
    let mut out = Scene::new(16, 12);
    scene::prepare(&params, &checker(64, 48), &mut out);
    let green = out.plane(Channel::Green).data();
    assert!(green.iter().all(|&v| (v - 0.5).abs() < 1e-5));
}

#[test]
fn grey_reaches_the_sensor_divided_by_the_white_balance_gains() {
    let camera = camera();
    let params = scene_params(&camera, 16, 12);
    for (width, height) in [(17, 13), (40, 10), (16, 12)] {
        let mut input = Scene::new(width, height);
        for plane in input.planes_mut() {
            plane.fill(0.3);
        }
        let mut out = Scene::new(16, 12);
        scene::prepare(&params, &input, &mut out);
        for (channel, gain) in Channel::ALL.into_iter().zip(params.wb_gains) {
            let expected = 0.3 / gain;
            assert!(
                out.plane(channel)
                    .data()
                    .iter()
                    .all(|&v| (v - expected).abs() < 1e-5),
                "{width}x{height}: {channel:?} is not {expected} everywhere"
            );
        }
    }
}

#[test]
fn out_of_gamut_colours_collect_no_negative_charge() {
    let camera = camera();
    let params = scene_params(&camera, 8, 6);
    let mut red = Scene::new(8, 6);
    red.plane_mut(Channel::Red).fill(1.0);
    let mut out = Scene::new(8, 6);
    scene::prepare(&params, &red, &mut out);
    for plane in out.planes() {
        assert!(plane.data().iter().all(|&v| v >= 0.0));
    }
    assert!(out.plane(Channel::Blue).data().iter().all(|&v| v == 0.0));
}

#[test]
fn portrait_scenes_turn_clockwise_onto_the_sensor() {
    let camera = camera();
    let t = timing(&camera, 4, 6);
    assert!(t.rotated);
    let params = scene::Params::resolve(&camera, &Preset::new("a520"), &t).unwrap();
    let mut input = Scene::new(4, 6);
    for plane in input.planes_mut() {
        plane.set(0, 0, 1.0);
    }
    let mut out = Scene::new(t.sensor_width, t.sensor_height);
    scene::prepare(&params, &input, &mut out);
    let green = out.plane(Channel::Green);
    assert_eq!((green.width(), green.height()), (6, 4));
    assert!(
        (green.get(5, 0) - 1.0).abs() < 1e-5,
        "top-left did not turn to top-right"
    );
    assert!(green.data().iter().filter(|&&v| v > 0.0).count() == 1);
}
