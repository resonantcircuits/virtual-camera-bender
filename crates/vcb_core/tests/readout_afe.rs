//! Physical behaviour of the readout, the analog front end and the converter.

#![cfg(feature = "serde")]

use std::f64::consts::TAU;

use vcb_core::adc::{self, Codes};
use vcb_core::afe::{self, Analog, GainMod, Inject, Wave};
use vcb_core::preset::{AfeBend, ClockBend, Resolution};
use vcb_core::readout::{self, Layout, Stream};
use vcb_core::sensor::Charge;
use vcb_core::{Camera, Preset, Timing};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn square(freq_hz: f64, amplitude_v: f64) -> Preset {
    let mut preset = Preset::new("a520");
    preset.bend.afe = Some(AfeBend {
        inject: Some(Inject {
            wave: Wave::Square,
            freq_hz,
            amplitude_v,
            phase_skew: 0.0,
        }),
        ..AfeBend::default()
    });
    preset
}

/// The resolved readout, analog and converter stages of one camera, preset and size.
struct Chain {
    timing: Timing,
    readout: readout::Params,
    afe: afe::Params,
    adc: adc::Params,
}

impl Chain {
    fn new(camera: &Camera, preset: &Preset, width: u32, height: u32) -> Self {
        let timing = Timing::derive(camera, preset, width, height).unwrap();
        Self {
            readout: readout::Params::resolve(camera, preset, &timing).unwrap(),
            afe: afe::Params::resolve(camera, preset, &timing).unwrap(),
            adc: adc::Params::resolve(camera, preset, &timing).unwrap(),
            timing,
        }
    }

    /// Turns the read noise off.
    fn quiet(mut self) -> Self {
        self.afe.read_noise_e = 0.0;
        self.afe.noise_1f_corner_hz = 0.0;
        self
    }

    fn layout(&self) -> &Layout {
        &self.readout.layout
    }

    fn charge(&self) -> Charge {
        Charge::new(self.timing.sensor_width, self.timing.sensor_height)
    }

    fn read(&self, state: &mut readout::State, charge: &Charge, index: u64) -> Stream {
        let mut stream = Stream::new(self.layout().clone());
        readout::read(&self.readout, state, charge, index, &mut stream);
        stream
    }

    /// A stream whose every stored sample holds `electrons`.
    fn flat(&self, electrons: f32) -> Stream {
        let mut stream = Stream::new(self.layout().clone());
        stream.samples_mut().fill(electrons);
        stream
    }

    fn analog(&self, state: &mut afe::State, stream: &Stream, index: u64) -> Analog {
        let mut analog = Analog::new(self.layout().clone());
        afe::process(&self.afe, state, stream, index, &mut analog);
        analog
    }

    fn codes(&self, analog: &Analog) -> Codes {
        let mut codes = Codes::new(self.layout().clone());
        adc::convert(&self.adc, analog, &mut codes);
        codes
    }

    /// Volts per electron from the AFE input to the ADC input.
    fn gain(&self) -> f64 {
        self.afe.volts_per_electron()
    }
}

/// Splits stored samples into the active part of each line.
fn active_lines<'a, T>(samples: &'a [T], layout: &Layout) -> impl Iterator<Item = &'a [T]> {
    let ob = layout.ob_cols() as usize;
    samples
        .chunks_exact(layout.line_len() as usize)
        .map(move |line| &line[ob..])
}

fn mean(values: impl IntoIterator<Item = f64>) -> f64 {
    let (sum, count) = values
        .into_iter()
        .fold((0.0, 0.0), |(sum, count), v| (sum + v, count + 1.0));
    sum / count
}

/// Returns, for every line, whether its mean active voltage lies above the frame's mean.
fn line_signs(analog: &Analog) -> Vec<bool> {
    let means: Vec<f64> = active_lines(analog.volts(), analog.layout())
        .map(|line| mean(line.iter().map(|&v| f64::from(v))))
        .collect();
    let overall = mean(means.iter().copied());
    means.iter().map(|&m| m > overall).collect()
}

