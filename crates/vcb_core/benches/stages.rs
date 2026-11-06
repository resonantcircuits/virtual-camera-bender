//! Timings of every stage of the signal chain on a native A520 still.
//!
//! The benches render a synthetic 2272 by 1704 scene (a sky gradient with a clipped
//! sun, textured ground, dark buildings with lit windows) through `cameras/a520.toml`
//! in still mode at native resolution. Every stage runs with two presets: `clean`, the
//! unmodified camera, and `hpf`, the bus bend with the bypass function. The stages that
//! milestones 3 and 4 bent also run a bent case: the scene with `full-spectrum` (the
//! IR-cut filter removed), the sensor with `heat-gun` (a hot CCD: its dark current,
//! the current's spread and the amplifier's glow) and `substrate` (no overflow drain,
//! an overexposure that blooms), the supply rail and the AFE on it with `brownout` (a sagging, rippling rail),
//! the AFE with `reset-drag` (a weak, flaky reset gate), the processor's capture with
//! `reclock-roll` (a detuned, wandering oscillator) and the frame buffer with
//! `s9000-tear` (two marginal address contacts).
//!
//! Each stage bench times one public stage function on the signal that the stages
//! before it produced. Persistent state carries on from one iteration to the next, as
//! in a video. The stages that rewrite their buffer in place, the bus bend, the capture,
//! the frame buffer and the codec, get a fresh copy of their input for every iteration,
//! made outside the timed region. The bus, capture and frame-buffer benches time one
//! iteration per sample: their pass-through case takes nanoseconds, and divan would
//! otherwise copy a frame-sized input for each of hundreds of iterations at once. The
//! `frame` bench times [`Simulator::frame`], the whole chain.
//!
//! Run with `cargo bench -p vcb_core`.

use std::fs;
use std::path::Path;

use divan::Bencher;
use vcb_core::adc::{self, Codes};
use vcb_core::afe::{self, Analog};
use vcb_core::isp::{self, Raw};
use vcb_core::power::{self, Rail};
use vcb_core::preset::{Mode, Resolution};
use vcb_core::readout::{self, Stream};
use vcb_core::sensor::{self, Charge};
use vcb_core::{
    Camera, Frame, Preset, Scene, Simulator, Timing, bus, capture, codec, memory, noise, scene,
};

fn main() {
    divan::main();
}

/// Presets benchmarked by every stage, named by their file `presets/a520-<name>.toml`.
const PRESETS: [&str; 2] = ["clean", "hpf"];

/// Presets of the scene bench: the two of every stage and the IR-cut filter removed.
const SCENE_PRESETS: [&str; 3] = ["clean", "hpf", "full-spectrum"];

/// Presets of the sensor bench: the two of every stage, a hot CCD and a drainless one.
const SENSOR_PRESETS: [&str; 4] = ["clean", "hpf", "heat-gun", "substrate"];

/// Presets of the AFE bench: the two of every stage, a sagging rail and a weak reset gate.
const AFE_PRESETS: [&str; 4] = ["clean", "hpf", "brownout", "reset-drag"];

/// Presets of the supply-rail bench: no rail bend, and a loaded, rippling rail.
const POWER_PRESETS: [&str; 2] = ["clean", "brownout"];

/// Presets of the capture bench: no reclock, and a detuned, wandering oscillator.
const CAPTURE_PRESETS: [&str; 2] = ["clean", "reclock-roll"];

/// Presets of the frame-buffer bench: no memory bend, and two marginal address contacts.
const MEMORY_PRESETS: [&str; 2] = ["clean", "s9000-tear"];

/// Presets of the whole-chain bench: the two of every stage and one per bent stage.
const FRAME_PRESETS: [&str; 8] = [
    "clean",
    "hpf",
    "full-spectrum",
    "heat-gun",
    "brownout",
    "reset-drag",
    "reclock-roll",
    "s9000-tear",
];

/// Width of the scene: the A520's native still width.
const WIDTH: u32 = 2272;

/// Height of the scene: the A520's native still height.
const HEIGHT: u32 = 1704;

