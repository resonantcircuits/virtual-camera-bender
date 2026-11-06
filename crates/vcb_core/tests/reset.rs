//! The reset bend: missed and weak pulses on the CCD's reset gate, read through the CDS.

#![cfg(all(feature = "serde", feature = "test-util"))]

use std::ops::Range;

use vcb_core::afe::{self, Analog};
use vcb_core::power::Rail;
use vcb_core::preset::{AfeBend, Mode, ResetBend, Resolution};
use vcb_core::readout::{self, Layout, Stream};
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator, Timing};

/// Sensor size of the analog-chain tests, at match-input resolution.
const WIDTH: u32 = 160;
const HEIGHT: u32 = 120;

/// Scene size of the whole-camera tests.
const SCENE_WIDTH: u32 = 64;
const SCENE_HEIGHT: u32 = 48;

/// Frames rendered from reset in each whole-camera comparison.
const FRAMES: u64 = 3;

/// Electrons of a dark packet, far below the output stage's swing.
const DARK_E: f32 = 200.0;

/// The largest difference between two readings of one packet that counts as the same, in electrons.
///
/// The voltages are stored as `f32`, a few thousandths of an electron at full scale.
const SAME_E: f64 = 0.05;

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

fn camera() -> Camera {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A preset at match-input resolution with `[bend.reset]` set to `bend`.
fn with_reset(bend: Option<ResetBend>) -> Preset {
    let mut preset = Preset::new("a520");
    preset.readout.resolution = Resolution::MatchInput;
    preset.bend.reset = bend;
    preset
}

fn drops(drop_hz: f64, hold_px: f64) -> ResetBend {
    ResetBend {
        drop_hz,
        hold_px,
        ..ResetBend::default()
    }
}

fn weak(reset_frac: f64) -> ResetBend {
    ResetBend {
        reset_frac,
        ..ResetBend::default()
    }
}

/// The analog front end of a preset on a sensor of the test size, without noise or band limits.
///
/// The bandwidth and slew rate are raised far past the pixel rate, so each sample's
/// voltage is the CDS reading of its packet alone.
#[derive(Clone)]
struct Chain {
    layout: Layout,
    afe: afe::Params,
    rail: Rail,
    /// The output stage's swing in electrons.
    swing_e: f64,
}

impl Chain {
    fn new(preset: &Preset) -> Self {
        let camera = camera();
        let timing = Timing::derive(&camera, preset, WIDTH, HEIGHT).unwrap();
        let readout = readout::Params::resolve(&camera, preset, &timing).unwrap();
        let mut afe = afe::Params::resolve(&camera, preset, &timing).unwrap();
        afe.read_noise_e = 0.0;
        afe.noise_1f_corner_hz = 0.0;
        afe.bandwidth_hz = 1e15;
        afe.slew_v_per_us = 1e12;
        Self {
            rail: Rail::new(&readout.layout, camera.power.rail_v),
            swing_e: afe.fd_swing_v / (afe.conv_gain_uv_per_e * 1e-6),
            layout: readout.layout,
            afe,
        }
    }

    /// The same chain with a sound reset gate.
    fn sound(&self) -> Self {
        let mut chain = self.clone();
        chain.afe.reset = None;
        chain
    }

    /// Pixel periods of one line, blanking included.
    fn line_px(&self) -> u64 {
        u64::from(self.layout.line_len() + self.layout.hblank_px())
    }

    /// The stored samples of a frame: dark optical black, `active_e` in every active sample.
    fn field(&self, active_e: f32) -> Stream {
        let mut stream = Stream::new(self.layout.clone());
        let ob = self.layout.ob_cols() as usize;
        for line in stream
            .samples_mut()
            .chunks_exact_mut(self.layout.line_len() as usize)
        {
            line[ob..].fill(active_e);
        }
        stream
    }

    /// Converts frame `index` and returns what the CDS read of every stored sample, in electrons.
    fn read(&self, state: &mut afe::State, stream: &Stream, index: u64) -> Vec<f64> {
        let mut analog = Analog::new(self.layout.clone());
        afe::process(&self.afe, state, stream, &self.rail, index, &mut analog);
        let gain = self.afe.volts_per_electron();
        analog
            .volts()
            .iter()
            .map(|&v| (f64::from(v) - self.afe.pedestal_v) / gain)
            .collect()
    }

    /// Returns the index into the stored samples of frame period `period`, if it holds a sample.
    fn sample(&self, period: u64) -> Option<usize> {
        let (line, at) = (period / self.line_px(), period % self.line_px());
        (line < u64::from(self.layout.rows()) && at < u64::from(self.layout.line_len()))
            .then(|| (line * u64::from(self.layout.line_len()) + at) as usize)
    }

    /// Returns whether frame period `period` holds an active sample, not optical black or blanking.
    fn active(&self, period: u64) -> bool {
        let at = period % self.line_px();
        self.sample(period).is_some() && at >= u64::from(self.layout.ob_cols())
    }

    /// The runs of missed pulses of frame `index` from `state`.
    fn runs(&self, state: &afe::State, index: u64) -> Vec<Range<u64>> {
        afe::missed_runs(&self.afe, state, &self.layout, index)
    }
}

fn assert_same(read_e: f64, expected_e: f64, what: &str) {
    assert!(
        (read_e - expected_e).abs() < SAME_E,
        "{what}: read {read_e} e, expected {expected_e} e"
    );
}

fn build(preset: Preset) -> Simulator {
    Simulator::builder((camera(), preset))
        .input_size(SCENE_WIDTH, SCENE_HEIGHT)
        .build()
        .unwrap()
}

/// A scene with a white sky over a grey gradient and a dark band.
fn scene() -> Scene {
    let rgb: Vec<u8> = (0..SCENE_HEIGHT)
        .flat_map(|y| {
            (0..SCENE_WIDTH).flat_map(move |x| {
                let v = match y * 3 / SCENE_HEIGHT {
                    0 => 255,
                    1 => (x * 255 / SCENE_WIDTH) as u8,
                    _ => 12,
                };
                [v, v, v]
            })
        })
        .collect();
    Scene::from_srgb8(SCENE_WIDTH, SCENE_HEIGHT, &rgb)
}

fn render(sim: &mut Simulator, scene: &Scene) -> Vec<Frame> {
    (0..FRAMES)
        .map(|index| sim.frame(scene, index).clone())
        .collect()
}

#[test]
fn an_inert_section_renders_byte_identically_to_none() {
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        let mut none = with_reset(None);
        none.readout.mode = mode;
        let expected = render(&mut build(none.clone()), &scene);
        for bend in [ResetBend::default(), drops(0.0, 200.0)] {
            let mut inert = none.clone();
            inert.bend.reset = Some(bend);
            assert_eq!(
                render(&mut build(inert), &scene),
                expected,
                "{mode} with {bend:?} differs from no section"
            );
        }
    }
}

