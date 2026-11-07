//! The stateful simulator that runs the signal chain frame by frame.

use std::fmt::{self, Display, Formatter};

use crate::adc::{self, Codes};
use crate::afe::{self, Analog};
use crate::isp::{self, Raw};
use crate::preset::Resolution;
use crate::readout::{self, Stream};
use crate::sensor::{self, Charge};
use crate::{
    Camera, Error, Frame, Preset, Scene, Timing, bus, capture, codec, frame, memory, power, scene,
};

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
        let buffers = Buffers::new(&camera, &params, &timing);
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

/// Returns whether two timings read out the same geometry on the same processor clock.
///
/// Everything but the resolution mode and the sensor-side clock must agree: sensor
/// size, rows read, columns, optical black, blanking, line skip, the processor's
/// capture period, the native pixel period, the frame period, oversampling,
/// orientation and output size. Then every buffer keeps its size. The sensor-side
/// pixel period, pixel clock and line period and the clock ratio may differ: they
/// follow the oscillator of `[bend.clock]`, a knob on the running camera, and the
/// kept buffers take the new period ([`Buffers::retime`]).
///
/// This is the only test that decides between keeping and rebuilding.
fn same_geometry(old: &Timing, new: &Timing) -> bool {
    let aligned = Timing {
        resolution: old.resolution,
        pixel_clock_hz: old.pixel_clock_hz,
        pixel_period_s: old.pixel_period_s,
        line_period_s: old.line_period_s,
        clock_ratio: old.clock_ratio,
        ..new.clone()
    };
    aligned == *old
}

/// The resolved parameters of every stage.
#[derive(Debug, Clone)]
struct Stages {
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
}

impl Stages {
    fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let sensor = sensor::Params::resolve(camera, preset, timing)?;
        let mut afe = afe::Params::resolve(camera, preset, timing)?;
        // The output amplifier's read noise is the sensor's at its temperature.
        afe.read_noise_e *= sensor.read_noise_scale();
        Ok(Self {
            scene: scene::Params::resolve(camera, preset, timing)?,
            sensor,
            readout: readout::Params::resolve(camera, preset, timing)?,
            power: power::Params::resolve(camera, preset, timing)?,
            afe,
            adc: adc::Params::resolve(camera, preset, timing)?,
            bus: bus::Params::resolve(camera, preset, timing)?,
            capture: capture::Params::resolve(camera, preset, timing)?,
            isp: isp::Params::resolve(camera, preset, timing)?,
            memory: memory::Params::resolve(camera, preset, timing)?,
            codec: codec::Params::resolve(camera, preset, timing)?,
        })
    }
}

/// The persistent state of every stage, as at power-on when default.
#[derive(Debug, Clone, Default)]
struct States {
    scene: scene::State,
    sensor: sensor::State,
    readout: readout::State,
    power: power::State,
    afe: afe::State,
    bus: bus::State,
    capture: capture::State,
    isp: isp::State,
    memory: memory::State,
    codec: codec::State,
}

/// The intermediate signal of every stage, allocated once.
#[derive(Debug, Clone)]
struct Buffers {
    scene: Scene,
    charge: Charge,
    stream: Stream,
    /// The supply rail of every readout line.
    rail: power::Rail,
    analog: Analog,
    codes: Codes,
    raw: Raw,
    frame: Frame,
    /// The frame turned upright, when the camera is held in portrait.
    upright: Option<Frame>,
}

impl Buffers {
    fn new(camera: &Camera, params: &Stages, timing: &Timing) -> Self {
        let layout = &params.readout.layout;
        Self {
            scene: Scene::new(timing.sensor_width, timing.sensor_height),
            charge: Charge::new(timing.sensor_width, timing.sensor_height),
            stream: Stream::new(layout.clone()),
            rail: power::Rail::new(layout, camera.power.rail_v),
            analog: Analog::new(layout.clone()),
            codes: Codes::new(layout.clone()),
            raw: Raw::new(layout.cols(), layout.rows()),
            frame: Frame::new(params.isp.output.width, params.isp.output.height),
            upright: timing
                .rotated
                .then(|| Frame::new(timing.output_width, timing.output_height)),
        }
    }

    /// Gives every buffer that carries readout timing the layout of `params`, keeping the samples.
    ///
    /// The stream, the voltages and the codes store the pixel period of the clock that
    /// produced them; a reconfiguration that keeps the geometry may move the
    /// sensor-side clock, so they take the new period while their counts stay.
    ///
    /// # Panics
    ///
    /// Panics if the new layout differs in anything but its pixel period, which a
    /// kept geometry rules out.
    fn retime(&mut self, params: &Stages) {
        let layout = &params.readout.layout;
        assert_eq!(
            self.rail.lines(),
            layout.rows(),
            "a kept geometry reads the same lines"
        );
        self.stream.retime(layout.clone());
        self.analog.retime(layout.clone());
        self.codes.retime(layout.clone());
    }
}

