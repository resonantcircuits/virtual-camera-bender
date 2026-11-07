//! The serial bend: bit errors on the link over which the processor programs the AFE.

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::afe::{
    self, CLAMP_ADDRESS, CONTROL_ADDRESS, OUTPUT_DISABLE, POWER_DOWN, PXGA_ADDRESSES, REGISTERS,
    VGA_ADDRESS,
};
use vcb_core::preset::{Mode, Resolution, SerialBend};
use vcb_core::readout::Direction;
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing};

/// Scene size of the tests, at match-input resolution.
const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

/// Frames rendered from reset in each whole-camera comparison.
const FRAMES: u64 = 3;

/// Bit error rate of the searches for a frame with chosen flips.
///
/// About one of the hundred bits that matter flips per frame, which makes a frame
/// with exactly one chosen flip about as likely as it can be.
const SEARCH_BIT_ERROR: f64 = 0.01;

/// Frames a search looks through before it gives up.
const SEARCH_FRAMES: u64 = 200_000;

/// The sRGB level of the mid-grey scene.
const MID_GREY: u8 = 118;

/// The sRGB level of a grey dark enough that 18 dB more gain does not clip it.
const DARK_GREY: u8 = 60;

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

/// Changes a camera so that one of its values no longer fits its register.
type BreakCamera = fn(&mut Camera);

/// The flipped address bits and the flipped data bits of each write, by intended address.
type Flips = [(u8, u32); REGISTERS];

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A preset in `mode` at match-input resolution with `[bend.serial]` at `bit_error`.
fn with_serial(mode: Mode, bit_error: Option<f64>) -> Preset {
    let mut preset = Preset::new("a520");
    preset.readout.mode = mode;
    preset.readout.resolution = Resolution::MatchInput;
    preset.bend.serial = bit_error.map(|bit_error| SerialBend { bit_error });
    preset
}

fn build_with(camera: Camera, preset: Preset) -> Simulator {
    Simulator::builder((camera, preset))
        .input_size(WIDTH, HEIGHT)
        .build()
        .unwrap()
}

fn build(preset: Preset) -> Simulator {
    build_with(camera(), preset)
}

/// A uniform scene at sRGB level `level`.
fn grey(level: u8) -> Scene {
    Scene::from_srgb8(WIDTH, HEIGHT, &vec![level; (WIDTH * HEIGHT * 3) as usize])
}

/// A white sky over a grey gradient and a coloured band.
fn scene() -> Scene {
    let rgb: Vec<u8> = (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).flat_map(move |x| match y * 3 / HEIGHT {
                0 => [255, 255, 255],
                1 => {
                    let v = (x * 255 / WIDTH) as u8;
                    [v, v, v]
                }
                _ => [200, 60, 40],
            })
        })
        .collect();
    Scene::from_srgb8(WIDTH, HEIGHT, &rgb)
}

fn render(sim: &mut Simulator, scene: &Scene) -> Vec<Frame> {
    (0..FRAMES)
        .map(|index| sim.frame(scene, index).clone())
        .collect()
}

/// The AFE's parameters of `preset` on the camera.
fn afe_params(preset: &Preset) -> afe::Params {
    let camera = camera();
    let timing = Timing::derive(&camera, preset, WIDTH, HEIGHT).unwrap();
    afe::Params::resolve(&camera, preset, &timing).unwrap()
}

/// Returns a mask of the low `bits` bits.
fn low_mask(bits: u8) -> u32 {
    (1u32 << bits) - 1
}

/// Keeps the flips that matter: the address bits, and the data bits the landing register holds.
fn effective(serial: &afe::Serial, flips: Flips) -> Flips {
    let mut kept = flips;
    for (sent, (address, data)) in kept.iter_mut().enumerate() {
        let landed = usize::from(sent as u8 ^ *address);
        *data &= serial
            .widths
            .get(landed)
            .map_or(0, |&width| low_mask(width));
    }
    kept
}

/// The flips of a frame whose only effective flip is `flip` on the write to `address`.
fn only(address: u8, flip: (u8, u32)) -> Flips {
    let mut flips = [(0, 0); REGISTERS];
    flips[usize::from(address)] = flip;
    flips
}