fn sign_changes(signs: &[bool]) -> usize {
    signs.windows(2).filter(|pair| pair[0] != pair[1]).count()
}

/// Time of the middle of each line's active samples, from the frame start, in seconds.
fn line_centres_s(layout: &Layout) -> Vec<f64> {
    let middle = f64::from(layout.ob_cols()) + 0.5 * f64::from(layout.cols());
    (0..layout.rows())
        .map(|line| f64::from(line) * layout.line_period_s() + middle * layout.pixel_period_s())
        .collect()
}

/// Estimates the frame-start phase of a square wave from the sign of each line.
///
/// Scans the phase for the values that agree with the most lines and returns their
/// circular mean, in cycles.
fn square_phase(signs: &[bool], centres_s: &[f64], freq_hz: f64) -> f64 {
    const STEPS: u32 = 4000;
    let scores: Vec<usize> = (0..STEPS)
        .map(|step| {
            let phase = f64::from(step) / f64::from(STEPS);
            signs
                .iter()
                .zip(centres_s)
                .filter(|&(&high, &t)| ((phase + freq_hz * t).fract() < 0.5) == high)
                .count()
        })
        .collect();
    let best = *scores.iter().max().unwrap();
    let (x, y) = (0..STEPS)
        .filter(|&step| scores[step as usize] == best)
        .map(|step| TAU * f64::from(step) / f64::from(STEPS))
        .fold((0.0, 0.0), |(x, y), angle| {
            (x + angle.cos(), y + angle.sin())
        });
    (y.atan2(x) / TAU).rem_euclid(1.0)
}

/// Returns the distance between two phases on the unit circle of cycles.
fn phase_gap(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(1.0);
    d.min(1.0 - d)
}

#[test]
fn square_injection_crosses_zero_twice_per_cycle() {
    let freq_hz = 1000.0;
    let chain = Chain::new(&camera(), &square(freq_hz, 0.05), 640, 480);
    let analog = chain.analog(&mut afe::State::default(), &chain.flat(2000.0), 0);
    let layout = chain.layout();
    // The lines read span (rows - 1) line periods; vertical blanking carries no samples.
    let span_s = f64::from(layout.rows() - 1) * layout.line_period_s();
    let expected = 2.0 * freq_hz * span_s;
    let changes = sign_changes(&line_signs(&analog)) as f64;
    assert!(
        (changes - expected).abs() <= 1.0,
        "{changes} sign changes, expected {expected:.2}"
    );
}

#[test]
fn injection_phase_runs_on_across_frames() {
    let freq_hz = 1000.0;
    let chain = Chain::new(&camera(), &square(freq_hz, 0.05), 640, 480);
    let layout = chain.layout();
    let stream = chain.flat(2000.0);
    let centres = line_centres_s(layout);
    let mut state = afe::State::default();

    let first = chain.analog(&mut state, &stream, 0);
    let advance = (freq_hz * layout.frame_period_s()).fract();
    assert!(
        (state.phase() - advance).abs() < 1e-9,
        "phase after one frame is {}, expected {advance}",
        state.phase()
    );
    let second = chain.analog(&mut state, &stream, 1);

    let phase0 = square_phase(&line_signs(&first), &centres, freq_hz);
    let phase1 = square_phase(&line_signs(&second), &centres, freq_hz);
    assert!(
        phase_gap(phase0, 0.0) < 0.01,
        "frame 0 starts at phase {phase0}"
    );
    assert!(
        phase_gap(phase1 - phase0, advance) < 0.01,
        "bands moved by {} cycles between frames, expected {advance}",
        (phase1 - phase0).rem_euclid(1.0)
    );
}