#[test]
fn a_missed_run_reads_black_once_the_diffusion_fills_the_swing() {
    let chain = Chain::new(&with_reset(Some(drops(5000.0, 8.0))));
    let bright_e = camera().sensor.full_well_e as f32;
    let bright = chain.field(bright_e);
    let read = chain.read(&mut afe::State::default(), &bright, 0);
    let sound = chain.sound().read(&mut afe::State::default(), &bright, 0);
    let packet_e = f64::from(bright_e);
    let (mut checked, mut black) = (0, 0);
    for run in chain.runs(&afe::State::default(), 0) {
        // Runs over bright packets only, the one before and the one after included.
        if run.start == 0 || !(run.start - 1..=run.end).all(|period| chain.active(period)) {
            continue;
        }
        checked += 1;
        for (missed, period) in (1u32..).zip(run.clone()) {
            let i = chain.sample(period).unwrap();
            // The diffusion holds the packet before the run and every packet since.
            let held_e = f64::from(missed) * packet_e;
            if held_e + packet_e <= chain.swing_e {
                assert_same(read[i], sound[i], "a packet within the swing");
            } else if held_e >= chain.swing_e {
                assert_same(read[i], 0.0, "a packet beyond the swing");
                black += 1;
            } else {
                assert!(
                    read[i] > 0.0 && read[i] < packet_e,
                    "the packet that fills the swing"
                );
            }
        }
        let after = chain.sample(run.end).unwrap();
        assert_same(read[after], sound[after], "the packet after the run");
    }
    assert!(checked >= 10, "only {checked} runs over the bright field");
    assert!(black > 0, "no missed run outlasted the swing");
    // Outside the runs every packet reads as with a sound gate.
    let mut missed = vec![false; read.len()];
    for period in chain.runs(&afe::State::default(), 0).into_iter().flatten() {
        if let Some(i) = chain.sample(period) {
            missed[i] = true;
        }
    }
    for ((&read, &sound), _) in read.iter().zip(&sound).zip(&missed).filter(|(_, m)| !**m) {
        assert_same(read, sound, "a packet outside the runs");
    }
}

