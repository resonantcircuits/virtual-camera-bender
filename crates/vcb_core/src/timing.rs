//! Derived readout clocks: the time and resolution model.

use std::fmt::{self, Display, Formatter};

use crate::camera::Clocking;
use crate::preset::{Mode, Orientation, Resolution};
use crate::validate::{at_least, positive};
use crate::{Camera, Error, Preset};

/// Fewest optical-black columns per line on a scaled-down virtual sensor.
///
/// The black clamp averages the optical-black samples of every line; below about
/// eight samples its estimate starts to follow the read noise.
const MIN_OB_COLS: u32 = 8;

/// Fewest horizontal blanking periods per line on a virtual sensor.
///
/// Inputs with more rows than the native sensor leave too little time per line for
/// the native active-line time; the line then keeps this much blanking and the frame
/// period grows instead.
const MIN_HBLANK_PX: u32 = 8;

/// The derived clocks and geometry of one readout mode at one sensor size.
///
/// A line consists of `ob_cols` optical-black samples, `cols` active samples and
/// `hblank_px` blanking periods; a frame of `rows_read` lines and `vblank_lines`
/// blanking lines. Hence
///
/// - `line_period_s = pixel_period_s * (ob_cols + cols + hblank_px)` on the sensor's
///   clock and
/// - `frame_period_s = dsp_pixel_period_s * (ob_cols + cols + hblank_px) * (rows_read +
///   vblank_lines)` on the processor's.
///
/// Without `[bend.clock]` the two clocks are one and `frame_period_s = line_period_s *
/// (rows_read + vblank_lines)`.
///
/// # Two clocks
///
/// The camera has two oscillators. The processor's crystal clocks its capture of the bus
/// and generates vertical sync (VD); the timing generator's clocks the CCD's transfer,
/// the AFE, the ADC and the bus drivers. A preset's `[bend.clock]` replaces the timing
/// generator's with one at `osc_mhz` (the LTC1799 mod), so the sensor side runs at
/// `clock_ratio = osc_mhz * 1e6 / pixel_clock_hz` of the camera's nominal clock while the
/// processor keeps the nominal:
///
/// > `pixel_period_s = dsp_pixel_period_s / clock_ratio`,
///
/// where `dsp_pixel_period_s` is the period this derivation gives the readout at this
/// resolution (the camera's own at native resolution, the fitted one at match-input), so
/// match-input scaling and the ratio compose. Line and pixel counts are the same on both
/// sides. The timing generator restarts with every VD, reads until the next and loses
/// what it has not read by then ([`capture`](crate::capture)). Each consumer uses the
/// clock of its hardware:
///
/// | Consumer | Clock | Quantity |
/// |---|---|---|
/// | Sensor: interline smear's transfer time, capped at the processor's frame period | sensor | `line_period_s`, `frame_period_s` |
/// | Readout: clock-fault rates per line, the stream's period | sensor | `line_period_s`, `pixel_period_s` |
/// | AFE: sub-steps, blanking intervals, an oscillator's advance per sample | sensor | `pixel_period_s` |
/// | ADC and bus bend: one word per period, circuit sub-steps | sensor | `pixel_period_s` |
/// | Supply rail, sampled per readout line | sensor | `line_period_s` |
/// | Sub-line analog constants: AFE bandwidth and slew, gate delays and setup, the drivers' slew, line capacitance | fixed in seconds by the hardware | scaled by [`resolution_scale`](Self::resolution_scale) alone |
/// | Processor's latch on the bus | processor | `dsp_pixel_period_s` |
/// | Image processor's exposure and white-balance servos, the movie electronic shutter, frame-buffer reads one frame later | processor (VD) | `frame_period_s` |
/// | Oscillators: AFE injection and gain modulation, flicker corner, ripple, the reclocked oscillator's drift | real time: sensor periods within the readout, VD to VD from frame to frame | seconds |
///
/// A slower oscillator therefore lengthens the sensor's lines in seconds while the AFE's
/// bandwidth stays in hertz, so the bandwidth spans more of a sensor pixel period: it
/// widens in pixels. The frame period is the processor's at every ratio, so a new
/// oscillator keeps the geometry of a running simulator.
///
/// # Resolution
///
/// At [`Resolution::Native`] the sensor has the camera's size and the pixel clock is
/// the camera's. At [`Resolution::MatchInput`] the sensor takes the input's size,
/// rounded down to even, and two native durations are kept fixed:
///
/// - the **active-line time**, `cols * dsp_pixel_period_s`, so a horizontal time constant
///   spans the same fraction of the image width at every resolution, and
/// - the **frame period**, so a vertical time constant or an oscillator spans the same
///   fraction of the image height.
///
/// The pixel period follows from the first; the optical-black columns and vertical
/// blanking scale with the sensor; horizontal blanking absorbs the rest of each line.
/// The pixel period is trimmed by at most half a period per line so a line holds a
/// whole number of periods. An input with more rows than the native sensor would
/// need negative blanking: the line keeps a minimum blanking and the frame period
/// grows beyond the native one.
///
/// # Sub-line constants
///
/// Analog constants shorter than a line (AFE bandwidth and slew, gate propagation
/// delays, minimum pulse and setup time, the flip-flop's maximum clock) are given in
/// physical time units, which at native resolution means a fixed number of native
/// pixel periods. Stages that model them scale them by
/// [`resolution_scale`](Self::resolution_scale), `dsp_pixel_period_s /
/// native_pixel_period_s`, so they keep their native meaning in pixels at every
/// resolution; without `[bend.clock]` that is `pixel_period_s / native_pixel_period_s`.
/// The reclocked oscillator does not enter the scale: it moves the clock, not the
/// analog hardware. The latch phase of the image processor is a fraction of the pixel
/// period and needs no scaling.
///
/// # Orientation
///
/// When the camera is held in portrait (`rotated`), the scene is rotated 90 degrees
/// clockwise onto the landscape sensor and the developed frame rotated back, so
/// `output_width` and `output_height` describe the upright frame.
///
/// Construct a `Timing` with [`Timing::derive`]; the fields are public for reading.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Timing {
    /// Still or movie readout.
    pub mode: Mode,
    /// How the sensor size was chosen.
    pub resolution: Resolution,
    /// Whether the camera is held in portrait and the scene rotated onto the sensor.
    pub rotated: bool,
    /// Active photosite columns of the virtual sensor.
    pub sensor_width: u32,
    /// Active photosite rows of the virtual sensor.
    pub sensor_height: u32,
    /// Rows read per frame; fewer than `sensor_height` in movie mode.
    pub rows_read: u32,
    /// Active samples per line.
    pub cols: u32,
    /// Optical-black samples at the start of every line.
    pub ob_cols: u32,
    /// Horizontal blanking per line in pixel periods.
    pub hblank_px: u32,
    /// Vertical blanking per frame in line periods.
    pub vblank_lines: u32,
    /// One row pair is read out of every `line_skip` row pairs.
    pub line_skip: u32,
    /// Horizontal pixel clock of the timing generator and AFE in hertz, `1 / pixel_period_s`.
    pub pixel_clock_hz: f64,
    /// Duration of one pixel period of the timing generator and AFE in seconds.
    ///
    /// The sensor, readout, AFE, ADC and bus stages run on it: `dsp_pixel_period_s /
    /// clock_ratio`.
    pub pixel_period_s: f64,
    /// Duration of one pixel period of the camera's own clock in seconds.
    pub native_pixel_period_s: f64,
    /// Duration of the processor's capture period, one pixel period of its own clock, in seconds.
    ///
    /// The processor keeps its own crystal and latches the bus once per capture period,
    /// while the timing generator and the AFE run on the sensor-side `pixel_period_s`.
    /// The camera's own period at native resolution, the fitted one at match-input.
    pub dsp_pixel_period_s: f64,
    /// Frequency of the sensor-side clock over the camera's nominal pixel clock.
    ///
    /// `osc_mhz * 1e6` over the readout mode's `pixel_clock_hz` with `[bend.clock]`;
    /// 1 without.
    pub clock_ratio: f64,
    /// Duration of one sensor-side line, including optical black and blanking, in seconds.
    pub line_period_s: f64,
    /// Duration of one frame of the processor, from VD to VD, in seconds.
    pub frame_period_s: f64,
    /// Analog simulation sub-steps per pixel period.
    pub oversample: u32,
    /// Width of the upright output frame in pixels.
    pub output_width: u32,
    /// Height of the upright output frame in pixels.
    pub output_height: u32,
}