#[test]
fn band_count_does_not_depend_on_resolution() {
    let freq_hz = 1000.0;
    let rates: Vec<(usize, f64)> = [(640, 480), (1280, 960)]
        .into_iter()
        .map(|(width, height)| {
            let chain = Chain::new(&camera(), &square(freq_hz, 0.05), width, height);
            let analog = chain.analog(&mut afe::State::default(), &chain.flat(2000.0), 0);
            let layout = chain.layout();
            let changes = sign_changes(&line_signs(&analog));
            let span_s = f64::from(layout.rows() - 1) * layout.line_period_s();
            (changes, changes as f64 * layout.frame_period_s() / span_s)
        })
        .collect();
    let (small, large) = (rates[0], rates[1]);
    assert!(
        small.0.abs_diff(large.0) <= 1,
        "{} bands at 640x480, {} at 1280x960",
        small.0,
        large.0
    );
    assert!(
        (small.1 / large.1 - 1.0).abs() < 0.01,
        "bands per frame {small:?} vs {large:?}"
    );
}

/// Electrons per line for an edge test: dark up to `edge`, bright from it on.
fn step_stream(chain: &Chain, edge: usize, electrons: f32) -> Stream {
    let layout = chain.layout().clone();
    let (ob, line_len) = (layout.ob_cols() as usize, layout.line_len() as usize);
    let mut stream = Stream::new(layout);
    for line in stream.samples_mut().chunks_exact_mut(line_len) {
        line[ob + edge..].fill(electrons);
    }
    stream
}

/// Returns the step response of line 10, normalised to 0 before and 1 long after the edge.
fn step_response(chain: &Chain, analog: &Analog, edge: usize, electrons: f32) -> Vec<f64> {
    let line = active_lines(analog.volts(), chain.layout())
        .nth(10)
        .unwrap();
    let low = chain.afe.pedestal_v;
    let high = low + f64::from(electrons) * chain.gain();
    line[edge..]
        .iter()
        .map(|&v| (f64::from(v) - low) / (high - low))
        .collect()
}

#[test]
fn bandwidth_sets_the_rise_time_in_pixels() {
    let (edge, electrons) = (200, 10_000.0);
    let mut slow = Chain::new(&camera(), &Preset::new("a520"), 640, 480).quiet();
    // A time constant of ten native pixel periods: ten pixels at every resolution.
    slow.afe.bandwidth_hz = 1.0 / (TAU * 10.0 * slow.afe.native_pixel_period_s);
    let analog = slow.analog(
        &mut afe::State::default(),
        &step_stream(&slow, edge, electrons),
        0,
    );
    let rise = step_response(&slow, &analog, edge, electrons);
    let samples_to_63 = rise.iter().position(|&r| r >= 0.63).unwrap() + 1;
    assert!(
        (9..=11).contains(&samples_to_63),
        "63 % after {samples_to_63} samples: {:?}",
        &rise[..12]
    );

    let mut native_preset = Preset::new("a520");
    native_preset.readout.resolution = Resolution::Native;
    let native = Chain::new(&camera(), &native_preset, 1, 1).quiet();
    assert!((native.timing.pixel_period_s / native.afe.native_pixel_period_s - 1.0).abs() < 1e-12);
    let analog = native.analog(
        &mut afe::State::default(),
        &step_stream(&native, edge, electrons),
        0,
    );
    let rise = step_response(&native, &analog, edge, electrons);
    assert!(
        rise[0] > 0.999,
        "first sample after the edge reaches {}",
        rise[0]
    );
}

#[test]
fn slew_limit_turns_a_step_into_a_ramp() {
    let (edge, electrons) = (200, 10_000.0);
    let mut chain = Chain::new(&camera(), &Preset::new("a520"), 640, 480).quiet();
    chain.afe.slew_v_per_us = 2.0;
    let analog = chain.analog(
        &mut afe::State::default(),
        &step_stream(&chain, edge, electrons),
        0,
    );
    let line = active_lines(analog.volts(), chain.layout())
        .nth(10)
        .unwrap();
    // Per pixel the node moves the native slew times the native pixel period.
    let per_pixel = chain.afe.slew_v_per_us * 1e6 * chain.afe.native_pixel_period_s;
    let steps: Vec<f64> = line[edge - 1..edge + 5]
        .windows(2)
        .map(|pair| f64::from(pair[1]) - f64::from(pair[0]))
        .collect();
    assert!(
        steps.iter().all(|&step| (step - per_pixel).abs() < 1e-5),
        "steps {steps:?}, expected {per_pixel} V"
    );
}

