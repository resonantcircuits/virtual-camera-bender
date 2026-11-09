//! The compressed buffer wired through the chain: while `[bend.memory] reaches_jpeg` is
//! set, the frame's JPEG scan passes the bent SDRAM between the encoder and the decoder;
//! otherwise the codec's direct round trip runs, and without a fault both give the same
//! bytes.

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::codec;
use vcb_core::memory;
use vcb_core::preset::{AddressFault, DataFault, FaultKind, MemoryBend, Mode, Resolution};
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing, noise};

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

/// The size of most tests' scenes, and of their frames at match-input resolution.
const SIZE: (u32, u32) = (160, 120);

fn camera_text() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    std::fs::read_to_string(path).unwrap()
}

fn camera() -> Camera {
    toml::from_str(&camera_text()).unwrap()
}

/// A landscape scene of gradients, hard edges and a little texture.
fn scene((width, height): (u32, u32)) -> Scene {
    let rgb: Vec<u8> = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let edge = if (x / 24 + y / 18) % 2 == 0 { 50 } else { 0 };
                let grain = noise::hash_u32(x, y, noise::salt("jpeg path scene"), 1) % 9;
                [
                    (x * 180 / width + edge + grain) as u8,
                    (y * 170 / height + 40 + grain) as u8,
                    ((x + y) * 90 / (width + height) + 100 - edge / 2) as u8,
                ]
            })
        })
        .collect();
    Scene::from_srgb8(width, height, &rgb)
}

/// A still at match-input resolution with `memory` as its memory bend.
fn preset(seed: u64, memory: Option<MemoryBend>) -> Preset {
    let mut preset = Preset::new("a520");
    preset.seed = seed;
    preset.readout.resolution = Resolution::MatchInput;
    preset.bend.memory = memory;
    preset
}

/// A memory bend with these faults that reaches the compressed buffer or not.
fn bend(faults: Vec<AddressFault>, data_faults: Vec<DataFault>, reaches_jpeg: bool) -> MemoryBend {
    MemoryBend {
        faults,
        data_faults,
        reaches_jpeg,
        ..MemoryBend::default()
    }
}

/// Address pin `pin` stuck low through a marginal contact.
fn flaky_address(pin: u8, on_frac: f64, switch_hz: f64) -> AddressFault {
    AddressFault {
        pin,
        kind: FaultKind::Low,
        with: None,
        on_frac: Some(on_frac),
        switch_hz: Some(switch_hz),
    }
}

/// Data pin `pin` stuck low through a marginal contact.
fn flaky_data(pin: u8, on_frac: f64, switch_hz: f64) -> DataFault {
    DataFault {
        pin,
        kind: FaultKind::Low,
        with: None,
        on_frac: Some(on_frac),
        switch_hz: Some(switch_hz),
    }
}

fn simulator(camera: &Camera, preset: &Preset, (width, height): (u32, u32)) -> Simulator {
    Simulator::builder((camera.clone(), preset.clone()))
        .input_size(width, height)
        .build()
        .unwrap()
}

/// A rendered frame, the length of its scan if it passed the SDRAM, and the scan as read back.
type Rendered = (Frame, Option<usize>, Option<Vec<u8>>);

/// Renders frames 0 to `frames - 1` from power-on.
fn run(camera: &Camera, preset: &Preset, size: (u32, u32), frames: u64) -> Vec<Rendered> {
    let mut sim = simulator(camera, preset, size);
    let scene = scene(size);
    (0..frames)
        .map(|index| {
            let frame = sim.frame(&scene, index).clone();
            (frame, sim.scan_len(), sim.scan().map(<[u8]>::to_vec))
        })
        .collect()
}