/// What reconfiguring a running [`Simulator`] kept of its buffers and state.
///
/// [`Simulator::set_preset`] and [`Simulator::set_scene_size`] return it. A
/// reconfiguration that keeps the readout geometry keeps every buffer and the memory of
/// every stage except those it reports reset; one that changes the geometry rebuilds
/// the simulator as [`SimulatorBuilder::build`] does, at power-on.
///
/// # Examples
///
/// ```
/// use vcb_core::preset::{Mode, Resolution, SensorOverride};
/// use vcb_core::{Camera, Preset, Reconfigured, Simulator};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
/// let camera: Camera = toml::from_str(&std::fs::read_to_string(path)?)?;
/// let mut preset = Preset::new("a520");
/// preset.readout.resolution = Resolution::MatchInput;
/// let mut sim = Simulator::builder((camera, preset.clone()))
///     .input_size(32, 24)
///     .build()?;
///
/// // A longer exposure is a knob on the running camera.
/// preset.sensor = Some(SensorOverride { exposure_s: Some(0.03) });
/// let longer = sim.set_preset(preset.clone())?;
/// assert_eq!(longer, Reconfigured::Kept { bus_reset: false, servo_reset: false });
/// assert!(longer.state_kept());
///
/// // Another readout mode is another geometry.
/// preset.readout.mode = Mode::Movie;
/// assert_eq!(sim.set_preset(preset)?, Reconfigured::Rebuilt);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reconfigured {
    /// The readout geometry stayed: every buffer is kept, and every stage's state but
    /// the ones flagged.
    Kept {
        /// The bus bend returned to power-on, because its wiring changed or the bend was
        /// added or removed.
        bus_reset: bool,
        /// A servo of the image processor forgot its value, because the preset turned
        /// it off.
        servo_reset: bool,
    },
    /// The readout geometry changed: buffers were reallocated and every stage is at
    /// power-on, as after [`Simulator::reset`].
    Rebuilt,
}

impl Reconfigured {
    /// Returns whether every buffer kept its allocation.
    #[must_use]
    pub fn buffers_kept(self) -> bool {
        matches!(self, Self::Kept { .. })
    }

    /// Returns whether every stage kept its state.
    #[must_use]
    pub fn state_kept(self) -> bool {
        self == Self::Kept {
            bus_reset: false,
            servo_reset: false,
        }
    }

    /// Returns whether the bus bend returned to power-on.
    #[must_use]
    pub fn bus_reset(self) -> bool {
        match self {
            Self::Kept { bus_reset, .. } => bus_reset,
            Self::Rebuilt => true,
        }
    }

    /// Returns whether the image processor's servos lost a value.
    #[must_use]
    pub fn servo_reset(self) -> bool {
        match self {
            Self::Kept { servo_reset, .. } => servo_reset,
            Self::Rebuilt => true,
        }
    }
}