/// Returns the first frame from `from` on whose effective flips satisfy `wanted`.
fn find(preset: &Preset, from: u64, wanted: impl Fn(&Flips) -> bool) -> u64 {
    let params = afe_params(preset);
    (from..from + SEARCH_FRAMES)
        .find(|&index| {
            wanted(&effective(
                &params.serial,
                afe::serial_flips(&params, index),
            ))
        })
        .unwrap_or_else(|| panic!("no frame of {SEARCH_FRAMES} from {from} has the flips"))
}

/// Returns the first frame from `from` on whose only effective flips are `target`.
fn find_only(preset: &Preset, from: u64, target: Flips) -> u64 {
    find(preset, from, |flips| *flips == target)
}

/// The nominal codes with `code` at `address`.
fn nominal_with(preset: &Preset, address: u8, code: u32) -> [u32; REGISTERS] {
    let mut codes = afe_params(preset).serial.nominal;
    codes[usize::from(address)] = code;
    codes
}

/// The flipped VGA MSB: the register's nominal code with its top bit inverted.
fn vga_msb(preset: &Preset) -> (u32, u32) {
    let serial = afe_params(preset).serial;
    let msb = 1 << (serial.widths[usize::from(VGA_ADDRESS)] - 1);
    (msb, serial.nominal[usize::from(VGA_ADDRESS)] ^ msb)
}

/// The VGA gain of `code` relative to the camera's, in decibels.
fn vga_step_db(code: u32) -> f64 {
    let afe = camera().afe;
    afe.vga_min_db + afe.vga_db_per_code * f64::from(code) - afe.pga_gain_db
}

/// The active words of the last frame with their optical-black columns cut, line by line.
fn active_words(sim: &Simulator) -> Vec<u16> {
    let codes = sim.codes();
    let layout = codes.layout();
    let ob = layout.ob_cols() as usize;
    codes
        .words()
        .chunks_exact(layout.line_len() as usize)
        .flat_map(|line| line[ob..].iter().copied())
        .collect()
}

/// The gain of `bent` over `clean` in decibels, from the words above black that stay clear of the clip at `expected_db`.
///
/// Every word whose clean level, raised by `expected_db`, would pass the top code must
/// read at the top code. Returns the gain and the number of words it was measured on.
fn gain_db(clean: &[u16], bent: &[u16], expected_db: f64) -> (f64, usize) {
    let camera = camera();
    let black = f64::from(camera.adc.black_level);
    let top = f64::from(camera.max_code());
    let expected = 10f64.powf(expected_db / 20.0);
    let (mut clean_sum, mut bent_sum, mut measured) = (0.0, 0.0, 0);
    for (&clean, &bent) in clean.iter().zip(bent) {
        let raised = (f64::from(clean) - black) * expected;
        if raised < 0.9 * (top - black) {
            clean_sum += f64::from(clean) - black;
            bent_sum += f64::from(bent) - black;
            measured += 1;
        } else if raised > 1.1 * (top - black) {
            assert!(
                f64::from(bent) >= top - 1.0,
                "a word of {clean} raised by {expected_db:.2} dB reads {bent}, not the clip"
            );
        }
    }
    (20.0 * (bent_sum / clean_sum).log10(), measured)
}

#[test]
fn a_sound_link_renders_byte_identically_to_none() {
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        let mut none = build(with_serial(mode, None));
        let expected = render(&mut none, &scene);
        let mut sound = build(with_serial(mode, Some(0.0)));
        assert_eq!(
            render(&mut sound, &scene),
            expected,
            "{mode}: a sound link differs from no section"
        );
        assert_eq!(sound.afe_state(), none.afe_state(), "{mode}: the AFE state");
        assert_eq!(sound.afe_state().serial_codes(), None, "{mode}");
    }
}

#[test]
fn the_camera_values_are_the_nearest_codes_of_their_registers() {
    let camera = camera();
    let afe = &camera.afe;
    let serial = afe_params(&with_serial(Mode::Still, None)).serial;
    assert_eq!(
        serial.nominal[usize::from(CONTROL_ADDRESS)],
        0,
        "the outputs are on"
    );
    let code = |address: u8| f64::from(serial.nominal[usize::from(address)]);
    let vga_db = afe.vga_min_db + afe.vga_db_per_code * code(VGA_ADDRESS);
    assert!(
        (vga_db - afe.pga_gain_db).abs() <= afe.vga_db_per_code / 2.0,
        "VGA code {} gives {vga_db} dB for {} dB",
        code(VGA_ADDRESS),
        afe.pga_gain_db
    );
    let code_v = camera.adc.full_scale_v / f64::from(camera.max_code());
    let pedestal_v = code(CLAMP_ADDRESS) * code_v;
    assert!(
        (pedestal_v - afe.pedestal_v).abs() <= code_v / 2.0,
        "clamp code {} gives {pedestal_v} V for {} V",
        code(CLAMP_ADDRESS),
        afe.pedestal_v
    );
    let pxga_step_db = afe.pxga_max_db / f64::from(low_mask(afe.pxga_bits));
    for (address, gain_db) in PXGA_ADDRESSES.into_iter().zip(afe.pxga_gain_db) {
        assert!((code(address) * pxga_step_db - gain_db).abs() <= pxga_step_db / 2.0);
    }
}

