//! The master-clock reclock: the processor's capture of a bus driven by a detuned timing
//! generator, and the two clocks of the timing.

#![cfg(all(feature = "serde", feature = "test-util"))]

use std::f64::consts::TAU;

use vcb_core::adc::Codes;
use vcb_core::afe::{Analog, Inject, Wave};
use vcb_core::power::Rail;
use vcb_core::preset::{AfeBend, Mode, RailBend, ReclockBend, Resolution};
use vcb_core::readout::{Direction, Layout, Stream};
use vcb_core::{
    Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing, afe, capture, sensor,
};

/// The word the tests' capture puts on the bus during blanking, unlike any tagged word.
const BLANK: u16 = u16::MAX;

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn preset(mode: Mode, resolution: Resolution, bend: Option<ReclockBend>) -> Preset {
    let mut preset = Preset::new("a520");
    preset.readout.mode = mode;
    preset.readout.resolution = resolution;
    preset.bend.clock = bend;
    preset
}

/// The camera's nominal pixel clock of `mode` in megahertz.
fn nominal_mhz(mode: Mode) -> f64 {
    camera().clocking(mode).pixel_clock_hz * 1e-6
}

/// A steady oscillator at `ratio` of the nominal clock, with a capture window.
fn reclock(mode: Mode, ratio: f64, hd_lock_px: f64) -> ReclockBend {
    ReclockBend {
        hd_lock_px,
        ..ReclockBend::new(nominal_mhz(mode) * ratio)
    }
}

/// The word of line `n`, period `m`: the line modulo 16 above the period.
fn tag(n: usize, m: usize) -> u16 {
    (((n % 16) << 12) | m) as u16
}

/// Returns the line modulo 16 and the period of a tagged word; `None` for blanking.
fn untag(word: u16) -> Option<(usize, usize)> {
    (word != BLANK).then_some((usize::from(word >> 12), usize::from(word & 0x0fff)))
}

/// The capture of one readout with its parameters, and codes tagged with their position.
struct Rig {
    timing: Timing,
    params: capture::Params,
    codes: Codes,
    /// The supply rail of every readout line, nominal unless a test sets it.
    rail: Rail,
}

impl Rig {
    fn new(mode: Mode, resolution: Resolution, size: (u32, u32), bend: ReclockBend) -> Self {
        let camera = camera();
        let preset = preset(mode, resolution, Some(bend));
        let timing = Timing::derive(&camera, &preset, size.0, size.1).unwrap();
        let mut params = capture::Params::resolve(&camera, &preset, &timing)
            .unwrap()
            .unwrap();
        params.blank_code = BLANK;
        let layout = Layout::from_timing(&timing, camera.clocking(mode).direction);
        let mut rig = Self {
            timing,
            params,
            rail: Rail::new(&layout, camera.power.rail_v),
            codes: Codes::new(layout),
        };
        rig.retag();
        rig
    }

    /// The camera's native readout of `mode`.
    fn native(mode: Mode, bend: ReclockBend) -> Self {
        Self::new(mode, Resolution::Native, (2272, 1704), bend)
    }

    /// Stored samples per line.
    fn stored(&self) -> usize {
        self.codes.layout().line_len() as usize
    }

    /// Periods per line, blanking included.
    fn line_len(&self) -> usize {
        self.timing.line_len() as usize
    }

    /// Writes each sample's own tag into the codes, as the timing generator drives them.
    fn retag(&mut self) {
        let stored = self.stored();
        for (n, line) in self.codes.words_mut().chunks_exact_mut(stored).enumerate() {
            for (m, word) in line.iter_mut().enumerate() {
                *word = tag(n, m);
            }
        }
    }

    /// Captures a freshly tagged frame from `state` and returns the words, line by line.
    fn capture(&mut self, state: &mut capture::State, index: u64) -> Vec<Vec<u16>> {
        self.retag();
        capture::reframe(
            Some(&self.params),
            state,
            &self.rail,
            index,
            &mut self.codes,
        );
        self.codes
            .words()
            .chunks_exact(self.stored())
            .map(<[u16]>::to_vec)
            .collect()
    }

    /// Captures one frame from power-on.
    fn capture_once(&mut self) -> Vec<Vec<u16>> {
        self.capture(&mut capture::State::default(), 0)
    }
}