#[test]
fn late_reset_sample_echoes_edges() {
    let (edge, electrons) = (100, 5000.0);
    let mut chain = Chain::new(&camera(), &Preset::new("a520"), 640, 480).quiet();
    chain.afe.cds_lag_px = 3.0;
    chain.afe.cds_mix = 0.5;
    let analog = chain.analog(
        &mut afe::State::default(),
        &step_stream(&chain, edge, electrons),
        0,
    );
    let response = step_response(&chain, &analog, edge, electrons);
    for (offset, &r) in response[..12].iter().enumerate() {
        let expected = if offset < 3 { 1.0 } else { 0.5 };
        assert!(
            (r - expected).abs() < 1e-3,
            "sample {offset} after the edge is {r}, expected {expected}"
        );
    }
}

#[test]
fn gain_modulation_swings_the_signal_by_its_depth() {
    let mut preset = Preset::new("a520");
    preset.bend.afe = Some(AfeBend {
        gain_mod: Some(GainMod {
            wave: Wave::Square,
            freq_hz: 1000.0,
            depth: 0.5,
        }),
        ..AfeBend::default()
    });
    let chain = Chain::new(&camera(), &preset, 640, 480).quiet();
    let electrons = 4000.0;
    let analog = chain.analog(&mut afe::State::default(), &chain.flat(electrons), 0);
    let nominal = f64::from(electrons) * chain.gain();
    let relative: Vec<f64> = active_lines(analog.volts(), chain.layout())
        .map(|line| (mean(line.iter().map(|&v| f64::from(v))) - chain.afe.pedestal_v) / nominal)
        .collect();
    let high = relative.iter().copied().fold(f64::MIN, f64::max);
    let low = relative.iter().copied().fold(f64::MAX, f64::min);
    assert!((high - 1.5).abs() < 0.01, "highest line gain {high}");
    assert!((low - 0.5).abs() < 0.01, "lowest line gain {low}");
}

#[test]
fn sample_and_hold_injection_holds_random_levels_within_its_amplitude() {
    let mut preset = Preset::new("a520");
    let amplitude_v = 0.05;
    preset.bend.afe = Some(AfeBend {
        inject: Some(Inject {
            wave: Wave::Noise,
            freq_hz: 200.0,
            amplitude_v,
            phase_skew: 0.0,
        }),
        ..AfeBend::default()
    });
    let chain = Chain::new(&camera(), &preset, 640, 480).quiet();
    let analog = chain.analog(&mut afe::State::default(), &chain.flat(1000.0), 0);
    let base = chain.afe.pedestal_v + 1000.0 * chain.gain();
    let reach = amplitude_v * chain.afe.pga_gain();
    let levels: Vec<f64> = active_lines(analog.volts(), chain.layout())
        .map(|line| f64::from(line[line.len() / 2]) - base)
        .collect();
    assert!(levels.iter().all(|level| level.abs() <= reach * 1.0001));
    // A 200 Hz cycle spans about ten lines at 640 x 480: levels hold, then jump.
    let holds = levels
        .windows(2)
        .filter(|pair| (pair[0] - pair[1]).abs() < 1e-6)
        .count();
    let jumps = levels.len() - 1 - holds;
    assert!(holds > 5 * jumps, "{holds} held lines, {jumps} jumps");
    assert!(jumps >= 30, "only {jumps} new levels in a frame");
}