#[test]
fn a_camera_value_beyond_its_register_is_refused() {
    let preset = with_serial(Mode::Still, None);
    let breaks: [(BreakCamera, &str); 3] = [
        (|c| c.afe.pga_gain_db = 45.0, "afe.pga_gain_db"),
        (
            |c| c.afe.pga_gain_db = c.afe.vga_min_db - 1.0,
            "afe.pga_gain_db",
        ),
        (|c| c.afe.clamp_bits = 6, "afe.pedestal_v"),
    ];
    for (breaks, field) in breaks {
        let mut camera = camera();
        breaks(&mut camera);
        let timing = Timing::derive(&camera, &preset, WIDTH, HEIGHT).unwrap();
        let error = afe::Params::resolve(&camera, &preset, &timing).unwrap_err();
        assert!(error.is_config(), "{error}");
        assert!(
            error.to_string().contains(field),
            "\"{error}\" names {field}"
        );
        let error = Simulator::builder((camera, preset.clone()))
            .input_size(WIDTH, HEIGHT)
            .build()
            .unwrap_err();
        assert!(error.is_config(), "the simulator: {error}");
    }
}

#[test]
fn at_certainty_every_bit_flips_and_every_write_is_lost() {
    let preset = with_serial(Mode::Still, Some(1.0));
    let params = afe_params(&preset);
    let data = low_mask(camera().afe.serial_data_bits);
    for index in 0..4 {
        assert_eq!(
            afe::serial_flips(&params, index),
            [(u8::MAX, data); REGISTERS],
            "frame {index}"
        );
    }
    // Every address inverts past the modelled ones, so the registers keep the camera's values.
    let scene = scene();
    let mut sim = build(preset);
    let frames = render(&mut sim, &scene);
    assert_eq!(sim.afe_state().serial_codes(), None);
    assert_eq!(
        frames,
        render(&mut build(with_serial(Mode::Still, None)), &scene),
        "the lost writes changed the camera"
    );

    // A corrupted register stays corrupted while every write is lost.
    let noisy = with_serial(Mode::Still, Some(SEARCH_BIT_ERROR));
    let (msb, corrupted) = vga_msb(&noisy);
    let index = find_only(&noisy, 0, only(VGA_ADDRESS, (0, msb)));
    let mut sim = build(noisy.clone());
    sim.frame(&scene, index);
    let held = Some(nominal_with(&noisy, VGA_ADDRESS, corrupted));
    assert_eq!(sim.afe_state().serial_codes(), held);
    assert_eq!(
        sim.set_preset(with_serial(Mode::Still, Some(1.0))).unwrap(),
        KEPT
    );
    for later in index + 1..index + 4 {
        sim.frame(&scene, later);
        assert_eq!(sim.afe_state().serial_codes(), held, "frame {later}");
    }
}

#[test]
fn a_flipped_vga_msb_brightens_a_grey_frame_by_18_db() {
    let scene = grey(MID_GREY);
    for mode in [Mode::Still, Mode::Movie] {
        let preset = with_serial(mode, Some(SEARCH_BIT_ERROR));
        let (msb, corrupted) = vga_msb(&preset);
        let index = find_only(&preset, 0, only(VGA_ADDRESS, (0, msb)));
        let mut bent = build(preset.clone());
        bent.frame(&scene, index);
        assert_eq!(
            bent.afe_state().serial_codes(),
            Some(nominal_with(&preset, VGA_ADDRESS, corrupted)),
            "{mode}: frame {index}"
        );
        let mut clean = build(with_serial(mode, None));
        clean.frame(&scene, index);
        let expected_db = vga_step_db(corrupted);
        assert!(
            (expected_db - 18.0).abs() < 0.5,
            "the VGA's MSB is worth {expected_db} dB"
        );
        let (measured_db, words) =
            gain_db(&active_words(&clean), &active_words(&bent), expected_db);
        assert!(words > 100, "{mode}: only {words} words below the clip");
        assert!(
            (measured_db - expected_db).abs() < 0.1,
            "{mode}: the frame is {measured_db:.3} dB brighter, the register {expected_db:.3} dB"
        );
    }
}