/// Reads the A520 camera file and a preset, forced to a native still readout.
fn load(name: &str) -> (Camera, Preset) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |path: &Path| {
        fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
    };
    let camera_path = root.join("cameras/a520.toml");
    let camera: Camera = toml::from_str(&read(&camera_path))
        .unwrap_or_else(|error| panic!("cannot parse {}: {error}", camera_path.display()));
    let preset_path = root.join(format!("presets/a520-{name}.toml"));
    let mut preset: Preset = toml::from_str(&read(&preset_path))
        .unwrap_or_else(|error| panic!("cannot parse {}: {error}", preset_path.display()));
    preset.readout.mode = Mode::Still;
    preset.readout.resolution = Resolution::Native;
    (camera, preset)
}

/// Draws the benchmark scene: sky with a clipped sun, buildings, textured ground.
fn input() -> Scene {
    let (width, height) = (WIDTH as f32, HEIGHT as f32);
    let horizon = 0.55 * height;
    let (sun_x, sun_y, sun_r) = (0.75 * width, 0.2 * height, 0.04 * width);
    let band_px = WIDTH / 12;
    let mut rgb = Vec::with_capacity(WIDTH as usize * HEIGHT as usize * 3);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (px, py) = (x as f32, y as f32);
            let band = x / band_px;
            let roof = horizon - height * 0.15f32.mul_add(noise::unit(band, 0, 1, 1), 0.05);
            let building = band % 2 == 1 && py > roof && py < 0.05f32.mul_add(height, horizon);
            let pixel = if building {
                let lit = x % 24 < 10 && y % 32 < 14 && noise::unit(x / 24, y / 32, 2, 1) > 0.6;
                if lit {
                    [250.0, 220.0, 140.0]
                } else {
                    [40.0, 38.0, 45.0]
                }
            } else if py < horizon {
                let t = py / horizon;
                let glow = 120.0 * (1.0 - (px - sun_x).hypot(py - sun_y) / sun_r).exp();
                [
                    130.0f32.mul_add(t, 60.0),
                    105.0f32.mul_add(t, 110.0),
                    40.0f32.mul_add(t, 200.0),
                ]
                .map(|c| c + glow)
            } else {
                let t = (py - horizon) / (height - horizon);
                let grain = 24.0 * (noise::unit(x, y, 3, 1) - 0.5);
                [30.0f32.mul_add(t, 90.0), 10.0f32.mul_add(t, 100.0), 50.0].map(|c| c + grain)
            };
            rgb.extend(pixel.map(|c: f32| c.clamp(0.0, 255.0) as u8));
        }
    }
    Scene::from_srgb8(WIDTH, HEIGHT, &rgb)
}

/// Every stage's parameters, state and output, primed by one pass through the chain.
struct Chain {
    input: Scene,
    scene: scene::Params,
    sensor: sensor::Params,
    readout: readout::Params,
    power: Option<power::Params>,
    afe: afe::Params,
    adc: adc::Params,
    bus: Option<bus::Params>,
    capture: Option<capture::Params>,
    isp: isp::Params,
    memory: Option<memory::Params>,
    codec: codec::Params,
    scene_state: scene::State,
    sensor_state: sensor::State,
    readout_state: readout::State,
    power_state: power::State,
    afe_state: afe::State,
    bus_state: bus::State,
    capture_state: capture::State,
    memory_state: memory::State,
    isp_state: isp::State,
    codec_state: codec::State,
    prepared: Scene,
    charge: Charge,
    stream: Stream,
    /// The supply rail on every line: the camera's nominal without a rail bend.
    rail: Rail,
    analog: Analog,
    /// The ADC's words, before the bend.
    codes: Codes,
    /// The words on the bus, after the bend.
    bent: Codes,
    /// The words the image processor latches on its own clock, after the capture.
    captured: Codes,
    /// The clamped raw, before the frame buffer.
    raw: Raw,
    /// The raw read back from the frame buffer, before development.
    read_back: Raw,
    /// The developed frame, before the codec.
    developed: Frame,
}

/// Panics with the stage whose parameters failed to resolve.
fn unresolved(stage: &str, error: &vcb_core::Error) -> ! {
    panic!("cannot resolve the {stage} stage: {error}")
}

