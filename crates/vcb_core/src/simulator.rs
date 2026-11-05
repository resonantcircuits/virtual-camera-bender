//! The stateful simulator that runs the signal chain frame by frame.

use crate::adc::{self, Codes};
use crate::afe::{self, Analog};
use crate::isp::{self, Raw};
use crate::preset::Resolution;
use crate::readout::{self, Stream};
use crate::sensor::{self, Charge};
use crate::{Camera, Error, Frame, Preset, Scene, Timing, bus, codec, frame, scene};

/// The required inputs of a [`Simulator`]: the camera and the preset.
#[derive(Debug, Clone, PartialEq)]
pub struct SimDeps {
    /// The camera model.
    pub camera: Camera,
    /// The preset; its `camera` must name the camera model.
    pub preset: Preset,
}

impl From<(Camera, Preset)> for SimDeps {
    fn from(value: (Camera, Preset)) -> Self {
        let (camera, preset) = value;
        Self { camera, preset }
    }
}

/// Builder of a [`Simulator`], created by [`Simulator::builder`].
#[derive(Debug, Clone)]
pub struct SimulatorBuilder {
    deps: SimDeps,
    input_size: Option<(u32, u32)>,
}

impl SimulatorBuilder {
    /// Sets the size of the input images, which match-input resolution needs.
    #[must_use]
    pub fn input_size(mut self, width: u32, height: u32) -> Self {
        self.input_size = Some((width, height));
        self
    }

    /// Validates the configuration, derives the timing and allocates every buffer.
    ///
    /// Without an input size the scene is taken to have the camera's native size.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a camera or preset value is invalid, the preset
    /// names another camera, a bus pin does not exist on the camera's ADC, or the
    /// preset uses match-input resolution without an input size. Returns an input
    /// error if the input size is too small for the readout.
    pub fn build(self) -> Result<Simulator, Error> {
        let SimDeps { camera, preset } = self.deps;
        camera.validate()?;
        check_preset(&camera, &preset)?;
        let scene_size = match (self.input_size, preset.readout.resolution) {
            (Some(size), _) => size,
            (None, Resolution::Native) => (camera.sensor.native_width, camera.sensor.native_height),
            (None, Resolution::MatchInput) => {
                return Err(Error::config(
                    "match-input resolution needs the input size; call input_size()",
                ));
            }
        };
        let timing = Timing::derive(&camera, &preset, scene_size.0, scene_size.1)?;
        let params = Stages::resolve(&camera, &preset, &timing)?;
        let buffers = Buffers::new(&params, &timing);
        trace_timing("sim.build.done", &camera, &timing);
        Ok(Simulator {
            camera,
            preset,
            scene_size,
            timing,
            params,
            state: States::default(),
            buffers,
        })
    }
}

/// Checks a preset on its own and against the camera it must name.
fn check_preset(camera: &Camera, preset: &Preset) -> Result<(), Error> {
    preset.validate()?;
    if preset.camera == camera.info.name {
        Ok(())
    } else {
        Err(Error::config(format!(
            "preset is for camera \"{}\" but the camera model is \"{}\"",
            preset.camera, camera.info.name
        )))
    }
}

/// Emits the derived readout of a newly configured simulator.
fn trace_timing(name: &'static str, camera: &Camera, timing: &Timing) {
    tracing::debug!(
        name,
        camera = %camera.info.name,
        mode = %timing.mode,
        resolution = %timing.resolution,
        sensor_width = timing.sensor_width,
        sensor_height = timing.sensor_height,
        frame_period_s = timing.frame_period_s,
        "simulator configured"
    );
}

/// Returns whether two timings read out the same geometry on the same clock.
///
/// Everything but the resolution mode must agree: sensor size, rows read, columns,
/// optical black, blanking, line skip, clocks, oversampling, orientation and output
/// size. Then every buffer keeps its size and every layout its pixel period.
fn same_geometry(old: &Timing, new: &Timing) -> bool {
    let renamed = Timing {
        resolution: old.resolution,
        ..new.clone()
    };
    renamed == *old
}

/// The resolved parameters of every stage.
#[derive(Debug, Clone)]
struct Stages {
    scene: scene::Params,
    sensor: sensor::Params,
    readout: readout::Params,
    afe: afe::Params,
    adc: adc::Params,
    bus: Option<bus::Params>,
    isp: isp::Params,
    codec: codec::Params,
}

impl Stages {
    fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        Ok(Self {
            scene: scene::Params::resolve(camera, preset, timing)?,
            sensor: sensor::Params::resolve(camera, preset, timing)?,
            readout: readout::Params::resolve(camera, preset, timing)?,
            afe: afe::Params::resolve(camera, preset, timing)?,
            adc: adc::Params::resolve(camera, preset, timing)?,
            bus: bus::Params::resolve(camera, preset, timing)?,
            isp: isp::Params::resolve(camera, preset, timing)?,
            codec: codec::Params::resolve(camera, preset, timing)?,
        })
    }
}

/// The persistent state of every stage, as at power-on when default.
#[derive(Debug, Clone, Default)]
struct States {
    sensor: sensor::State,
    readout: readout::State,
    afe: afe::State,
    bus: bus::State,
    isp: isp::State,
    codec: codec::State,
}