/// Whether stored sample `sample` of line `line` has the mosaic phase of `phase`, 0 to 3 for R, Gr, Gb, B.
///
/// The phase of the line's image row and of the sample's column counted from the
/// first active sample, the optical-black columns continuing the alternation.
fn in_phase(sim: &Simulator, line: u32, sample: usize, phase: usize) -> bool {
    let layout = sim.codes().layout();
    let row = layout.image_row(line) as usize & 1;
    let column = (sample + layout.ob_cols() as usize) & 1;
    2 * row + column == phase
}

#[test]
fn a_flipped_pxga_gr_bit_changes_only_the_gr_samples() {
    let scene = grey(MID_GREY);
    // A chain that settles fully within each pixel period, so a sample's word depends
    // on its own packet alone and every other word is untouched.
    let mut fast = camera();
    fast.afe.bandwidth_hz = 1e15;
    let mut bottom_up = fast.clone();
    bottom_up.still.direction = Direction::BottomUp;
    let cases = [
        (Mode::Still, fast.clone()),
        (Mode::Movie, fast),
        (Mode::Still, bottom_up),
    ];
    let gr = 1;
    let address = PXGA_ADDRESSES[gr];
    for (mode, camera) in cases {
        let direction = camera.clocking(mode).direction;
        let preset = with_serial(mode, Some(SEARCH_BIT_ERROR));
        let serial = afe_params(&preset).serial;
        let msb = 1 << (serial.widths[usize::from(address)] - 1);
        let index = find_only(&preset, 0, only(address, (0, msb)));
        let mut bent = build_with(camera.clone(), preset.clone());
        bent.frame(&scene, index);
        let mut clean = build_with(camera.clone(), with_serial(mode, None));
        clean.frame(&scene, index);
        let corrupted = serial.nominal[usize::from(address)] ^ msb;
        assert_eq!(
            bent.afe_state().serial_codes(),
            Some(nominal_with(&preset, address, corrupted))
        );
        let layout = clean.codes().layout().clone();
        let line_len = layout.line_len() as usize;
        let ob = layout.ob_cols() as usize;
        let (mut changed, mut phase_words) = (0, 0);
        let (mut clean_active, mut bent_active) = (Vec::new(), Vec::new());
        let lines = clean
            .codes()
            .words()
            .chunks_exact(line_len)
            .zip(bent.codes().words().chunks_exact(line_len));
        for (line, (clean_line, bent_line)) in (0..layout.rows()).zip(lines) {
            for (sample, (&clean_word, &bent_word)) in clean_line.iter().zip(bent_line).enumerate()
            {
                if in_phase(&clean, line, sample, gr) {
                    phase_words += 1;
                    changed += usize::from(clean_word != bent_word);
                    if sample >= ob {
                        clean_active.push(clean_word);
                        bent_active.push(bent_word);
                    }
                } else {
                    assert_eq!(
                        clean_word, bent_word,
                        "{mode} {direction:?}: line {line}, sample {sample} is not Gr but changed"
                    );
                }
            }
        }
        assert!(
            changed * 10 >= phase_words * 9,
            "{mode} {direction:?}: {changed} of {phase_words} Gr words changed"
        );
        let pxga_db = serial.pxga_max_db * f64::from(corrupted)
            / f64::from(low_mask(serial.widths[usize::from(address)]));
        let (measured_db, _) = gain_db(&clean_active, &bent_active, pxga_db);
        assert!(
            (measured_db - pxga_db).abs() < 0.1,
            "{mode} {direction:?}: the Gr sites are {measured_db:.3} dB brighter, the register {pxga_db:.3} dB"
        );
    }
}