impl Chain {
    /// Resolves every stage for the named preset and runs the chain once.
    fn new(name: &str) -> Self {
        let (camera, preset) = load(name);
        let timing = Timing::derive(&camera, &preset, WIDTH, HEIGHT)
            .unwrap_or_else(|error| panic!("cannot derive the timing: {error}"));
        let (c, p, t) = (&camera, &preset, &timing);
        let readout =
            readout::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("readout", &e));
        let isp = isp::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("isp", &e));
        let layout = readout.layout.clone();
        let (cols, rows) = (layout.cols(), layout.rows());
        let (width, height) = (timing.sensor_width, timing.sensor_height);
        let developed = Frame::new(isp.output.width, isp.output.height);
        let mut chain = Self {
            input: input(),
            scene: scene::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("scene", &e)),
            sensor: sensor::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("sensor", &e)),
            readout,
            power: power::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("power", &e)),
            afe: afe::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("afe", &e)),
            adc: adc::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("adc", &e)),
            bus: bus::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("bus", &e)),
            capture: capture::Params::resolve(c, p, t)
                .unwrap_or_else(|e| unresolved("capture", &e)),
            isp,
            memory: memory::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("memory", &e)),
            codec: codec::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("codec", &e)),
            scene_state: scene::State::default(),
            sensor_state: sensor::State::default(),
            readout_state: readout::State::default(),
            power_state: power::State::default(),
            afe_state: afe::State::default(),
            bus_state: bus::State::default(),
            capture_state: capture::State::default(),
            memory_state: memory::State::default(),
            isp_state: isp::State::default(),
            codec_state: codec::State::default(),
            prepared: Scene::new(width, height),
            charge: Charge::new(width, height),
            stream: Stream::new(layout.clone()),
            rail: Rail::new(&layout, camera.power.rail_v),
            analog: Analog::new(layout.clone()),
            codes: Codes::new(layout.clone()),
            bent: Codes::new(layout.clone()),
            captured: Codes::new(layout),
            raw: Raw::new(cols, rows),
            read_back: Raw::new(cols, rows),
            developed,
        };
        chain.run();
        chain
    }

    /// Runs every stage once, in the simulator's order.
    fn run(&mut self) {
        scene::prepare(
            &self.scene,
            &mut self.scene_state,
            &self.input,
            &mut self.prepared,
        );
        let charge = &mut self.charge;
        sensor::expose(
            &self.sensor,
            &mut self.sensor_state,
            &self.prepared,
            0,
            charge,
        );
        let stream = &mut self.stream;
        readout::read(
            &self.readout,
            &mut self.readout_state,
            &self.charge,
            0,
            stream,
        );
        power::track(
            self.power.as_ref(),
            &mut self.power_state,
            &self.bus_state,
            0,
            &mut self.rail,
        );
        afe::process(
            &self.afe,
            &mut self.afe_state,
            &self.stream,
            &self.rail,
            0,
            &mut self.analog,
        );
        adc::convert(&self.adc, &self.analog, &mut self.codes);
        self.bent.words_mut().copy_from_slice(self.codes.words());
        bus::bend(
            self.bus.as_ref(),
            &mut self.bus_state,
            &self.rail,
            0,
            &mut self.bent,
        );
        self.captured.words_mut().copy_from_slice(self.bent.words());
        capture::reframe(
            self.capture.as_ref(),
            &mut self.capture_state,
            &self.rail,
            0,
            &mut self.captured,
        );
        isp::clamp_black(&self.isp, &self.captured, &self.rail, &mut self.raw);
        self.read_back.clone_from(&self.raw);
        memory::bend(
            self.memory.as_ref(),
            &mut self.memory_state,
            0,
            &mut self.read_back,
        );
        isp::develop(
            &self.isp,
            &mut self.isp_state,
            &self.read_back,
            &mut self.developed,
        );
    }
}

#[divan::bench(args = SCENE_PRESETS, sample_count = 20)]
fn scene_prepare(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    bencher.bench_local(|| {
        scene::prepare(
            &chain.scene,
            &mut chain.scene_state,
            &chain.input,
            &mut chain.prepared,
        );
    });
}