impl Display for Reconfigured {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            Self::Kept {
                bus_reset: false,
                servo_reset: false,
            } => "kept buffers and state",
            Self::Kept {
                bus_reset: true,
                servo_reset: false,
            } => "kept buffers and state, reset the bus bend",
            Self::Kept {
                bus_reset: false,
                servo_reset: true,
            } => "kept buffers and state, reset a servo",
            Self::Kept {
                bus_reset: true,
                servo_reset: true,
            } => "kept buffers and state, reset the bus bend and a servo",
            Self::Rebuilt => "rebuilt at power-on",
        })
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
    /// phases of the injected and gain-modulating oscillators, the AFE's low-pass node,
    /// flicker-noise level and serially programmed registers (which a write lost to
    /// `[bend.serial]` leaves as the last frame had them), the bus bend's capacitor
    /// voltages, protection-diode and
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
        scene::prepare(&params.scene, &mut state.scene, scene, &mut b.scene);
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
        power::track(
            params.power.as_ref(),
            &mut state.power,
            &state.bus,
            index,
            &mut b.rail,
        );
        afe::process(
            &params.afe,
            &mut state.afe,
            &b.stream,
            &b.rail,
            index,
            &mut b.analog,
        );
        adc::convert(&params.adc, &b.analog, &mut b.codes);
        bus::bend(
            params.bus.as_ref(),
            &mut state.bus,
            &b.rail,
            index,
            &mut b.codes,
        );
        capture::reframe(
            params.capture.as_ref(),
            &mut state.capture,
            &b.rail,
            index,
            &mut b.codes,
        );
        isp::clamp_black(&params.isp, &b.codes, &b.rail, &mut b.raw);
        memory::bend(params.memory.as_ref(), &mut state.memory, index, &mut b.raw);
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
    ///   old one's phase, and its serial registers whatever `[bend.serial]` becomes,
    ///   until the next frame's writes.
    /// - Running clock faults run on, and the exposure and white-balance servos keep
    ///   their values; a servo that the preset turns off forgets its value at once.
    /// - The supply rail, the processor's capture and the frame-buffer SDRAM keep their
    ///   state whatever `[bend.optics]`, `[bend.rail]`, `[bend.clock]` and
    ///   `[bend.memory]` become, a new oscillator frequency included; removing
    ///   `[bend.rail]` returns the rail to the nominal voltage, and removing
    ///   `[bend.memory]` forgets the SDRAM's content.
    ///
    /// Everything else, the exposure time of the preset's `[sensor]` section included,
    /// takes effect with the next frame. Otherwise parameters, buffers and state are
    /// rebuilt as `build` would, as after [`reset`](Self::reset). The returned
    /// [`Reconfigured`] says which of the two happened and which stages were reset.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`SimulatorBuilder::build`] other than a missing input
    /// size: a configuration error if a preset value is invalid, the preset names
    /// another camera or a bus pin does not exist on the camera's ADC, and an input error
    /// if the scene size is too small for the readout.
    pub fn set_preset(&mut self, preset: Preset) -> Result<Reconfigured, Error> {
        self.reconfigure(preset, self.scene_size)
    }

    /// Changes the size of the scenes to come, under the rules of [`set_preset`](Self::set_preset).
    ///
    /// The size decides the sensor at match-input resolution and the orientation when
    /// it is `auto`; at native resolution in a fixed orientation nothing changes and
    /// everything is kept.
    ///
    /// # Errors
    ///
    /// Returns an input error if the size is too small for the readout; the simulator
    /// then keeps its previous size.
    pub fn set_scene_size(&mut self, width: u32, height: u32) -> Result<Reconfigured, Error> {
        self.reconfigure(self.preset.clone(), (width, height))
    }

    /// Applies a preset and scene size, keeping what the geometry allows.
    fn reconfigure(
        &mut self,
        preset: Preset,
        scene_size: (u32, u32),
    ) -> Result<Reconfigured, Error> {
        check_preset(&self.camera, &preset)?;
        let timing = Timing::derive(&self.camera, &preset, scene_size.0, scene_size.1)?;
        let params = Stages::resolve(&self.camera, &preset, &timing)?;
        let outcome = if same_geometry(&self.timing, &timing) {
            self.buffers.retime(&params);
            let bus_reset =
                !bus::same_wiring(self.preset.bend.bus.as_ref(), preset.bend.bus.as_ref());
            if bus_reset {
                self.state.bus = bus::State::default();
            }
            let servo_reset = isp::stops_servo(&self.params.isp, &params.isp);
            self.state.isp.forget_stopped_servos(&params.isp);
            if self.params.power.is_some() && params.power.is_none() {
                self.buffers.rail.reset();
            }
            if self.params.memory.is_some() && params.memory.is_none() {
                self.state.memory = memory::State::default();
            }
            Reconfigured::Kept {
                bus_reset,
                servo_reset,
            }
        } else {
            self.buffers = Buffers::new(&self.camera, &params, &timing);
            self.state = States::default();
            trace_timing("sim.reconfigure.done", &self.camera, &timing);
            Reconfigured::Rebuilt
        };
        self.params = params;
        self.timing = timing;
        self.preset = preset;
        self.scene_size = scene_size;
        Ok(outcome)
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
    ///
    /// The supply rail returns to the nominal voltage with it.
    pub fn reset(&mut self) {
        self.state = States::default();
        self.buffers.rail.reset();
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

    /// Returns the supply rail's state, for tests.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn power_state(&self) -> &power::State {
        &self.state.power
    }

    /// Returns the words the processor captured in the last frame, for tests.
    ///
    /// The ADC's codes after the bus bend and the capture, before the black clamp.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn codes(&self) -> &Codes {
        &self.buffers.codes
    }

    /// Returns the supply rail of every readout line of the last frame, for tests.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn rail(&self) -> &power::Rail {
        &self.buffers.rail
    }

    /// Returns the processor capture's state, for tests.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn capture_state(&self) -> &capture::State {
        &self.state.capture
    }

    /// Returns the frame-buffer SDRAM's state, for tests.
    #[cfg(feature = "test-util")]
    #[must_use]
    pub fn memory_state(&self) -> &memory::State {
        &self.state.memory
    }
}