#[test]
fn reaches_jpeg_defaults_to_false_and_is_written_only_when_set() {
    let text = "camera = \"a520\"\n[bend.memory]\nreaches_jpeg = true\n";
    let parsed: Preset = toml::from_str(text).unwrap();
    parsed.validate().unwrap();
    let memory = parsed.bend.memory.as_ref().unwrap();
    assert!(memory.reaches_jpeg);
    let written = toml::to_string(&parsed).unwrap();
    assert!(written.contains("reaches_jpeg = true"), "{written}");
    assert_eq!(toml::from_str::<Preset>(&written).unwrap(), parsed);

    let plain: Preset = toml::from_str("camera = \"a520\"\n[bend.memory]\n").unwrap();
    assert!(
        !plain.bend.memory.as_ref().unwrap().reaches_jpeg,
        "the compressed buffer lies out of the faults' reach unless the preset says otherwise"
    );
    let written = toml::to_string(&plain).unwrap();
    assert!(!written.contains("reaches_jpeg"), "{written}");
    let error = toml::from_str::<Preset>("camera = \"a520\"\n[bend.memory]\nreaches_jpeg = 1\n")
        .unwrap_err();
    assert!(error.to_string().contains("bool"), "{error}");

    let camera = camera();
    let timing = Timing::derive(&camera, &parsed, 64, 48).unwrap();
    let params = memory::Params::resolve(&camera, &parsed, &timing)
        .unwrap()
        .unwrap();
    assert!(params.reaches_jpeg, "the switch reaches the memory stage");
    let params = memory::Params::resolve(&camera, &plain, &timing)
        .unwrap()
        .unwrap();
    assert!(!params.reaches_jpeg);
}