impl Timing {
    /// Derives the readout timing for a camera, a preset and an input size.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if the camera's clocking or the preset's
    /// `[bend.clock] osc_mhz` is invalid, and an input error if the input is empty or too
    /// small to hold one Bayer quad per output pixel.
    ///
    /// # Examples
    ///
    /// ```
    /// use vcb_core::{Camera, Preset, Timing};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
    /// let camera: Camera = toml::from_str(&std::fs::read_to_string(path)?)?;
    /// let preset = Preset::new("a520");
    /// let small = Timing::derive(&camera, &preset, 640, 480)?;
    /// let large = Timing::derive(&camera, &preset, 1920, 1440)?;
    /// assert!((small.frame_period_s / large.frame_period_s - 1.0).abs() < 1e-9);
    /// # Ok(())
    /// # }
    /// ```
    pub fn derive(
        camera: &Camera,
        preset: &Preset,
        input_width: u32,
        input_height: u32,
    ) -> Result<Self, Error> {
        if input_width == 0 || input_height == 0 {
            return Err(Error::input(format!(
                "input is empty ({input_width} x {input_height})"
            )));
        }
        let mode = preset.readout.mode;
        let resolution = preset.readout.resolution;
        let rotated = held_in_portrait(preset.readout.orientation, input_width, input_height);
        let (scene_width, scene_height) = if rotated {
            (input_height, input_width)
        } else {
            (input_width, input_height)
        };
        let clock = camera.clocking(mode);
        at_least("readout line_skip", clock.line_skip, 1)?;
        crate::validate::positive("readout pixel_clock_hz", clock.pixel_clock_hz)?;
        let h_bin = match mode {
            Mode::Still => 1,
            Mode::Movie => camera.movie.h_bin.max(1),
        };

        let native_w = camera.sensor.native_width;
        let native_h = camera.sensor.native_height;
        let native_rows = rows_read(native_h, clock.line_skip);
        let native_ob = camera.sensor.ob_cols;
        let native_pixel_period_s = 1.0 / clock.pixel_clock_hz;
        let native_frame_s = native_pixel_period_s
            * f64::from(native_ob + native_w + clock.hblank_px)
            * f64::from(native_rows + clock.vblank_lines);

        let (sensor_width, sensor_height) = match resolution {
            Resolution::Native => (native_w, native_h),
            Resolution::MatchInput => (scene_width & !1, scene_height & !1),
        };
        let rows = rows_read(sensor_height, clock.line_skip);
        if sensor_width < 2 || rows < 2 || sensor_width / h_bin < 1 {
            return Err(Error::input(format!(
                "a {input_width} x {input_height} input is too small for {mode} readout \
                 with line skip {} and column binning {h_bin}",
                clock.line_skip
            )));
        }

        let (ob_cols, hblank_px, vblank_lines, dsp_pixel_period_s) = match resolution {
            Resolution::Native => (
                native_ob,
                clock.hblank_px,
                clock.vblank_lines,
                native_pixel_period_s,
            ),
            Resolution::MatchInput => fit_line(
                &Native {
                    width: native_w,
                    rows: native_rows,
                    ob_cols: native_ob,
                    vblank_lines: clock.vblank_lines,
                    pixel_period_s: native_pixel_period_s,
                    frame_period_s: native_frame_s,
                },
                sensor_width,
                rows,
            ),
        };

        // The processor keeps the period derived above; the timing generator runs at
        // the replacement oscillator's share of the nominal clock.
        let (clock_ratio, pixel_period_s) = sensor_clock(preset, clock, dsp_pixel_period_s)?;
        let line_len = ob_cols + sensor_width + hblank_px;
        let lines = rows + vblank_lines;
        let line_period_s = pixel_period_s * f64::from(line_len);
        let frame_period_s = dsp_pixel_period_s * f64::from(line_len) * f64::from(lines);

        let (developed_width, developed_height) = match (mode, resolution) {
            (Mode::Still, _) => (sensor_width, rows),
            (Mode::Movie, Resolution::MatchInput) => (sensor_width / h_bin, rows),
            (Mode::Movie, Resolution::Native) => {
                (camera.movie.output_width, camera.movie.output_height)
            }
        };
        let (output_width, output_height) = if rotated {
            (developed_height, developed_width)
        } else {
            (developed_width, developed_height)
        };

        Ok(Self {
            mode,
            resolution,
            rotated,
            sensor_width,
            sensor_height,
            rows_read: rows,
            cols: sensor_width,
            ob_cols,
            hblank_px,
            vblank_lines,
            line_skip: clock.line_skip,
            pixel_clock_hz: 1.0 / pixel_period_s,
            pixel_period_s,
            native_pixel_period_s,
            dsp_pixel_period_s,
            clock_ratio,
            line_period_s,
            frame_period_s,
            oversample: camera.afe.oversample.max(1),
            output_width,
            output_height,
        })
    }