#[test]
fn lowered_cte_trails_grow_with_distance_from_the_register() {
    let camera = camera();
    let column = 100;
    // Returns the trail length, the column profile, and the charge read, left in the
    // register and put in, in electrons.
    let trail = |cte: f64, row: u32| -> (usize, Vec<f32>, [f64; 3]) {
        let mut chain = Chain::new(&camera, &Preset::new("a520"), 640, 480);
        chain.readout.cte = cte;
        let mut charge = chain.charge();
        charge.plane_mut().row_mut(row).fill(10_000.0);
        let stream = chain.read(&mut readout::State::default(), &charge, 0);
        let profile: Vec<f32> = active_lines(stream.samples(), chain.layout())
            .map(|line| line[column])
            .collect();
        let peak = profile.iter().copied().fold(0.0, f32::max);
        let length = profile[row as usize..]
            .iter()
            .filter(|&&v| v > 0.1 * peak)
            .count();
        let read: f64 = active_lines(stream.samples(), chain.layout())
            .flatten()
            .map(|&v| f64::from(v))
            .sum();
        let left: f64 = stream.residual().iter().map(|&v| f64::from(v)).sum();
        let total: f64 = charge.plane().data().iter().map(|&v| f64::from(v)).sum();
        (length, profile, [read, left, total])
    };

    let lengths: Vec<usize> = [40, 240, 440]
        .into_iter()
        .map(|row| {
            let (length, _, [read, left, total]) = trail(0.995, row);
            assert!(
                ((read + left) / total - 1.0).abs() < 1e-4,
                "row {row}: {read} e read and {left} e left of {total} e"
            );
            assert!(
                read <= total * (1.0 + 1e-6),
                "row {row}: the readout created charge"
            );
            length
        })
        .collect();
    assert!(
        lengths.windows(2).all(|pair| pair[0] < pair[1]),
        "trail lengths {lengths:?} do not grow with distance"
    );
    assert!(lengths[0] > 1, "a near row leaves no trail: {lengths:?}");

    for row in [40, 440] {
        let (length, profile, [read, left, total]) = trail(1.0, row);
        assert_eq!(
            length, 1,
            "lossless transfer leaves a trail behind row {row}"
        );
        let lit = |line: usize, value: f32| (line == row as usize) == (value > 0.0);
        assert!(profile.iter().enumerate().all(|(line, &v)| lit(line, v)));
        assert!((read / total - 1.0).abs() < 1e-6 && left.abs() < 1e-6);
    }
}

/// A charge plane whose rows hold 100 plus the row number, so each line names its row.
fn numbered_rows(chain: &Chain) -> Charge {
    let mut charge = chain.charge();
    for (row, values) in charge.plane_mut().rows_mut().enumerate() {
        values.fill(100.0 + row as f32);
    }
    charge
}

/// Returns the sensor row each line holds, or `None` for an empty line.
fn rows_read(stream: &Stream) -> Vec<Option<u32>> {
    active_lines(stream.samples(), stream.layout())
        .map(|line| (line[0] >= 100.0).then(|| (line[0] - 100.0).round() as u32))
        .collect()
}

#[test]
fn vertical_clock_faults_repeat_and_skip_rows() {
    let mut chain = Chain::new(&camera(), &Preset::new("a520"), 640, 480);
    chain.readout.cte = 1.0;
    let charge = numbered_rows(&chain);

    let clean = rows_read(&chain.read(&mut readout::State::default(), &charge, 0));
    let expected: Vec<Option<u32>> = chain.layout().row_map().iter().map(|&r| Some(r)).collect();
    assert_eq!(clean, expected);

    chain.readout.v_pulse_drop_hz = 200.0;
    let faulty = rows_read(&chain.read(&mut readout::State::default(), &charge, 0));
    let pairs: Vec<(u32, u32)> = faulty
        .windows(2)
        .filter_map(|pair| Some((pair[0]?, pair[1]?)))
        .collect();
    let repeats = pairs.iter().filter(|(a, b)| a == b).count();
    let skips = pairs.iter().filter(|(a, b)| *b > *a + 1).count();
    assert!(repeats > 0, "no repeated rows: {faulty:?}");
    assert!(skips > 0, "no skipped rows: {faulty:?}");
    assert!(
        pairs.iter().all(|(a, b)| b >= a),
        "rows went backwards: {faulty:?}"
    );
    let swapped = faulty
        .iter()
        .enumerate()
        .filter(|(line, row)| row.is_some_and(|row| row % 2 != *line as u32 % 2))
        .count();
    assert!(swapped > 0, "no line has the other Bayer row phase");
}