#[test]
fn a_tristated_frame_holds_one_word() {
    let scene = scene();
    let preset = with_serial(Mode::Still, Some(SEARCH_BIT_ERROR));
    let sound = with_serial(Mode::Still, None);
    for bit in [POWER_DOWN, OUTPUT_DISABLE] {
        let index = find_only(&preset, 1, only(CONTROL_ADDRESS, (0, bit)));
        let after = find_only(&preset, index + 1, [(0, 0); REGISTERS]);
        let mut bent = build(sound.clone());
        let mut clean = build(sound.clone());
        // A clean first frame leaves its last word on the bus.
        bent.frame(&scene, 0);
        clean.frame(&scene, 0);
        let last = *bent.codes().words().last().unwrap();
        assert_eq!(bent.set_preset(preset.clone()).unwrap(), KEPT);
        bent.frame(&scene, index);
        clean.frame(&scene, index);
        assert_eq!(
            bent.afe_state().serial_codes(),
            Some(nominal_with(&preset, CONTROL_ADDRESS, bit))
        );
        let words = bent.codes().words();
        assert!(
            words.iter().all(|&word| word == words[0]),
            "control {bit:#b}: the tristated frame holds more than one word"
        );
        assert!(
            words[0].abs_diff(last) <= 1,
            "control {bit:#b}: the frame holds {} after a last word of {last}",
            words[0]
        );
        assert_ne!(words, clean.codes().words(), "the frame is the camera's");
        // The analog chain ran on behind the outputs: the next clean frame is the camera's.
        bent.frame(&scene, after);
        clean.frame(&scene, after);
        assert_eq!(bent.afe_state().serial_codes(), None);
        assert_eq!(
            bent.codes(),
            clean.codes(),
            "control {bit:#b}: frame {after}"
        );
    }
}

#[test]
fn a_misdirected_write_lands_on_the_register_at_its_address() {
    let preset = with_serial(Mode::Still, Some(SEARCH_BIT_ERROR));
    // The VGA write with its address's bit 0 flipped lands on the control register.
    let index = find_only(
        &preset,
        0,
        only(VGA_ADDRESS, (VGA_ADDRESS ^ CONTROL_ADDRESS, 0)),
    );
    let serial = afe_params(&preset).serial;
    let landed = serial.nominal[usize::from(VGA_ADDRESS)]
        & low_mask(serial.widths[usize::from(CONTROL_ADDRESS)]);
    let mut sim = build(preset.clone());
    let scene = scene();
    sim.frame(&scene, index);
    assert_eq!(
        sim.afe_state().serial_codes(),
        Some(nominal_with(&preset, CONTROL_ADDRESS, landed)),
        "the VGA keeps its value, the control register takes the VGA's data"
    );
    let words = sim.codes().words();
    assert_eq!(
        words.iter().all(|&word| word == words[0]),
        landed & (POWER_DOWN | OUTPUT_DISABLE) != 0,
        "control {landed:#b} and the outputs"
    );
}

#[test]
fn a_lost_write_keeps_the_last_frames_register() {
    let scene = grey(DARK_GREY);
    let preset = with_serial(Mode::Still, Some(SEARCH_BIT_ERROR));
    let (msb, corrupted) = vga_msb(&preset);
    let first = find_only(&preset, 0, only(VGA_ADDRESS, (0, msb)));
    // The VGA write goes to an address past the modelled registers; the others are clean.
    let second = find(&preset, first + 1, |flips| {
        let (address, data) = flips[usize::from(VGA_ADDRESS)];
        let lost = usize::from(VGA_ADDRESS ^ address) >= REGISTERS;
        lost && data == 0 && *flips == only(VGA_ADDRESS, (address, 0))
    });
    let clean_after = find_only(&preset, second + 1, [(0, 0); REGISTERS]);
    let mut bent = build(preset.clone());
    let mut clean = build(with_serial(Mode::Still, None));
    let held = Some(nominal_with(&preset, VGA_ADDRESS, corrupted));
    for index in [first, second] {
        bent.frame(&scene, index);
        clean.frame(&scene, index);
        assert_eq!(bent.afe_state().serial_codes(), held, "frame {index}");
        let expected_db = vga_step_db(corrupted);
        let (measured_db, words) =
            gain_db(&active_words(&clean), &active_words(&bent), expected_db);
        assert!(words > 100, "frame {index}: {words} words");
        assert!(
            (measured_db - expected_db).abs() < 0.1,
            "frame {index} is {measured_db:.3} dB brighter, the register {expected_db:.3} dB"
        );
    }
    // A clean write restores the camera's exact gain.
    bent.frame(&scene, clean_after);
    clean.frame(&scene, clean_after);
    assert_eq!(bent.afe_state().serial_codes(), None);
    assert_eq!(bent.codes(), clean.codes(), "frame {clean_after}");
}