    /// Returns the size of the developed frame on the landscape sensor, before rotating back.
    #[must_use]
    pub fn developed_size(&self) -> (u32, u32) {
        if self.rotated {
            (self.output_height, self.output_width)
        } else {
            (self.output_width, self.output_height)
        }
    }

    /// Returns the pixel periods per line: optical black, active and blanking.
    #[must_use]
    pub fn line_len(&self) -> u32 {
        self.ob_cols + self.cols + self.hblank_px
    }

    /// Returns the line periods per frame: rows read and vertical blanking.
    #[must_use]
    pub fn lines_per_frame(&self) -> u32 {
        self.rows_read + self.vblank_lines
    }

    /// Returns how many sensor-side line periods one cycle of an oscillator at `freq_hz` lasts.
    #[must_use]
    pub fn lines_per_cycle(&self, freq_hz: f64) -> f64 {
        1.0 / (freq_hz * self.line_period_s)
    }

    /// Returns how many sensor-side pixel periods one cycle of an oscillator at `freq_hz` lasts.
    #[must_use]
    pub fn samples_per_cycle(&self, freq_hz: f64) -> f64 {
        1.0 / (freq_hz * self.pixel_period_s)
    }

    /// Returns the factor that stretches sub-line constants to the virtual resolution.
    ///
    /// `dsp_pixel_period_s / native_pixel_period_s`: 1 at native resolution, the
    /// virtual pixel's duration in native pixel periods at match-input. A stage
    /// multiplies a sub-line analog constant (a bandwidth's time constant, a gate delay,
    /// a slew time) by it, so the constant spans the same number of pixels at every
    /// resolution. It ignores `[bend.clock]`: a reclocked oscillator moves the clock,
    /// not the analog hardware, so in sensor pixel periods such a constant shrinks as
    /// the oscillator slows.
    #[must_use]
    pub fn resolution_scale(&self) -> f64 {
        self.dsp_pixel_period_s / self.native_pixel_period_s
    }
}