#[test]
fn horizontal_glitches_shift_bands_with_wraparound() {
    let mut chain = Chain::new(&camera(), &Preset::new("a520"), 640, 480);
    chain.readout.cte = 1.0;
    let mut charge = chain.charge();
    for values in charge.plane_mut().rows_mut() {
        for (x, value) in values.iter_mut().enumerate() {
            *value = 1.0 + x as f32;
        }
    }
    let clean = chain.read(&mut readout::State::default(), &charge, 0);
    for (line, row) in active_lines(clean.samples(), chain.layout()).zip(charge.plane().rows()) {
        assert_eq!(line, &row[..line.len()]);
    }

    chain.readout.h_glitch_hz = 50.0;
    let glitched = chain.read(&mut readout::State::default(), &charge, 0);
    let mut shifts = Vec::new();
    for (line, reference) in active_lines(glitched.samples(), chain.layout())
        .zip(active_lines(clean.samples(), chain.layout()))
    {
        // Every sample of a clean line is distinct, so the first one locates the shift.
        let first = reference[0].to_bits();
        let shift = line.iter().position(|v| v.to_bits() == first).unwrap();
        let mut rotated = reference.to_vec();
        rotated.rotate_right(shift);
        assert_eq!(
            line,
            rotated.as_slice(),
            "a line is not a rotation of its clean line"
        );
        shifts.push(shift);
    }
    let shifted = shifts.iter().filter(|&&s| s != 0).count();
    assert!(shifted > 0, "no line was shifted");
    let bands = shifts
        .windows(2)
        .filter(|pair| pair[0] == 0 && pair[1] != 0)
        .count();
    assert!(
        bands > 0 && shifted > bands,
        "{shifted} shifted lines in {bands} bands"
    );
}

/// Mean, standard deviation and lag-one autocorrelation along lines of active codes.
fn code_statistics(codes: &Codes) -> (f64, f64, f64) {
    let layout = codes.layout();
    let m = mean(
        active_lines(codes.words(), layout)
            .flatten()
            .map(|&c| f64::from(c)),
    );
    let (mut power, mut lagged) = (0.0, 0.0);
    for line in active_lines(codes.words(), layout) {
        for pair in line.windows(2) {
            let (a, b) = (f64::from(pair[0]) - m, f64::from(pair[1]) - m);
            power += a * a;
            lagged += a * b;
        }
    }
    let count = f64::from(layout.rows() * (layout.cols() - 1));
    (m, (power / count).sqrt(), lagged / power)
}

#[test]
fn read_noise_has_the_camera_level_and_streaks_along_lines() {
    let camera = camera();
    let mut white = Chain::new(&camera, &Preset::new("a520"), 1280, 960);
    white.afe.noise_1f_corner_hz = 0.0;
    let stream = white.flat(0.0);
    let codes = white.codes(&white.analog(&mut afe::State::default(), &stream, 0));
    let (_, sigma, white_lag) = code_statistics(&codes);
    let lsb_v = f64::from(white.adc.full_scale_v) / f64::from(white.adc.max_code());
    let expected = white.afe.read_noise_e * white.gain() / lsb_v;
    assert!(
        (sigma / expected - 1.0).abs() < 0.15,
        "code noise {sigma:.3}, expected {expected:.3}"
    );

    let mut pink = Chain::new(&camera, &Preset::new("a520"), 1280, 960);
    pink.afe.noise_1f_corner_hz = 20_000.0;
    let codes = pink.codes(&pink.analog(&mut afe::State::default(), &stream, 0));
    let (_, _, pink_lag) = code_statistics(&codes);
    assert!(pink_lag > 0.0, "lag-one autocorrelation {pink_lag}");
    assert!(
        pink_lag > white_lag + 0.002,
        "flicker raises the lag-one autocorrelation from {white_lag} only to {pink_lag}"
    );
}