#[test]
fn the_registers_persist_across_a_reconfiguration_and_reset_at_power_on() {
    let scene = scene();
    let preset = with_serial(Mode::Still, Some(SEARCH_BIT_ERROR));
    let (msb, corrupted) = vga_msb(&preset);
    let index = find_only(&preset, 0, only(VGA_ADDRESS, (0, msb)));
    let held = Some(nominal_with(&preset, VGA_ADDRESS, corrupted));
    let mut sim = build(preset.clone());
    sim.frame(&scene, index);
    assert_eq!(sim.afe_state().serial_codes(), held);
    // A turned error rate and a removed section keep the registers until the next writes.
    for turned in [Some(0.02), Some(0.0), None] {
        let state = sim.afe_state().clone();
        assert_eq!(
            sim.set_preset(with_serial(Mode::Still, turned)).unwrap(),
            KEPT
        );
        assert_eq!(
            sim.afe_state(),
            &state,
            "{turned:?}: the AFE lost its state"
        );
        assert_eq!(sim.afe_state().serial_codes(), held, "{turned:?}");
    }
    // Without the section the processor's writes are clean.
    sim.frame(&scene, index + 1);
    assert_eq!(sim.afe_state().serial_codes(), None);
    // Power-on and a rebuilt camera hold the camera's values.
    assert_eq!(sim.set_preset(preset.clone()).unwrap(), KEPT);
    sim.frame(&scene, index);
    assert_eq!(sim.afe_state().serial_codes(), held);
    sim.reset();
    assert_eq!(sim.afe_state().serial_codes(), None, "after reset");
    sim.frame(&scene, index);
    assert_eq!(sim.afe_state().serial_codes(), held);
    let movie = with_serial(Mode::Movie, Some(SEARCH_BIT_ERROR));
    assert_eq!(sim.set_preset(movie).unwrap(), Reconfigured::Rebuilt);
    assert_eq!(sim.afe_state().serial_codes(), None, "after a rebuild");
}

#[test]
fn the_bend_replays_from_reset() {
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        let preset = with_serial(mode, Some(0.05));
        let mut sim = build(preset.clone());
        let first = render(&mut sim, &scene);
        assert_ne!(
            first,
            render(&mut build(with_serial(mode, None)), &scene),
            "{mode}: the bend left the frames alone"
        );
        sim.reset();
        assert_eq!(
            render(&mut sim, &scene),
            first,
            "{mode}: the replay from reset"
        );
        assert_eq!(
            render(&mut build(preset), &scene),
            first,
            "{mode}: a fresh camera"
        );
    }
}

#[test]
fn the_flips_differ_between_seeds_and_frames_at_the_set_rate() {
    let mut preset = with_serial(Mode::Still, Some(0.05));
    let params = afe_params(&preset);
    let frames: Vec<Flips> = (0..32)
        .map(|index| afe::serial_flips(&params, index))
        .collect();
    for (i, a) in frames.iter().enumerate() {
        for (j, b) in frames.iter().enumerate().skip(i + 1) {
            assert_ne!(a, b, "frames {i} and {j} flip the same bits");
        }
    }
    let seeds: Vec<Flips> = (0..8)
        .map(|seed| {
            preset.seed = seed;
            afe::serial_flips(&afe_params(&preset), 0)
        })
        .collect();
    for (i, a) in seeds.iter().enumerate() {
        for (j, b) in seeds.iter().enumerate().skip(i + 1) {
            assert_ne!(a, b, "seeds {i} and {j} flip the same bits in frame 0");
        }
    }
    // Each bit flips on its own with the set probability.
    let params = afe_params(&with_serial(Mode::Still, Some(SEARCH_BIT_ERROR)));
    let frames = 4000;
    let bits_per_frame = REGISTERS as f64 * f64::from(8 + params.serial.data_bits);
    let flipped: u32 = (0..frames)
        .flat_map(|index| afe::serial_flips(&params, index))
        .map(|(address, data)| address.count_ones() + data.count_ones())
        .sum();
    let rate = f64::from(flipped) / (frames as f64 * bits_per_frame);
    assert!(
        (rate / SEARCH_BIT_ERROR - 1.0).abs() < 0.05,
        "{rate} of the bits flipped at a bit error of {SEARCH_BIT_ERROR}"
    );
}