#[divan::bench(args = SENSOR_PRESETS, sample_count = 20)]
fn sensor_expose(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    let Chain {
        sensor,
        sensor_state,
        prepared,
        charge,
        ..
    } = &mut chain;
    bencher.bench_local(|| sensor::expose(sensor, sensor_state, prepared, 0, charge));
}

#[divan::bench(args = PRESETS, sample_count = 20)]
fn readout_read(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    let Chain {
        readout,
        readout_state,
        charge,
        stream,
        ..
    } = &mut chain;
    bencher.bench_local(|| readout::read(readout, readout_state, charge, 0, stream));
}

#[divan::bench(args = POWER_PRESETS, sample_count = 20)]
fn power_track(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    let Chain {
        power,
        power_state,
        bus_state,
        rail,
        ..
    } = &mut chain;
    bencher.bench_local(|| power::track(power.as_ref(), power_state, bus_state, 0, rail));
}

#[divan::bench(args = AFE_PRESETS, sample_count = 20)]
fn afe_process(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    let Chain {
        afe,
        afe_state,
        stream,
        rail,
        analog,
        ..
    } = &mut chain;
    bencher.bench_local(|| afe::process(afe, afe_state, stream, rail, 0, analog));
}

#[divan::bench(args = PRESETS, sample_count = 20)]
fn adc_convert(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    bencher.bench_local(|| adc::convert(&chain.adc, &chain.analog, &mut chain.codes));
}

#[divan::bench(args = PRESETS, sample_count = 20, sample_size = 1)]
fn bus_bend(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    let Chain {
        bus,
        bus_state,
        rail,
        codes,
        ..
    } = &mut chain;
    bencher
        .with_inputs(|| codes.clone())
        .bench_local_refs(|codes| bus::bend(bus.as_ref(), bus_state, rail, 0, codes));
}

#[divan::bench(args = CAPTURE_PRESETS, sample_count = 20, sample_size = 1)]
fn capture_reframe(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    let Chain {
        capture,
        capture_state,
        rail,
        bent,
        ..
    } = &mut chain;
    bencher
        .with_inputs(|| bent.clone())
        .bench_local_refs(|codes| {
            capture::reframe(capture.as_ref(), capture_state, rail, 0, codes);
        });
}

#[divan::bench(args = PRESETS, sample_count = 20)]
fn isp_clamp_black(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    bencher.bench_local(|| {
        isp::clamp_black(&chain.isp, &chain.captured, &chain.rail, &mut chain.raw);
    });
}

#[divan::bench(args = MEMORY_PRESETS, sample_count = 20, sample_size = 1)]
fn memory_bend(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    let Chain {
        memory,
        memory_state,
        raw,
        ..
    } = &mut chain;
    bencher
        .with_inputs(|| raw.clone())
        .bench_local_refs(|raw| memory::bend(memory.as_ref(), memory_state, 0, raw));
}

#[divan::bench(args = PRESETS, sample_count = 20)]
fn isp_develop(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    let Chain {
        isp,
        isp_state,
        read_back,
        developed,
        ..
    } = &mut chain;
    bencher.bench_local(|| isp::develop(isp, isp_state, read_back, developed));
}

#[divan::bench(args = PRESETS, sample_count = 20)]
fn codec_encode(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    let Chain {
        codec,
        codec_state,
        developed,
        ..
    } = &mut chain;
    bencher
        .with_inputs(|| developed.clone())
        .bench_local_refs(|frame| codec::encode(codec, codec_state, frame));
}

#[divan::bench(args = FRAME_PRESETS, sample_count = 20)]
fn frame(bencher: Bencher, preset: &str) {
    let (camera, preset) = load(preset);
    let input = input();
    let mut simulator = Simulator::builder((camera, preset))
        .input_size(WIDTH, HEIGHT)
        .build()
        .unwrap_or_else(|error| panic!("cannot build the simulator: {error}"));
    bencher.bench_local(|| {
        simulator.frame(&input, 0);
    });
}