#[test]
fn a_steady_oscillator_at_the_nominal_clock_captures_every_word_in_place() {
    for mode in [Mode::Still, Mode::Movie] {
        for (resolution, size) in [
            (Resolution::Native, (2272, 1704)),
            (Resolution::MatchInput, (568, 426)),
            (Resolution::MatchInput, (3000, 2000)),
        ] {
            let bend = ReclockBend {
                ppm_per_v: 2000.0,
                ..reclock(mode, 1.0, 40.0)
            };
            let mut rig = Rig::new(mode, resolution, size, bend);
            let tagged = rig.codes.words().to_vec();
            rig.capture_once();
            assert!(
                rig.codes.words() == tagged.as_slice(),
                "{mode} {resolution} {size:?}: a nominal oscillator moved a word"
            );
        }
    }
}

#[test]
fn a_clock_detuned_by_a_part_in_a_trillion_still_captures_every_word_in_place() {
    // Off the exact nominal, the capture runs its full model; the TG then slips by
    // less than a thousandth of a period over the frame, at every resolution.
    for mode in [Mode::Still, Mode::Movie] {
        for (resolution, size) in [
            (Resolution::Native, (2272, 1704)),
            (Resolution::MatchInput, (568, 426)),
            (Resolution::MatchInput, (640, 480)),
        ] {
            let mut rig = Rig::new(mode, resolution, size, reclock(mode, 1.0 + 1e-12, 0.0));
            let tagged = rig.codes.words().to_vec();
            rig.capture_once();
            assert!(
                rig.codes.words() == tagged.as_slice(),
                "{mode} {resolution} {size:?}: the model moved a word"
            );
        }
    }
}

#[test]
fn twice_the_clock_puts_two_timing_generator_lines_side_by_side_at_half_width() {
    let mut rig = Rig::native(Mode::Movie, reclock(Mode::Movie, 2.0, 0.0));
    let rows = rig.timing.rows_read as usize;
    let stored = rig.stored();
    let lines = rig.capture_once();
    for (k, line) in lines.iter().enumerate() {
        let words: Vec<_> = line.iter().map(|&word| untag(word)).collect();
        if 2 * k >= rows {
            assert!(
                words.iter().all(Option::is_none),
                "line {k}: the TG is done"
            );
            continue;
        }
        // Line k shows TG line 2k, then its blanking, then TG line 2k + 1.
        let first: Vec<_> = words
            .iter()
            .flatten()
            .filter(|w| w.0 == 2 * k % 16)
            .collect();
        let second: Vec<_> = (2 * k + 1 < rows)
            .then(|| words.iter().flatten().filter(|w| w.0 == (2 * k + 1) % 16))
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(
            first.len() + second.len(),
            words.iter().flatten().count(),
            "line {k} holds other lines"
        );
        let last_first = words
            .iter()
            .rposition(|w| w.is_some_and(|w| w.0 == 2 * k % 16));
        let first_second = words
            .iter()
            .position(|w| w.is_some_and(|w| w.0 == (2 * k + 1) % 16));
        if let (Some(a), Some(b)) = (last_first, first_second) {
            assert!(a < b, "line {k}: TG line {} comes first", 2 * k);
            assert!(
                words[a + 1..b].iter().all(Option::is_none),
                "line {k}: blanking between"
            );
        }
        for half in [&first, &second] {
            for pair in half.windows(2) {
                assert_eq!(pair[1].1, pair[0].1 + 2, "line {k}: two periods per latch");
            }
        }
        // The whole of TG line 2k fits in the first half of the processor's line.
        assert!(
            first[0].1 <= 1 && first.last().unwrap().1 >= stored - 2,
            "line {k}"
        );
        if let Some(start) = second.first() {
            assert!(
                start.1 <= 1,
                "line {k}: TG line {} from its start",
                2 * k + 1
            );
        }
    }
}

#[test]
fn half_the_clock_with_a_wide_window_stretches_each_line_start_and_loses_the_bottom() {
    let line = Rig::native(Mode::Movie, reclock(Mode::Movie, 1.0, 0.0)).line_len();
    let window = 2.0 * line as f64;
    let mut rig = Rig::native(Mode::Movie, reclock(Mode::Movie, 0.5, window));
    let frame_lines = rig.timing.lines_per_frame() as usize;
    let lines = rig.capture_once();
    // The processor waits for each TG line, two of its own long: VD comes halfway.
    let whole = (frame_lines - 1) / 2;
    let lost = frame_lines.div_ceil(2);
    for (k, words) in lines.iter().enumerate() {
        if k < whole {
            for (j, &word) in words.iter().enumerate() {
                assert_eq!(untag(word), Some((k % 16, j / 2)), "line {k}, sample {j}");
            }
        } else if k >= lost {
            assert!(words.iter().all(|&w| w == BLANK), "line {k} after VD");
        }
    }
    assert!(lost < lines.len(), "the bottom of the frame is lost");
    assert!(lines.len() - lost > lines.len() / 3, "about half the frame");
}