#[test]
fn a_dark_run_survives_missed_resets() {
    let chain = Chain::new(&with_reset(Some(drops(5000.0, 8.0))));
    let runs = chain.runs(&afe::State::default(), 0);
    let longest = runs.iter().map(|run| run.end - run.start).max().unwrap();
    assert!(
        (longest + 1) as f64 * f64::from(DARK_E) < chain.swing_e,
        "a run of {longest} periods fills the swing with dark packets"
    );
    let dark = chain.field(DARK_E);
    let read = chain.read(&mut afe::State::default(), &dark, 0);
    let sound = chain.sound().read(&mut afe::State::default(), &dark, 0);
    for (&read, &sound) in read.iter().zip(&sound) {
        assert_same(read, sound, "a dark packet");
    }
}

#[test]
fn a_weak_reset_walks_a_bright_run_into_black() {
    let r = 0.25;
    let chain = Chain::new(&with_reset(Some(weak(r))));
    let e = camera().sensor.full_well_e;
    let swing = chain.swing_e;
    // Bright enough that the charge a reset leaves behind fills the swing on its own.
    assert!((1.0 - r) * e / r >= swing);
    let read = chain.read(&mut afe::State::default(), &chain.field(e as f32), 0);
    // The first line from power-on: the dark optical black leaves the diffusion empty,
    // and after n packets of e it holds q_n = (e / r) (1 - (1 - r)^n).
    let ob = chain.layout.ob_cols() as usize;
    let charge = |n: i32| e / r * (1.0 - (1.0 - r).powi(n));
    let mut fell = false;
    for n in 1..=12 {
        let expected = charge(n).min(swing) - ((1.0 - r) * charge(n - 1)).min(swing);
        let got = read[ob + n as usize - 1];
        assert!(
            (got - expected).abs() <= 0.01 * e,
            "packet {n}: read {got} e, the closed form gives {expected} e"
        );
        fell |= expected < 0.5 * e;
    }
    assert!(fell, "the closed form never fell");
    assert!(read[ob] > 0.99 * e, "the first packet did not read in full");
    // Further along every line the diffusion stays full and the packets read black.
    let line_len = chain.layout.line_len() as usize;
    for line in read.chunks_exact(line_len) {
        for &tail in &line[line_len - 16..] {
            assert_same(tail, 0.0, "the end of a bright line");
        }
    }
}

#[test]
fn a_weak_reset_below_the_swing_cancels_in_the_cds() {
    let e = camera().sensor.full_well_e as f32;
    // At half strength the diffusion settles at 2 e, within the swing.
    let half = Chain::new(&with_reset(Some(weak(0.5))));
    assert!(2.0 * f64::from(e) < half.swing_e);
    let bright = half.field(e);
    let read = half.read(&mut afe::State::default(), &bright, 0);
    let sound = half.sound().read(&mut afe::State::default(), &bright, 0);
    for (&read, &sound) in read.iter().zip(&sound) {
        assert_same(read, sound, "a bright packet at half strength");
    }
    // A dark field never comes near the swing even at a quarter.
    let quarter = Chain::new(&with_reset(Some(weak(0.25))));
    let dark = quarter.field(DARK_E);
    let read = quarter.read(&mut afe::State::default(), &dark, 0);
    let sound = quarter.sound().read(&mut afe::State::default(), &dark, 0);
    for (&read, &sound) in read.iter().zip(&sound) {
        assert_same(read, sound, "a dark packet at a quarter strength");
    }
}

#[test]
fn a_late_reset_sample_sees_the_diffusion() {
    // A reset sample taken at the signal sample, with full weight, cancels the packet
    // whatever the diffusion did to it.
    let mut preset = with_reset(Some(ResetBend {
        drop_hz: 5000.0,
        hold_px: 8.0,
        reset_frac: 0.5,
    }));
    preset.bend.afe = Some(AfeBend {
        cds_lag_px: 0.0,
        cds_mix: 1.0,
        ..AfeBend::default()
    });
    let chain = Chain::new(&preset);
    let bright = chain.field(camera().sensor.full_well_e as f32);
    let read = chain.read(&mut afe::State::default(), &bright, 0);
    for &read in &read {
        assert_same(read, 0.0, "a packet minus its own reading");
    }
}

