//! Timings of every stage of the signal chain on a native A520 still.
//!
//! The benches render a synthetic 2272 by 1704 scene (a sky gradient with a clipped
//! sun, textured ground, dark buildings with lit windows) through `cameras/a520.toml`
//! in still mode at native resolution, with two presets: `clean`, the unmodified
//! camera, and `hpf`, the bus bend with the bypass function.
//!
//! Each stage bench times one public stage function on the signal that the stages
//! before it produced. Persistent state carries on from one iteration to the next, as
//! in a video. The two stages that rewrite their buffer in place, the bus bend and the
//! codec, get a fresh copy of their input for every iteration, made outside the timed
//! region. The `frame` bench times [`Simulator::frame`], the whole chain.
//!
//! Run with `cargo bench -p vcb_core`.

use std::fs;
use std::path::Path;

use divan::Bencher;
use vcb_core::adc::{self, Codes};
use vcb_core::afe::{self, Analog};
use vcb_core::isp::{self, Raw};
use vcb_core::power::Rail;
use vcb_core::preset::{Mode, Resolution};
use vcb_core::readout::{self, Stream};
use vcb_core::sensor::{self, Charge};
use vcb_core::{Camera, Frame, Preset, Scene, Simulator, Timing, bus, codec, noise, scene};

fn main() {
    divan::main();
}

/// Presets benchmarked, named by their file `presets/a520-<name>.toml`.
const PRESETS: [&str; 2] = ["clean", "hpf"];

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
    afe: afe::Params,
    adc: adc::Params,
    bus: Option<bus::Params>,
    isp: isp::Params,
    codec: codec::Params,
    scene_state: scene::State,
    sensor_state: sensor::State,
    readout_state: readout::State,
    afe_state: afe::State,
    bus_state: bus::State,
    isp_state: isp::State,
    codec_state: codec::State,
    prepared: Scene,
    charge: Charge,
    stream: Stream,
    /// The camera's nominal supply rail on every line.
    rail: Rail,
    analog: Analog,
    /// The ADC's words, before the bend.
    codes: Codes,
    /// The words the image processor latches, after the bend.
    bent: Codes,
    raw: Raw,
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
            afe: afe::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("afe", &e)),
            adc: adc::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("adc", &e)),
            bus: bus::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("bus", &e)),
            isp,
            codec: codec::Params::resolve(c, p, t).unwrap_or_else(|e| unresolved("codec", &e)),
            scene_state: scene::State::default(),
            sensor_state: sensor::State::default(),
            readout_state: readout::State::default(),
            afe_state: afe::State::default(),
            bus_state: bus::State::default(),
            isp_state: isp::State::default(),
            codec_state: codec::State::default(),
            prepared: Scene::new(width, height),
            charge: Charge::new(width, height),
            stream: Stream::new(layout.clone()),
            rail: Rail::new(&layout, camera.power.rail_v),
            analog: Analog::new(layout.clone()),
            codes: Codes::new(layout.clone()),
            bent: Codes::new(layout),
            raw: Raw::new(cols, rows),
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
        isp::clamp_black(&self.isp, &self.bent, &self.rail, &mut self.raw);
        isp::develop(
            &self.isp,
            &mut self.isp_state,
            &self.raw,
            &mut self.developed,
        );
    }
}

#[divan::bench(args = PRESETS, sample_count = 20)]
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

#[divan::bench(args = PRESETS, sample_count = 20)]
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

#[divan::bench(args = PRESETS, sample_count = 20)]
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

#[divan::bench(args = PRESETS, sample_count = 20)]
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

#[divan::bench(args = PRESETS, sample_count = 20)]
fn isp_clamp_black(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    bencher.bench_local(|| isp::clamp_black(&chain.isp, &chain.bent, &chain.rail, &mut chain.raw));
}

#[divan::bench(args = PRESETS, sample_count = 20)]
fn isp_develop(bencher: Bencher, preset: &str) {
    let mut chain = Chain::new(preset);
    let Chain {
        isp,
        isp_state,
        raw,
        developed,
        ..
    } = &mut chain;
    bencher.bench_local(|| isp::develop(isp, isp_state, raw, developed));
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

#[divan::bench(args = PRESETS, sample_count = 20)]
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