/// Returns the timing generator's position, in periods from VD, of each word of `lines`.
///
/// Line `k` may hold TG lines `k - 1` to `k + 1` only, as for a clock within a few
/// periods per line of the nominal.
fn positions(lines: &[Vec<u16>], line_len: usize) -> Vec<Vec<Option<usize>>> {
    lines
        .iter()
        .enumerate()
        .map(|(k, words)| {
            words
                .iter()
                .map(|&word| {
                    let (tag, m) = untag(word)?;
                    let n = (k.saturating_sub(1)..=k + 1).find(|n| n % 16 == tag);
                    let n = n.unwrap_or_else(|| panic!("line {k} holds a TG line {tag} mod 16"));
                    Some(n * line_len + m)
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_clock_one_period_per_line_fast_skews_each_line_by_one_sample_more() {
    for mode in [Mode::Still, Mode::Movie] {
        let line = Rig::native(mode, reclock(mode, 1.0, 0.0)).line_len();
        let ratio = 1.0 + 1.0 / line as f64;
        let mut rig = Rig::native(mode, reclock(mode, ratio, 0.0));
        let at = positions(&rig.capture_once(), line);
        let mut checked = 0usize;
        for k in 1..at.len() {
            for (j, (before, now)) in at[k - 1].iter().zip(&at[k]).enumerate() {
                if let (Some(before), Some(now)) = (before, now) {
                    assert_eq!(now - before, line + 1, "{mode}: line {k}, sample {j}");
                    checked += 1;
                }
            }
        }
        // The shift wraps the line: blanking enters from the right as lines go down.
        assert!(
            checked > at.len() * rig.stored() / 2,
            "{mode}: {checked} pairs"
        );
        assert_eq!(at[0][0], Some(0), "{mode}: VD starts both clocks together");
    }
}

/// Returns how many lines start with another TG position in their line than line 0.
fn skewed_lines(at: &[Vec<Option<usize>>], line_len: usize) -> usize {
    let start = at[0][0].map(|p| p % line_len);
    at.iter()
        .filter(|words| words[0].map(|p| p % line_len) != start)
        .count()
}

#[test]
fn a_capture_window_wider_than_the_slip_holds_every_line_to_its_start() {
    let mode = Mode::Movie;
    let line = Rig::native(mode, reclock(mode, 1.0, 0.0)).line_len();
    let slip = 3.0;
    for ratio in [1.0 + slip / line as f64, 1.0 - slip / line as f64] {
        let mut locked = Rig::native(mode, reclock(mode, ratio, slip + 2.0));
        let lines = locked.capture_once();
        let at = positions(&lines, line);
        for (k, words) in lines.iter().enumerate() {
            for (j, (&word, &first)) in words.iter().zip(&lines[0]).enumerate() {
                let expected = untag(first).map(|(_, m)| (k % 16, m));
                assert_eq!(untag(word), expected, "ratio {ratio}: line {k}, sample {j}");
            }
        }
        assert_eq!(skewed_lines(&at, line), 0, "ratio {ratio}");

        let mut loose = Rig::native(mode, reclock(mode, ratio, slip - 1.0));
        let at = positions(&loose.capture_once(), line);
        assert!(
            skewed_lines(&at, line) > at.len() * 9 / 10,
            "ratio {ratio}: a narrow window lets the lines skew"
        );
    }
}

#[test]
fn a_window_narrower_than_the_slip_catches_the_phase_only_as_it_wraps_past() {
    // Forty periods of slip a line against a twelve-period window: the phase runs free
    // for a hundred lines, is caught where it wraps into the window, and runs on.
    let mode = Mode::Still;
    let line = Rig::native(mode, reclock(mode, 1.0, 0.0)).line_len();
    let ratio = 1.0 - 40.0 / line as f64;
    let mut free = Rig::native(mode, reclock(mode, ratio, 0.0));
    let mut caught = Rig::native(mode, reclock(mode, ratio, 12.0));
    let (free, caught) = (free.capture_once(), caught.capture_once());
    let first_difference = free.iter().zip(&caught).position(|(a, b)| a != b);
    let first_difference = first_difference.expect("the window catches the phase somewhere");
    assert!(
        first_difference > line / 40 / 2,
        "free running until the phase first wraps, line {first_difference}"
    );
}

#[test]
fn drift_moves_the_clock_from_frame_to_frame_and_runs_on_across_frames() {
    let mode = Mode::Movie;
    let drifting = ReclockBend {
        drift_ppm: 20_000.0,
        drift_hz: 0.25,
        ..ReclockBend::new(nominal_mhz(mode))
    };
    let mut rig = Rig::native(mode, drifting);
    let mut running = capture::State::default();
    let first = rig.capture(&mut running, 0);
    let second = rig.capture(&mut running, 1);
    assert_ne!(
        first, second,
        "the same words captured a frame later differ"
    );
    let after_reset = rig.capture(&mut capture::State::default(), 1);
    assert_eq!(
        after_reset, first,
        "the capture remembers nothing but the drift"
    );
    assert_ne!(after_reset, second, "the drift runs on across frames");

    // The phase advances in real time: the drift's frequency times the frame period.
    let mut state = capture::State::default();
    rig.capture(&mut state, 0);
    let expected = (0.25 * rig.timing.frame_period_s).rem_euclid(1.0);
    assert!((state.drift_cycles()[0] - expected).abs() < 1e-12);

    // A steady detuned oscillator captures the same words every frame.
    let mut steady = Rig::native(mode, reclock(mode, 0.995, 6.0));
    let mut state = capture::State::default();
    let first = steady.capture(&mut state, 0);
    assert_eq!(steady.capture(&mut state, 1), first);
}

#[test]
fn match_input_shows_the_native_look_downscaled() {
    // A slow clock running free: diagonal tears into blanking and a frame that rolls.
    // The share of each band of the frame lost to blanking matches the native readout.
    let mode = Mode::Still;
    let bend = reclock(mode, 0.989, 0.0);
    let bands = 12;
    let lost_share = |rig: &mut Rig| -> Vec<f64> {
        let lines = rig.capture_once();
        let edge = |band: usize| band * lines.len() / bands;
        (0..bands)
            .map(|band| {
                let band = &lines[edge(band)..edge(band + 1)];
                let all: usize = band.iter().map(Vec::len).sum();
                let lost = band.iter().flatten().filter(|&&w| w == BLANK).count();
                lost as f64 / all as f64
            })
            .collect()
    };
    let native = lost_share(&mut Rig::native(mode, bend));
    for size in [(568, 426), (640, 480)] {
        let small = lost_share(&mut Rig::new(mode, Resolution::MatchInput, size, bend));
        for (band, (n, s)) in native.iter().zip(&small).enumerate() {
            assert!(
                (n - s).abs() < 0.01,
                "{size:?} band {band}: {s:.3} lost against {n:.3} at native"
            );
        }
    }
    // The tears are there to match: about a third of the frame is blanking, unevenly.
    assert!(native.iter().all(|&share| share > 0.2), "{native:?}");
    let spread =
        native.iter().copied().fold(0.0, f64::max) - native.iter().copied().fold(1.0, f64::min);
    assert!(spread > 0.05, "{native:?}");
}

/// A scene with edges in both directions and saturated colours.
fn scene(width: u32, height: u32) -> Scene {
    let rgb: Vec<u8> = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let v = ((x * 255 / width + y * 96 / height) % 256) as u8;
                match (x * 3 / width, y * 2 / height) {
                    (0, 0) => [60, 140, 40],
                    (1, 0) => [90, 150, 230],
                    (2, 0) => [220, 70, 40],
                    _ => [v, v / 2 + 60, 255 - v],
                }
            })
        })
        .collect();
    Scene::from_srgb8(width, height, &rgb)
}

const WIDTH: u32 = 96;
const HEIGHT: u32 = 72;

fn simulator(preset: Preset) -> Simulator {
    Simulator::builder((camera(), preset))
        .input_size(WIDTH, HEIGHT)
        .build()
        .unwrap()
}

fn render(sim: &mut Simulator, scene: &Scene, frames: std::ops::Range<u64>) -> Vec<Frame> {
    frames
        .map(|index| sim.frame(scene, index).clone())
        .collect()
}

/// A wandering, loosely locked oscillator.
fn wandering(osc_mhz: f64) -> ReclockBend {
    ReclockBend {
        drift_ppm: 4000.0,
        drift_hz: 0.7,
        hd_lock_px: 30.0,
        ..ReclockBend::new(osc_mhz)
    }
}

#[test]
fn a_nominal_oscillator_renders_byte_identically_to_no_bend() {
    let scene = scene(WIDTH, HEIGHT);
    for mode in [Mode::Still, Mode::Movie] {
        let plain = preset(mode, Resolution::MatchInput, None);
        let expected = render(&mut simulator(plain), &scene, 0..3);
        let bend = ReclockBend {
            hd_lock_px: 12.0,
            ppm_per_v: -900.0,
            ..ReclockBend::new(nominal_mhz(mode))
        };
        let bent = preset(mode, Resolution::MatchInput, Some(bend));
        assert_eq!(
            render(&mut simulator(bent), &scene, 0..3),
            expected,
            "{mode}"
        );
    }
}

#[test]
fn a_reclocked_camera_replays_identically_from_reset() {
    let scene = scene(WIDTH, HEIGHT);
    for mode in [Mode::Still, Mode::Movie] {
        let bent = preset(mode, Resolution::MatchInput, Some(wandering(26.7)));
        let mut sim = simulator(bent.clone());
        let first = render(&mut sim, &scene, 0..4);
        sim.reset();
        assert_eq!(render(&mut sim, &scene, 0..4), first, "{mode}: after reset");
        assert_eq!(
            render(&mut simulator(bent), &scene, 0..4),
            first,
            "{mode}: from a new simulator"
        );
    }
}

#[test]
fn a_new_oscillator_keeps_the_running_camera_and_continues_the_drift() {
    let scene = scene(WIDTH, HEIGHT);
    let mode = Mode::Movie;
    let before = preset(mode, Resolution::MatchInput, Some(wandering(26.7)));
    let after = preset(mode, Resolution::MatchInput, Some(wandering(25.9)));
    let mut sim = simulator(before);
    render(&mut sim, &scene, 0..2);
    let state = sim.capture_state().clone();
    let processor = sim.timing().dsp_pixel_period_s;
    let sensor = sim.timing().pixel_period_s;

    assert_eq!(sim.set_preset(after.clone()).unwrap(), KEPT);
    assert_eq!(sim.capture_state(), &state, "the drift's phase is kept");
    assert_eq!(
        sim.timing().dsp_pixel_period_s.to_bits(),
        processor.to_bits()
    );
    assert!(
        sim.timing().pixel_period_s > sensor,
        "a slower sensor clock"
    );
    sim.frame(&scene, 2);
    assert_eq!(
        sim.codes().layout().pixel_period_s().to_bits(),
        sim.timing().pixel_period_s.to_bits()
    );

    // The phase ran on through the change: as after three frames from power-on, and
    // unlike a camera that starts afresh with the new oscillator.
    let mut through = simulator(after.clone());
    render(&mut through, &scene, 0..3);
    assert_eq!(sim.capture_state(), through.capture_state());
    let mut fresh = simulator(after);
    fresh.frame(&scene, 2);
    assert_ne!(sim.capture_state(), fresh.capture_state());
}

#[test]
fn a_timing_generator_too_slow_for_the_frame_leaves_the_pedestal_on_the_bus() {
    let scene = scene(WIDTH, HEIGHT);
    let camera = camera();
    let mode = Mode::Movie;
    let line = Rig::native(mode, reclock(mode, 1.0, 0.0)).line_len();
    let slow = reclock(mode, 0.5, 2.0 * line as f64);
    let mut sim = simulator(preset(mode, Resolution::MatchInput, Some(slow)));
    sim.frame(&scene, 0);
    let codes = sim.codes();
    let stored = codes.layout().line_len() as usize;
    let lines: Vec<_> = codes.words().chunks_exact(stored).collect();
    let black = camera.adc.black_level;
    assert!(lines[0].iter().any(|&w| w != black), "the top is captured");
    assert!(
        lines.last().unwrap().iter().all(|&w| w == black),
        "the bottom holds the black code"
    );
}

#[test]
fn the_sensor_runs_on_the_oscillator_and_the_processor_on_its_crystal() {
    let camera = camera();
    let mode = Mode::Still;
    let plain = preset(mode, Resolution::MatchInput, None);
    let slow = preset(mode, Resolution::MatchInput, Some(reclock(mode, 0.9, 0.0)));
    let t1 = Timing::derive(&camera, &plain, 640, 480).unwrap();
    let t09 = Timing::derive(&camera, &slow, 640, 480).unwrap();
    let close = |a: f64, b: f64| (a / b - 1.0).abs() < 1e-12;

    assert!(close(t09.clock_ratio, 0.9));
    assert_eq!(
        t09.dsp_pixel_period_s.to_bits(),
        t1.pixel_period_s.to_bits()
    );
    assert_eq!(t09.frame_period_s.to_bits(), t1.frame_period_s.to_bits());
    assert!(close(t09.pixel_period_s, t1.pixel_period_s / 0.9));
    assert!(close(t09.pixel_clock_hz, 0.9 * t1.pixel_clock_hz));
    assert!(
        close(t09.line_period_s, t1.line_period_s / 0.9),
        "longer lines"
    );
    assert!(close(t09.resolution_scale(), t1.resolution_scale()));
    assert!(close(
        t1.resolution_scale(),
        t1.pixel_period_s / t1.native_pixel_period_s
    ));
    // The sensor reads past VD: its lines no longer fit the processor's frame.
    let sensor_frame = t09.line_period_s * f64::from(t09.lines_per_frame());
    assert!(sensor_frame > t09.frame_period_s * 1.1);

    // The AFE's pole is fixed in seconds at this resolution, so it spans more of a
    // slower sensor period: about 1 / 0.9 the bandwidth in pixels.
    let in_pixels = |preset: &Preset, timing: &Timing| {
        let afe = afe::Params::resolve(&camera, preset, timing).unwrap();
        afe.bandwidth_hz / timing.resolution_scale() * timing.pixel_period_s
    };
    assert!(close(in_pixels(&slow, &t09), in_pixels(&plain, &t1) / 0.9));

    let text = t09.to_string();
    assert!(
        text.contains("processor") && text.contains("0.900000"),
        "{text}"
    );
    assert!(!t1.to_string().contains("processor"));

    let mut sim = Simulator::builder((camera.clone(), plain))
        .input_size(640, 480)
        .build()
        .unwrap();
    assert_eq!(sim.set_preset(slow).unwrap(), KEPT);
    assert_eq!(sim.timing(), &t09);
}

#[test]
fn an_invalid_oscillator_is_rejected_by_the_timing() {
    let camera = camera();
    for osc_mhz in [0.0, -1.0, f64::NAN] {
        let bent = preset(
            Mode::Still,
            Resolution::Native,
            Some(ReclockBend::new(osc_mhz)),
        );
        let error = Timing::derive(&camera, &bent, 64, 48).unwrap_err();
        assert!(error.is_config(), "{osc_mhz}: {error}");
    }
}

#[test]
fn a_sagging_rail_slows_a_supply_sensitive_oscillator_from_the_line_it_reaches() {
    // The rail drops half a volt from the middle of the frame on; at 2e6 / L ppm per
    // volt the oscillator then runs one period per line slow.
    let mode = Mode::Still;
    let line = Rig::native(mode, reclock(mode, 1.0, 0.0)).line_len();
    let bend = ReclockBend {
        ppm_per_v: 2e6 / line as f64,
        ..reclock(mode, 1.0, 0.0)
    };
    let mut rig = Rig::native(mode, bend);
    let rows = rig.rail.lines();
    let half = rows / 2;
    let sagged_v = rig.rail.nominal_v() - 0.5;
    for n in half..rows {
        rig.rail.set(n, sagged_v, sagged_v);
    }
    let lines = rig.capture_once();
    for (k, words) in lines.iter().enumerate().take(half as usize - 1) {
        for (j, &word) in words.iter().enumerate() {
            assert_eq!(untag(word), Some((k % 16, j)), "line {k}, sample {j} moved");
        }
    }
    let at = positions(&lines, line);
    let mut checked = 0usize;
    for k in half as usize + 2..at.len() {
        for (j, (before, now)) in at[k - 1].iter().zip(&at[k]).enumerate() {
            if let (Some(before), Some(now)) = (before, now) {
                assert_eq!(now + 1, before + line, "line {k}, sample {j}");
                checked += 1;
            }
        }
    }
    assert!(checked > at.len() * rig.stored() / 4, "{checked} pairs");
}

/// The still preset at match-input with the rail and the oscillator bent.
fn railed(rail: Option<RailBend>, clock: ReclockBend) -> Preset {
    let mut preset = preset(Mode::Still, Resolution::MatchInput, Some(clock));
    preset.bend.rail = rail;
    preset
}

#[test]
fn the_rail_detunes_the_oscillator_only_where_it_moves() {
    let scene = scene(WIDTH, HEIGHT);
    let camera = camera();
    // 400 mA more through the 0.5 ohm source: the rail settles 0.2 V low.
    let sag = RailBend {
        load_ma: 0.2 / camera.power.source_ohm * 1e3,
        ..RailBend::default()
    };
    let at = |ppm_per_v: f64| ReclockBend {
        ppm_per_v,
        ..ReclockBend::new(nominal_mhz(Mode::Still))
    };
    let mut sagging = simulator(railed(Some(sag), at(1000.0)));
    let sensitive = render(&mut sagging, &scene, 0..2);
    let rail = sagging.rail();
    let middle_v = rail.v_at(rail.lines() / 2, 0.5);
    assert!(
        (middle_v - (camera.power.rail_v - 0.2)).abs() < 1e-3,
        "the rail stands at {middle_v} V"
    );
    let immune = render(&mut simulator(railed(Some(sag), at(0.0))), &scene, 0..2);
    assert_ne!(sensitive, immune, "200 ppm slow on the sagging rail");
    assert_eq!(
        render(&mut simulator(railed(None, at(1000.0))), &scene, 0..2),
        render(&mut simulator(railed(None, at(0.0))), &scene, 0..2),
        "a nominal rail leaves a supply-sensitive oscillator on its frequency"
    );
}

#[test]
fn a_rail_fed_oscillator_replays_from_reset_and_keeps_state_when_its_sensitivity_turns() {
    let scene = scene(WIDTH, HEIGHT);
    let spiking = RailBend {
        load_ma: 150.0,
        spike_ma: 300.0,
        spike_hz: 6.0,
        spike_ms: 20.0,
        ripple_mv: 30.0,
        ripple_hz: 120.0,
        source_ohm: Some(1.0),
    };
    let clock = |ppm_per_v: f64| ReclockBend {
        ppm_per_v,
        ..wandering(26.7)
    };
    let bent = railed(Some(spiking), clock(3000.0));
    let mut sim = simulator(bent.clone());
    let first = render(&mut sim, &scene, 0..4);
    sim.reset();
    assert_eq!(render(&mut sim, &scene, 0..4), first, "after reset");
    assert_eq!(
        render(&mut simulator(bent.clone()), &scene, 0..4),
        first,
        "from a new simulator"
    );

    // Turning the sensitivity is a knob: the rail's and the drift's states carry on.
    let mut running = simulator(bent);
    render(&mut running, &scene, 0..2);
    let (rail, drift) = (
        running.power_state().clone(),
        running.capture_state().clone(),
    );
    let turned = railed(Some(spiking), clock(-3000.0));
    assert_eq!(running.set_preset(turned.clone()).unwrap(), KEPT);
    assert_eq!(running.power_state(), &rail);
    assert_eq!(running.capture_state(), &drift);
    let mut through = simulator(turned);
    render(&mut through, &scene, 0..2);
    assert_eq!(
        through.power_state(),
        &rail,
        "the rail does not depend on the clock"
    );
}

/// Electrons of the bright side of the edges in the AFE tests.
const EDGE_E: f32 = 5000.0;

/// Sample of each line at which the AFE tests' edge rises, after optical black.
const EDGE_AT: usize = 20;

/// The resolved quiet AFE of a 64 by 48 match-input still, reclocked to `osc_mhz` if given.
fn quiet_afe(osc_mhz: Option<f64>) -> (Timing, Layout, afe::Params) {
    let camera = camera();
    let preset = preset(
        Mode::Still,
        Resolution::MatchInput,
        osc_mhz.map(ReclockBend::new),
    );
    let timing = Timing::derive(&camera, &preset, 64, 48).unwrap();
    let layout = Layout::from_timing(&timing, Direction::TopDown);
    let mut params = afe::Params::resolve(&camera, &preset, &timing).unwrap();
    params.read_noise_e = 0.0;
    params.noise_1f_corner_hz = 0.0;
    (timing, layout, params)
}

/// The AFE's response to an edge on every line, from black to [`EDGE_E`], normalised.
///
/// Returns the first eight samples from the edge on, 0 before the edge and 1 settled.
fn edge_response(osc_mhz: Option<f64>, set: impl Fn(&mut afe::Params)) -> Vec<f64> {
    let (_, layout, mut params) = quiet_afe(osc_mhz);
    set(&mut params);
    let len = layout.line_len() as usize;
    let edge = layout.ob_cols() as usize + EDGE_AT;
    let mut stream = Stream::new(layout.clone());
    for line in stream.samples_mut().chunks_exact_mut(len) {
        line[edge..].fill(EDGE_E);
    }
    let mut analog = Analog::new(layout.clone());
    let rail = Rail::new(&layout, camera().power.rail_v);
    afe::process(
        &params,
        &mut afe::State::default(),
        &stream,
        &rail,
        0,
        &mut analog,
    );
    let low = params.pedestal_v;
    let high = low + f64::from(EDGE_E) * params.volts_per_electron();
    analog.volts()[10 * len + edge..10 * len + edge + 8]
        .iter()
        .map(|&v| (f64::from(v) - low) / (high - low))
        .collect()
}

#[test]
fn the_afe_keeps_its_bandwidth_and_slew_in_seconds_under_a_new_oscillator() {
    let nominal = nominal_mhz(Mode::Still);
    // A time constant of one processor pixel period, at every resolution.
    let pole = |p: &mut afe::Params| p.bandwidth_hz = 1.0 / (TAU * p.native_pixel_period_s);
    let plain = edge_response(None, pole);
    assert_eq!(
        edge_response(Some(nominal), pole),
        plain,
        "the camera's own clock settles as without the bend"
    );
    let half = edge_response(Some(nominal / 2.0), pole);
    // One sample spans one time constant on the nominal clock and two at half of it.
    assert!(
        (plain[0] - (1.0 - (-1.0f64).exp())).abs() < 1e-4,
        "{plain:?}"
    );
    assert!((half[0] - (1.0 - (-2.0f64).exp())).abs() < 1e-4, "{half:?}");
    assert!(half[0] > plain[0] && half[1] > plain[1], "a sharper edge");

    // The bandwidth in sensor pixels doubles with half the clock.
    let in_pixels = |osc_mhz: Option<f64>| {
        let (timing, _, params) = quiet_afe(osc_mhz);
        params.bandwidth_hz * timing.pixel_period_s / params.resolution_scale
    };
    let ratio = in_pixels(Some(nominal / 2.0)) / in_pixels(None);
    assert!((ratio - 2.0).abs() < 1e-9, "{ratio}");

    // A slew-limited edge ramps twice as far per sample at half the clock.
    let slow = |p: &mut afe::Params| p.slew_v_per_us = 2.0;
    let (plain, half) = (
        edge_response(None, slow),
        edge_response(Some(nominal / 2.0), slow),
    );
    assert!(plain[0] < 0.5, "the slew limits the edge: {plain:?}");
    assert!(
        (half[0] / plain[0] - 2.0).abs() < 1e-3,
        "{half:?} against {plain:?}"
    );
}

#[test]
fn the_afe_oscillators_advance_from_vd_to_vd_at_every_clock() {
    let freq_hz = 1234.567;
    let nominal = nominal_mhz(Mode::Still);
    for osc_mhz in [None, Some(nominal / 2.0), Some(nominal * 1.5)] {
        let (timing, layout, mut params) = quiet_afe(osc_mhz);
        params.inject = Some(Inject {
            wave: Wave::Sine,
            freq_hz,
            amplitude_v: 0.01,
            phase_skew: 0.0,
        });
        let mut state = afe::State::default();
        let mut analog = Analog::new(layout.clone());
        let rail = Rail::new(&layout, camera().power.rail_v);
        afe::process(
            &params,
            &mut state,
            &Stream::new(layout),
            &rail,
            0,
            &mut analog,
        );
        let expected = (freq_hz * timing.frame_period_s).rem_euclid(1.0);
        let gap = (state.phase() - expected).rem_euclid(1.0);
        assert!(
            gap.min(1.0 - gap) < 1e-9,
            "{osc_mhz:?} MHz: phase {} after a frame, expected {expected}",
            state.phase()
        );
    }
    // Through the simulator, the injection runs on with the processor's frame.
    let mut bent = preset(
        Mode::Movie,
        Resolution::MatchInput,
        Some(ReclockBend::new(nominal / 2.0)),
    );
    bent.bend.afe = Some(AfeBend {
        inject: Some(Inject {
            wave: Wave::Square,
            freq_hz,
            amplitude_v: 0.01,
            phase_skew: 0.0,
        }),
        ..AfeBend::default()
    });
    let mut sim = simulator(bent);
    let scene = scene(WIDTH, HEIGHT);
    render(&mut sim, &scene, 0..3);
    let expected = (3.0 * freq_hz * sim.timing().frame_period_s).rem_euclid(1.0);
    let gap = (sim.afe_state().phase() - expected).rem_euclid(1.0);
    assert!(gap.min(1.0 - gap) < 1e-9, "after three frames");
}

#[test]
fn the_smear_gathers_light_only_until_vd() {
    let camera = camera();
    let mode = Mode::Movie;
    let smear = |ratio: Option<f64>| {
        let preset = preset(
            mode,
            Resolution::MatchInput,
            ratio.map(|ratio| reclock(mode, ratio, 0.0)),
        );
        let timing = Timing::derive(&camera, &preset, WIDTH, HEIGHT).unwrap();
        let params = sensor::Params::resolve(&camera, &preset, &timing).unwrap();
        (timing, params)
    };
    let (timing, plain) = smear(None);
    let read_s = f64::from(timing.rows_read) * timing.line_period_s;
    assert!(read_s < timing.frame_period_s);
    assert!((f64::from(plain.readout_s) / read_s - 1.0).abs() < 1e-6);
    // A fast oscillator transfers the frame sooner and smears less.
    let (_, fast) = smear(Some(1.25));
    let less = fast.smear_per_electron() / plain.smear_per_electron();
    assert!((less - 0.8).abs() < 1e-5, "{less}");
    // A slow one is cut off by VD: its transfer lasts one frame period, not 1 / ratio.
    let (_, slow) = smear(Some(0.5));
    let more = slow.smear_per_electron() / plain.smear_per_electron();
    let expected = (timing.frame_period_s / read_s) as f32;
    assert!((more / expected - 1.0).abs() < 1e-5, "{more}");
    assert!(more < 2.0);
}