#[test]
fn a_missed_run_carries_the_charge_across_a_line() {
    let chain = Chain::new(&with_reset(Some(drops(500.0, 8000.0))));
    let (line_px, line_len, ob) = (
        chain.line_px(),
        u64::from(chain.layout.line_len()),
        u64::from(chain.layout.ob_cols()),
    );
    // Runs are long enough to outlast a line's blanking, which at match-input
    // resolution is most of the line.
    assert!(8000 > 2 * (line_px - line_len));
    let bright_e = camera().sensor.full_well_e;
    let bright = chain.field(bright_e as f32);
    let (mut carried, mut fresh) = (0, 0);
    for index in 0..8 {
        let read = chain.read(&mut afe::State::default(), &bright, index);
        let mut end = 0;
        for run in chain.runs(&afe::State::default(), index) {
            // Where the run before this one ended: the reset returned there.
            let previous_end = std::mem::replace(&mut end, run.end);
            let line = run.start / line_px;
            let first_active = (line + 1) * line_px + ob;
            if line + 1 >= u64::from(chain.layout.rows()) || run.end <= first_active {
                continue;
            }
            let i = chain.sample(first_active).unwrap();
            let at = run.start % line_px;
            if at >= ob && at + 3 <= line_len {
                // Three bright packets of the line before have filled the swing, and
                // the diffusion takes them through blanking and the dark optical black.
                assert_same(read[i], 0.0, "the first packet after a full line end");
                carried += 1;
            } else if at >= line_len && previous_end < line * line_px + line_len {
                // A run from blanking after a reset at the line's last sample holds only
                // that packet and reads on.
                assert_same(
                    read[i],
                    bright_e,
                    "the first packet after a run from blanking",
                );
                fresh += 1;
            }
        }
    }
    assert!(
        carried > 0 && fresh > 0,
        "{carried} carried and {fresh} fresh runs"
    );
}

/// What frame 2 of a bright field reads after frames 0 and 1, and from power-on.
fn frame_two(chain: &Chain) -> (bool, Vec<f64>, Vec<f64>) {
    let bright = chain.field(camera().sensor.full_well_e as f32);
    let mut state = afe::State::default();
    for index in 0..2 {
        chain.read(&mut state, &bright, index);
    }
    let spans = state.reset_missed();
    let continued = chain.read(&mut state, &bright, 2);
    let fresh = chain.read(&mut afe::State::default(), &bright, 2);
    (spans, continued, fresh)
}

#[test]
fn a_missed_run_carries_the_charge_into_the_next_frame() {
    // Runs and the stretches between them average 2000 periods, so about half the
    // seeds leave a run in progress at VD.
    let mut cases = [false, false];
    for seed in 0..64 {
        let mut preset = with_reset(Some(drops(950.0, 2000.0)));
        preset.seed = seed;
        let (spans, continued, fresh) = frame_two(&Chain::new(&preset));
        if spans {
            assert_ne!(
                continued, fresh,
                "seed {seed}: a run across VD left no trace"
            );
        } else {
            assert_eq!(continued, fresh, "seed {seed}: frame 2 remembers frame 1");
        }
        cases[usize::from(spans)] = true;
        if cases == [true, true] {
            return;
        }
    }
    panic!("64 seeds did not give both a run across VD and none: {cases:?}");
}

#[test]
fn the_bend_replays_from_reset() {
    let scene = scene();
    let preset = with_reset(Some(ResetBend {
        drop_hz: 20_000.0,
        hold_px: 8.0,
        reset_frac: 0.3,
    }));
    let mut sim = build(preset.clone());
    let first = render(&mut sim, &scene);
    assert_ne!(
        first,
        render(&mut build(with_reset(None)), &scene),
        "the bend left the scene alone"
    );
    sim.reset();
    assert_eq!(
        render(&mut sim, &scene),
        first,
        "the replay from reset differs"
    );
    assert_eq!(
        render(&mut build(preset), &scene),
        first,
        "a fresh camera differs"
    );
}

