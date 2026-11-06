//! The master-clock reclock: the processor's capture of a bus driven by a detuned timing
//! generator, and the two clocks of the timing.

#![cfg(all(feature = "serde", feature = "test-util"))]

use vcb_core::adc::Codes;
use vcb_core::preset::{Mode, ReclockBend, Resolution};
use vcb_core::readout::Layout;
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing, afe, capture};

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
        capture::reframe(Some(&self.params), state, index, &mut self.codes);
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