/// The intermediate signal of every stage, allocated once.
#[derive(Debug, Clone)]
struct Buffers {
    scene: Scene,
    charge: Charge,
    stream: Stream,
    analog: Analog,
    codes: Codes,
    raw: Raw,
    frame: Frame,
    /// The frame turned upright, when the camera is held in portrait.
    upright: Option<Frame>,
}

impl Buffers {
    fn new(params: &Stages, timing: &Timing) -> Self {
        let layout = &params.readout.layout;
        Self {
            scene: Scene::new(timing.sensor_width, timing.sensor_height),
            charge: Charge::new(timing.sensor_width, timing.sensor_height),
            stream: Stream::new(layout.clone()),
            analog: Analog::new(layout.clone()),
            codes: Codes::new(layout.clone()),
            raw: Raw::new(layout.cols(), layout.rows()),
            frame: Frame::new(params.isp.output.width, params.isp.output.height),
            upright: timing
                .rotated
                .then(|| Frame::new(timing.output_width, timing.output_height)),
        }
    }
}

/// A simulated camera that turns scenes into frames, one call per frame.
///
/// The simulator owns the camera, the preset, the resolved parameters of every stage,
/// the persistent analog state that carries from frame to frame (oscillator phase,
/// circuit nodes, the exposure and white-balance servos) and all working buffers, so
/// rendering a frame does not allocate. [`set_preset`](Self::set_preset) and
/// [`set_scene_size`](Self::set_scene_size) change the configuration of a running
/// simulator, as turning a knob on the bent camera would.
///
/// # Examples
///
/// ```
/// use vcb_core::{Camera, Preset, Scene, Simulator};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
/// let camera: Camera = toml::from_str(&std::fs::read_to_string(path)?)?;
/// let mut sim = Simulator::builder((camera, Preset::new("a520")))
///     .input_size(32, 24)
///     .build()?;
/// let scene = Scene::from_srgb8(32, 24, &[90; 32 * 24 * 3]);
/// let first = sim.frame(&scene, 0).clone();
/// sim.reset();
/// assert_eq!(sim.frame(&scene, 0), &first, "the same sequence from reset is identical");
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct Simulator {
    camera: Camera,
    preset: Preset,
    scene_size: (u32, u32),
    timing: Timing,
    params: Stages,
    state: States,
    buffers: Buffers,
}

impl Simulator {
    /// Starts building a simulator for a camera and a preset.
    pub fn builder(deps: impl Into<SimDeps>) -> SimulatorBuilder {
        SimulatorBuilder {
            deps: deps.into(),
            input_size: None,
        }
    }

    /// Renders `scene` as frame `index` of a sequence and returns the upright frame.
    ///
    /// The scene may have any non-zero size; it is cropped to the sensor's aspect ratio
    /// and resampled to the sensor. `index` selects the temporal noise of the frame.
    ///
    /// # Determinism
    ///
    /// The hardware's memory persists from one call to the next, as in the camera: the
    /// phases of the injected and gain-modulating oscillators, the AFE's low-pass node
    /// and flicker-noise level, the bus bend's capacitor voltages, protection-diode and
    /// gate states, clock faults that straddle the frame boundary, and the exposure and
    /// white balance that the servos (`ae_mode` and `wb` set to `"servo"`) metered on
    /// the previous frame. Nothing else carries over: scene auto-exposure meters each
    /// frame on its own and the black clamp starts afresh from each frame's first line.
    /// The same sequence of scenes and indices rendered from a fresh or
    /// [`reset`](Self::reset) simulator, with the same camera, preset and seed, is
    /// identical byte for byte. Rendering the same index twice without a reset in
    /// between is not guaranteed to repeat, because the state has moved on.
    ///
    /// # Panics
    ///
    /// Panics if `scene` is empty.
    pub fn frame(&mut self, scene: &Scene, index: u64) -> &Frame {
        let Self {
            params,
            state,
            buffers: b,
            ..
        } = self;
        scene::prepare(&params.scene, scene, &mut b.scene);
        params.sensor.exposure_scale = state.isp.servo_exposure();
        sensor::expose(
            &params.sensor,
            &mut state.sensor,
            &b.scene,
            index,
            &mut b.charge,
        );
        readout::read(
            &params.readout,
            &mut state.readout,
            &b.charge,
            index,
            &mut b.stream,
        );
        afe::process(&params.afe, &mut state.afe, &b.stream, index, &mut b.analog);
        adc::convert(&params.adc, &b.analog, &mut b.codes);
        bus::bend(params.bus.as_ref(), &mut state.bus, index, &mut b.codes);
        isp::clamp_black(&params.isp, &b.codes, &mut b.raw);
        isp::develop(&params.isp, &mut state.isp, &b.raw, &mut b.frame);
        isp::meter(
            &params.isp,
            &mut state.isp,
            &b.raw,
            state.sensor.exposure_scale(),
        );
        codec::encode(&params.codec, &mut state.codec, &mut b.frame);
        tracing::trace!(
            name: "sim.frame.done",
            index,
            black_level = b.raw.black_level(),
            exposure_scale = state.sensor.exposure_scale(),
            "frame rendered"
        );
        match &mut b.upright {
            Some(upright) => {
                frame::rotate_ccw(&b.frame, upright);
                upright
            }
            None => &b.frame,
        }
    }