#[test]
fn a_new_drop_rate_keeps_the_diffusion_charge() {
    let scene = scene();
    let mut sim = build(with_reset(Some(ResetBend {
        drop_hz: 20_000.0,
        hold_px: 8.0,
        reset_frac: 0.3,
    })));
    render(&mut sim, &scene);
    let before = sim.afe_state().clone();
    assert!(before.fd_charge_e() > 0.0);
    let turned = with_reset(Some(ResetBend {
        drop_hz: 500.0,
        hold_px: 8.0,
        reset_frac: 0.3,
    }));
    assert_eq!(sim.set_preset(turned).unwrap(), KEPT);
    assert_eq!(sim.afe_state(), &before, "the AFE lost its state");
    assert!((sim.afe_state().fd_charge_e() - before.fd_charge_e()).abs() < f64::EPSILON);
}

/// Returns the mean length of the runs of frame 0 that start and end within it, and their count.
fn run_lengths(chain: &Chain) -> (f64, usize) {
    let runs = chain.runs(&afe::State::default(), 0);
    let last = runs.last().map_or(0, |run| run.end);
    let whole: Vec<u64> = runs
        .iter()
        .filter(|run| run.start > 0 && run.end < last)
        .map(|run| run.end - run.start)
        .collect();
    (
        whole.iter().sum::<u64>() as f64 / whole.len() as f64,
        whole.len(),
    )
}

#[test]
fn a_run_spans_the_same_pixels_at_every_resolution() {
    let (drop_hz, hold_px) = (100_000.0, 8.0);
    let camera = camera();
    let mut native = Preset::new("a520");
    native.bend.reset = Some(drops(drop_hz, hold_px));
    let mut small = native.clone();
    small.readout.resolution = Resolution::MatchInput;
    for (preset, width, height) in [(&native, 2272, 1704), (&small, 640, 480)] {
        let timing = Timing::derive(&camera, preset, width, height).unwrap();
        let readout = readout::Params::resolve(&camera, preset, &timing).unwrap();
        let afe = afe::Params::resolve(&camera, preset, &timing).unwrap();
        let pixel_s = readout.layout.pixel_period_s();
        let gate = afe.reset.clone().unwrap();
        assert!(
            (gate.hold_s / pixel_s / hold_px - 1.0).abs() < 1e-9,
            "{width} wide: a run of {} s is {} periods",
            gate.hold_s,
            gate.hold_s / pixel_s
        );
        let chain = Chain {
            rail: Rail::new(&readout.layout, camera.power.rail_v),
            swing_e: 0.0,
            layout: readout.layout,
            afe,
        };
        let (mean_px, count) = run_lengths(&chain);
        assert!(
            (mean_px / hold_px - 1.0).abs() < 0.05,
            "{width} wide: runs average {mean_px} pixels of {count}"
        );
        // Runs start at drop_hz while none runs: one per gap of 1 / p and run of hold_px periods.
        let p = drop_hz * pixel_s;
        let periods = timing.frame_period_s / pixel_s;
        let expected = periods / (1.0 / p + hold_px);
        assert!(
            (count as f64 / expected - 1.0).abs() < 0.05,
            "{width} wide: {count} runs in a frame, expected {expected}"
        );
    }
}

#[test]
fn reset_values_are_checked() {
    let parsed: Preset =
        toml::from_str("camera = \"a520\"\n[bend.reset]\ndrop_hz = 200.0\n").unwrap();
    assert_eq!(
        parsed.bend.reset,
        Some(drops(200.0, ResetBend::default().hold_px))
    );
    parsed.validate().unwrap();
    toml::from_str::<Preset>("camera = \"a520\"\n[bend.reset]\nhold = 2.0\n").unwrap_err();
    for bad in [
        drops(-1.0, 8.0),
        drops(f64::NAN, 8.0),
        drops(10.0, 0.5),
        drops(10.0, f64::INFINITY),
        weak(1.5),
        weak(-0.1),
    ] {
        let error = with_reset(Some(bad)).validate().unwrap_err();
        assert!(error.is_config(), "{bad:?}: {error}");
    }
    let mut camera = camera();
    camera.afe.fd_swing_v = 0.0;
    assert!(camera.validate().unwrap_err().is_config());
}