/// Returns the clock ratio and the sensor-side pixel period for the processor's period.
///
/// The ratio is the timing generator's oscillator over the readout mode's nominal
/// `pixel_clock_hz`, 1 without `[bend.clock]`, and the sensor-side period
/// `dsp_pixel_period_s / ratio`.
fn sensor_clock(
    preset: &Preset,
    clock: Clocking,
    dsp_pixel_period_s: f64,
) -> Result<(f64, f64), Error> {
    let ratio = match preset.bend.clock {
        Some(bend) => {
            positive("bend.clock.osc_mhz", bend.osc_mhz)?;
            bend.osc_mhz * 1e6 / clock.pixel_clock_hz
        }
        None => 1.0,
    };
    Ok((ratio, dsp_pixel_period_s / ratio))
}

/// Returns whether the camera is held in portrait for an input of the given size.
fn held_in_portrait(orientation: Orientation, width: u32, height: u32) -> bool {
    match orientation {
        Orientation::Auto => height > width,
        Orientation::Landscape => false,
        Orientation::Portrait => true,
    }
}

/// The native readout geometry that a match-input readout is fitted to.
struct Native {
    width: u32,
    rows: u32,
    ob_cols: u32,
    vblank_lines: u32,
    pixel_period_s: f64,
    frame_period_s: f64,
}