    /// Replaces the preset, keeping buffers and analog state where the hardware allows.
    ///
    /// The preset is checked as [`SimulatorBuilder::build`] checks it, and the timing
    /// derived for the present [`scene_size`](Self::scene_size). On an error nothing
    /// changes and the simulator renders as before.
    ///
    /// # State
    ///
    /// If the readout geometry is unchanged (sensor size, rows read, columns, optical
    /// black, blanking, clocks, mode, orientation, output size and oversampling; the
    /// resolution mode may differ if those agree), every stage's parameters are resolved
    /// anew, every buffer is kept, and so is the hardware's memory, as when a knob turns
    /// on a running camera:
    ///
    /// - The bus bend keeps its capacitor voltages, diodes and gates while its wiring
    ///   stays: the same source pins, target, function, potentiometer node, common bus,
    ///   whether patch resistors are fitted and whether the pot is set to zero (a short
    ///   to ground). A changed pot, capacitor or patch resistor value keeps the charge.
    ///   A change of wiring, or adding or removing the bend, returns the bend to its
    ///   power-on state.
    /// - The AFE keeps its low-pass node, flicker level and oscillator phases whatever
    ///   the injection or gain modulation becomes, so a new waveform continues from the
    ///   old one's phase.
    /// - Running clock faults run on, and the exposure and white-balance servos keep
    ///   their values; a servo that the preset turns off forgets its value after the
    ///   next frame.
    ///
    /// Otherwise parameters, buffers and state are rebuilt as `build` would, as after
    /// [`reset`](Self::reset).
    ///
    /// # Errors
    ///
    /// Returns the errors of [`SimulatorBuilder::build`] other than a missing input
    /// size: a configuration error if a preset value is invalid, the preset names
    /// another camera or a bus pin does not exist on the camera's ADC, and an input error
    /// if the scene size is too small for the readout.
    pub fn set_preset(&mut self, preset: Preset) -> Result<(), Error> {
        self.reconfigure(preset, self.scene_size)
    }

    /// Changes the size of the scenes to come, under the rules of [`set_preset`](Self::set_preset).
    ///
    /// The size decides the sensor at match-input resolution and the orientation when
    /// it is `auto`; at native resolution in a fixed orientation nothing changes.
    ///
    /// # Errors
    ///
    /// Returns an input error if the size is too small for the readout; the simulator
    /// then keeps its previous size.
    pub fn set_scene_size(&mut self, width: u32, height: u32) -> Result<(), Error> {
        self.reconfigure(self.preset.clone(), (width, height))
    }

    /// Applies a preset and scene size, keeping what the geometry allows.
    fn reconfigure(&mut self, preset: Preset, scene_size: (u32, u32)) -> Result<(), Error> {
        check_preset(&self.camera, &preset)?;
        let timing = Timing::derive(&self.camera, &preset, scene_size.0, scene_size.1)?;
        let params = Stages::resolve(&self.camera, &preset, &timing)?;
        if same_geometry(&self.timing, &timing) {
            if !bus::same_wiring(self.preset.bend.bus.as_ref(), preset.bend.bus.as_ref()) {
                self.state.bus = bus::State::default();
            }
        } else {
            self.buffers = Buffers::new(&params, &timing);
            self.state = States::default();
            trace_timing("sim.reconfigure.done", &self.camera, &timing);
        }
        self.params = params;
        self.timing = timing;
        self.preset = preset;
        self.scene_size = scene_size;
        Ok(())
    }

    /// Returns the camera model.
    #[must_use]
    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Returns the preset in use.
    #[must_use]
    pub fn preset(&self) -> &Preset {
        &self.preset
    }

    /// Returns the width and height of the scenes the timing was derived for.
    ///
    /// The input size given to the builder or to [`set_scene_size`](Self::set_scene_size),
    /// or the camera's native size if none was given.
    #[must_use]
    pub fn scene_size(&self) -> (u32, u32) {
        self.scene_size
    }

    /// Returns the derived readout timing.
    #[must_use]
    pub fn timing(&self) -> &Timing {
        &self.timing
    }

    /// Returns the exposure scale the sensor applied to the last frame; 1 is the set exposure.
    #[must_use]
    pub fn exposure_scale(&self) -> f32 {
        self.state.sensor.exposure_scale()
    }

    /// Returns every stage to its power-on state.
    pub fn reset(&mut self) {
        self.state = States::default();
    }

    /// Returns the bus bend's state, for tests.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn bus_state(&self) -> &bus::State {
        &self.state.bus
    }

    /// Returns the image processor's state, for tests.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn isp_state(&self) -> &isp::State {
        &self.state.isp
    }

    /// Returns the analog front end's state, for tests.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn afe_state(&self) -> &afe::State {
        &self.state.afe
    }
}