/// A preset that patches in every readout and analog bend.
fn every_bend() -> Preset {
    let mut preset = Preset::new("a520");
    preset.seed = 9;
    preset.bend.clocks = Some(ClockBend {
        cte: Some(0.999),
        v_pulse_drop_hz: 40.0,
        v_stall_lines: 6.0,
        h_glitch_hz: 30.0,
        h_shift_frac: 0.3,
    });
    preset.bend.afe = Some(AfeBend {
        inject: Some(Inject {
            wave: Wave::Sine,
            freq_hz: 770.0,
            amplitude_v: 0.03,
            phase_skew: 0.013,
        }),
        gain_mod: Some(GainMod {
            wave: Wave::Noise,
            freq_hz: 45.0,
            depth: 0.2,
        }),
        cds_lag_px: 2.5,
        cds_mix: 0.3,
    });
    preset
}

#[test]
fn every_bend_replays_identically_from_power_on() {
    let chain = Chain::new(&camera(), &every_bend(), 160, 120);
    let mut charge = chain.charge();
    for (row, values) in charge.plane_mut().rows_mut().enumerate() {
        for (x, value) in values.iter_mut().enumerate() {
            *value = (40 * x + 25 * row) as f32 % 9000.0;
        }
    }
    let run = || -> Vec<Vec<u16>> {
        let (mut readout_state, mut afe_state) = (readout::State::default(), afe::State::default());
        (0..4)
            .map(|index| {
                let stream = chain.read(&mut readout_state, &charge, index);
                let analog = chain.analog(&mut afe_state, &stream, index);
                chain.codes(&analog).words().to_vec()
            })
            .collect()
    };
    let first = run();
    assert_eq!(run(), first);
    assert!(first.windows(2).all(|pair| pair[0] != pair[1]));
}

#[test]
fn consecutive_frames_differ_in_noise_but_not_in_signal() {
    let chain = Chain::new(&camera(), &Preset::new("a520"), 640, 480);
    let mut charge = chain.charge();
    for values in charge.plane_mut().rows_mut() {
        for (x, value) in values.iter_mut().enumerate() {
            *value = 20.0 * x as f32;
        }
    }
    let (mut readout_state, mut afe_state) = (readout::State::default(), afe::State::default());
    let frames: Vec<Codes> = (0..2)
        .map(|index| {
            let stream = chain.read(&mut readout_state, &charge, index);
            chain.codes(&chain.analog(&mut afe_state, &stream, index))
        })
        .collect();
    let layout = chain.layout();
    let differences: Vec<f64> = active_lines(frames[1].words(), layout)
        .flatten()
        .zip(active_lines(frames[0].words(), layout).flatten())
        .map(|(&b, &a)| f64::from(b) - f64::from(a))
        .collect();
    let changed = differences.iter().filter(|&&d| d.abs() > 0.5).count();
    assert!(
        changed * 2 > differences.len(),
        "only {changed} codes changed"
    );
    let drift = mean(differences.iter().copied());
    assert!(drift.abs() < 0.05, "the signal moved by {drift} codes");
    let noise = mean(differences.iter().map(|d| d * d)).sqrt();
    // Column means over all lines carry the signal; they agree to within the noise.
    let cols = layout.cols() as usize;
    let column_means = |codes: &Codes| -> Vec<f64> {
        let mut sums = vec![0.0; cols];
        for line in active_lines(codes.words(), layout) {
            for (sum, &code) in sums.iter_mut().zip(line) {
                *sum += f64::from(code);
            }
        }
        sums.iter()
            .map(|sum| sum / f64::from(layout.rows()))
            .collect()
    };
    let (before, after) = (column_means(&frames[0]), column_means(&frames[1]));
    let worst = before
        .iter()
        .zip(&after)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    let bound = 5.0 * noise / f64::from(layout.rows()).sqrt();
    assert!(
        worst < bound,
        "a column mean moved by {worst} codes, noise allows {bound}"
    );
}