#[test]
fn the_camera_restart_interval_is_validated_and_feeds_the_codec() {
    let mut camera = camera();
    assert_eq!(
        camera.jpeg.restart_interval, 0,
        "the A520's files are taken to carry no restart markers"
    );
    let still = Preset::new("a520");
    let timing = Timing::derive(&camera, &still, 64, 48).unwrap();
    assert_eq!(
        codec::Params::resolve(&camera, &still, &timing)
            .unwrap()
            .restart_interval,
        0
    );
    camera.jpeg.restart_interval = 12;
    camera.validate().unwrap();
    assert_eq!(
        codec::Params::resolve(&camera, &still, &timing)
            .unwrap()
            .restart_interval,
        12
    );

    // A DRI segment holds the interval in 16 bits.
    camera.jpeg.restart_interval = 65_535;
    camera.validate().unwrap();
    camera.jpeg.restart_interval = 65_536;
    let error = camera.validate().unwrap_err();
    assert!(
        error.is_config() && error.to_string().contains("jpeg.restart_interval"),
        "{error}"
    );

    let text = camera_text();
    let without = text
        .lines()
        .filter(|line| !line.starts_with("restart_interval"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(without, text);
    let error = toml::from_str::<Camera>(&without).unwrap_err();
    assert!(error.to_string().contains("restart_interval"), "{error}");
}

#[test]
fn without_reaches_jpeg_a_fault_that_reaches_only_the_scan_leaves_the_frame_alone() {
    // At 284 by 212 the raw ends below the scan as natively, so a11, whose row bit is the
    // scan's base, reaches only the compressed buffer.
    let camera = camera();
    let size = (284, 212);
    let contact = vec![flaky_address(11, 0.3, 20.0)];
    let off = preset(1, Some(bend(contact.clone(), Vec::new(), false)));
    let timing = Timing::derive(&camera, &off, size.0, size.1).unwrap();
    let params = memory::Params::resolve(&camera, &off, &timing)
        .unwrap()
        .unwrap();
    let raw_end = params.address(params.pitch - 1, params.rows - 1) + 1;
    let bit = params.faults[0].bits[1].unwrap();
    assert!(
        raw_end <= 1 << bit && params.jpeg_base == 1 << bit,
        "the raw ends at {raw_end}, below a11's row bit {bit}, the scan's base {}",
        params.jpeg_base
    );

    let clean = run(&camera, &preset(1, None), size, 4);
    let raw_only = run(&camera, &off, size, 4);
    for (index, ((frame, len, scan), (clean_frame, ..))) in raw_only.iter().zip(&clean).enumerate()
    {
        assert_eq!(
            frame, clean_frame,
            "frame {index}: the raw reads back clean and the codec's direct round trip runs"
        );
        assert_eq!((len, scan), (&None, &None), "frame {index}");
    }

    // With the switch thrown the same contact reaches the scan.
    let on = preset(1, Some(bend(contact, Vec::new(), true)));
    let scanned = run(&camera, &on, size, 4);
    assert!(
        scanned
            .iter()
            .all(|(_, len, scan)| len.is_some_and(|len| len > 0)
                && scan.as_ref().map(Vec::len) == *len),
        "every frame's scan passes the SDRAM"
    );
    assert!(
        scanned
            .iter()
            .zip(&clean)
            .any(|((frame, ..), (clean_frame, ..))| frame != clean_frame),
        "a burst read across the contact's change decodes out of step"
    );
}

#[test]
fn with_reaches_jpeg_and_no_fault_the_frame_is_the_direct_round_trip_byte_for_byte() {
    for mode in [Mode::Still, Mode::Movie] {
        for size in [SIZE, (150, 98)] {
            for restart_interval in [0, 3] {
                let mut camera = camera();
                camera.jpeg.restart_interval = restart_interval;
                let mut direct = preset(7, None);
                direct.readout.mode = mode;
                let mut scanned = preset(7, Some(bend(Vec::new(), Vec::new(), true)));
                scanned.readout.mode = mode;
                let case = format!("{mode} {size:?}, restart interval {restart_interval}");
                let expected = run(&camera, &direct, size, 3);
                let actual = run(&camera, &scanned, size, 3);
                for (index, ((frame, len, scan), (direct_frame, direct_len, _))) in
                    actual.iter().zip(&expected).enumerate()
                {
                    assert_eq!(frame, direct_frame, "{case}, frame {index}");
                    assert_eq!(*direct_len, None, "{case}, frame {index}");
                    let scan = scan.as_ref().expect("the scan passed the SDRAM");
                    assert_eq!(*len, Some(scan.len()), "{case}, frame {index}");
                    let markers = scan
                        .windows(2)
                        .filter(|pair| pair[0] == 0xFF && (0xD0..=0xD7).contains(&pair[1]))
                        .count();
                    assert_eq!(
                        markers > 0,
                        restart_interval > 0,
                        "{case}, frame {index}: {markers} restart markers"
                    );
                }
            }
        }
    }
}

#[test]
fn a_marginal_data_contact_damages_the_frame_from_its_first_flip() {
    // d11 carries no sample bit, so the raw is the clean camera's and only the scan
    // takes the fault: bit 3 of every high byte where the contact closes.
    let camera = camera();
    let width = SIZE.0 as usize;
    let mut checked = 0;
    for seed in 0..200 {
        let faulted = preset(
            seed,
            Some(bend(Vec::new(), vec![flaky_data(11, 0.2, 10.0)], true)),
        );
        let clean = preset(seed, Some(bend(Vec::new(), Vec::new(), true)));
        let raw_only = preset(
            seed,
            Some(bend(Vec::new(), vec![flaky_data(11, 0.2, 10.0)], false)),
        );
        let [(direct, ..)] = run(&camera, &raw_only, SIZE, 1).try_into().unwrap();
        let [(intact, _, intact_scan)] = run(&camera, &clean, SIZE, 1).try_into().unwrap();
        assert_eq!(
            direct, intact,
            "seed {seed}: the raw-only path is the clean frame"
        );

        let mut sim = simulator(&camera, &faulted, SIZE);
        let frame = sim.frame(&scene(SIZE), 0).clone();
        let scan = sim.scan().unwrap().to_vec();
        let intact_scan = intact_scan.unwrap();
        assert_eq!(
            scan.len(),
            intact_scan.len(),
            "seed {seed}: the faults keep the length"
        );
        let Some(first) = scan.iter().zip(&intact_scan).position(|(a, b)| a != b) else {
            assert_eq!(
                frame, direct,
                "seed {seed}: an untouched scan decodes as the direct path"
            );
            continue;
        };
        assert_ne!(
            frame, direct,
            "seed {seed}: a flipped bit at byte {first} shows"
        );

        // The contact was clear over the bursts before the first flip.
        let timing = Timing::derive(&camera, &faulted, SIZE.0, SIZE.1).unwrap();
        let params = memory::Params::resolve(&camera, &faulted, &timing)
            .unwrap()
            .unwrap();
        let (writes, reads) = sim.memory_state().scan_data_contact_bursts(11).unwrap();
        let faulted_burst = writes
            .iter()
            .zip(reads)
            .position(|(&write, &read)| write || read)
            .expect("a flip needs the contact closed at a burst");
        assert!(
            first >= 2 * faulted_burst * params.pitch as usize,
            "seed {seed}: byte {first} lies in or after burst {faulted_burst}"
        );

        // The MCU rows whose bits all lie before the flip decode as in the direct path:
        // those that the scan cut short at the flip still decodes in full.
        let codec = codec::Params::resolve(&camera, &faulted, &timing).unwrap();
        let mut cut = Frame::new(SIZE.0, SIZE.1);
        codec::decode_scan(
            &codec,
            &mut codec::State::default(),
            &intact_scan[..first],
            &mut cut,
        );
        let row_bytes = width * 3 * 8 * codec.subsampling.factors().1 as usize;
        let rows = cut
            .rgb()
            .chunks(row_bytes)
            .zip(direct.rgb().chunks(row_bytes))
            .take_while(|(a, b)| a == b)
            .count();
        let split = rows * row_bytes;
        assert_eq!(
            frame.rgb()[..split],
            direct.rgb()[..split],
            "seed {seed}: the {rows} MCU rows before byte {first} decode where the contact was clear"
        );
        if rows >= 2 {
            checked += 1;
        }
        if checked >= 3 {
            break;
        }
    }
    assert!(
        checked >= 3,
        "only {checked} seeds held the contact clear over two MCU rows and then flipped a bit"
    );
}

#[test]
fn a_run_through_the_scan_replays_from_reset_byte_for_byte() {
    let camera = camera();
    let preset = preset(
        5,
        Some(bend(
            vec![flaky_address(11, 0.3, 20.0)],
            vec![flaky_data(11, 0.2, 10.0)],
            true,
        )),
    );
    let scene = scene(SIZE);
    let mut sim = simulator(&camera, &preset, SIZE);
    let render = |sim: &mut Simulator| -> Vec<Rendered> {
        (0..5)
            .map(|index| {
                let frame = sim.frame(&scene, index).clone();
                (frame, sim.scan_len(), sim.scan().map(<[u8]>::to_vec))
            })
            .collect()
    };
    let first = render(&mut sim);
    sim.reset();
    let again = render(&mut sim);
    assert!(first == again, "the same sequence from reset is identical");
    let fresh = render(&mut simulator(&camera, &preset, SIZE));
    assert!(
        first == fresh,
        "a fresh simulator renders the same sequence"
    );
    let clean = run(&camera, &self::preset(5, None), SIZE, 5);
    assert!(
        first.iter().zip(&clean).any(|((a, ..), (b, ..))| a != b),
        "the contacts damage some frame of the run"
    );
}

#[test]
fn throwing_the_switch_keeps_the_camera_running() {
    let camera = camera();
    let scene = scene(SIZE);
    let mut preset = preset(
        3,
        Some(bend(vec![flaky_address(11, 0.3, 20.0)], Vec::new(), false)),
    );
    let mut sim = simulator(&camera, &preset, SIZE);
    assert_eq!(sim.scan_len(), None, "no frame yet");
    sim.frame(&scene, 0);
    assert_eq!(sim.scan_len(), None, "the direct round trip");

    let before = sim.memory_state().clone();
    preset.bend.memory.as_mut().unwrap().reaches_jpeg = true;
    assert_eq!(sim.set_preset(preset.clone()).unwrap(), KEPT);
    assert_eq!(
        sim.memory_state(),
        &before,
        "the SDRAM keeps its words and contacts"
    );
    assert_eq!(sim.scan_len(), None, "the last frame's");
    sim.frame(&scene, 1);
    let len = sim.scan_len().expect("the scan passed the SDRAM");
    assert_eq!(sim.scan().map(<[u8]>::len), Some(len));

    preset.bend.memory.as_mut().unwrap().reaches_jpeg = false;
    assert_eq!(sim.set_preset(preset.clone()).unwrap(), KEPT);
    sim.frame(&scene, 2);
    assert_eq!(sim.scan_len(), None);
    assert_eq!(sim.scan(), None);

    // Removing the bend with the switch thrown forgets the SDRAM, as without it.
    preset.bend.memory.as_mut().unwrap().reaches_jpeg = true;
    assert_eq!(sim.set_preset(preset.clone()).unwrap(), KEPT);
    sim.frame(&scene, 3);
    preset.bend.memory = None;
    assert_eq!(sim.set_preset(preset).unwrap(), KEPT);
    assert_eq!(sim.memory_state(), &memory::State::default());
    sim.frame(&scene, 4);
    assert_eq!(sim.scan_len(), None);
}