/// Fits a line of `cols` active samples to the native active-line time and frame period.
///
/// Returns `(ob_cols, hblank_px, vblank_lines, pixel_period_s)`.
fn fit_line(native: &Native, cols: u32, rows: u32) -> (u32, u32, u32, f64) {
    let sx = f64::from(cols) / f64::from(native.width);
    let sy = f64::from(rows) / f64::from(native.rows);
    let ob = ((f64::from(native.ob_cols) * sx).round() as u32)
        .max(MIN_OB_COLS.min(native.ob_cols))
        .max(1);
    let vblank = (f64::from(native.vblank_lines) * sy).round() as u32;
    let active_s = f64::from(native.width) * native.pixel_period_s;
    let pixel_s = active_s / f64::from(cols);
    let line_s = native.frame_period_s / f64::from(rows + vblank);
    let periods = (line_s / pixel_s).round();
    let stored = f64::from(ob + cols);
    if periods - stored >= f64::from(MIN_HBLANK_PX) {
        // Trim the pixel period so the line holds whole periods and the frame stays native.
        (ob, (periods - stored) as u32, vblank, line_s / periods)
    } else {
        (ob, MIN_HBLANK_PX, vblank, pixel_s)
    }
}

/// Rows read from a sensor of `height` rows: one row pair in every `line_skip` pairs.
fn rows_read(height: u32, line_skip: u32) -> u32 {
    height / (2 * line_skip) * 2
}

impl Display for Timing {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "mode          {}", self.mode)?;
        writeln!(f, "resolution    {}", self.resolution)?;
        writeln!(
            f,
            "orientation   {}",
            if self.rotated {
                "portrait (scene rotated 90 degrees onto the sensor)"
            } else {
                "landscape"
            }
        )?;
        writeln!(
            f,
            "sensor        {} x {} photosites",
            self.sensor_width, self.sensor_height
        )?;
        writeln!(
            f,
            "line          {} optical black + {} active + {} blanking = {} pixel periods",
            self.ob_cols,
            self.cols,
            self.hblank_px,
            self.line_len()
        )?;
        writeln!(
            f,
            "frame         {} rows read (line skip {}) + {} blanking = {} line periods",
            self.rows_read,
            self.line_skip,
            self.vblank_lines,
            self.lines_per_frame()
        )?;
        writeln!(
            f,
            "pixel clock   {:.6} MHz ({:.3} ns per pixel, native {:.3} ns)",
            self.pixel_clock_hz * 1e-6,
            self.pixel_period_s * 1e9,
            self.native_pixel_period_s * 1e9
        )?;
        if self.dsp_pixel_period_s.to_bits() != self.pixel_period_s.to_bits() {
            writeln!(
                f,
                "processor     {:.6} MHz ({:.3} ns per pixel); the sensor runs at {:.6} of it",
                1e-6 / self.dsp_pixel_period_s,
                self.dsp_pixel_period_s * 1e9,
                self.clock_ratio
            )?;
        }
        writeln!(
            f,
            "active line   {:.3} µs",
            f64::from(self.cols) * self.pixel_period_s * 1e6
        )?;
        writeln!(f, "line period   {:.3} µs", self.line_period_s * 1e6)?;
        writeln!(
            f,
            "frame period  {:.3} ms ({:.2} frames per second)",
            self.frame_period_s * 1e3,
            1.0 / self.frame_period_s
        )?;
        writeln!(
            f,
            "oversample    {} sub-steps per pixel ({:.3} ns each)",
            self.oversample,
            self.pixel_period_s * 1e9 / f64::from(self.oversample)
        )?;
        write!(
            f,
            "output        {} x {} px",
            self.output_width, self.output_height
        )
    }
}
