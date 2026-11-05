//! Derived readout clocks: the time and resolution model.

use std::fmt::{self, Display, Formatter};

use crate::preset::{Mode, Resolution};
use crate::validate::at_least;
use crate::{Camera, Error, Preset};

/// Fewest optical-black columns per line on a scaled-down virtual sensor.
///
/// The black clamp averages the optical-black samples of every line; below about
/// eight samples its estimate starts to follow the read noise.
const MIN_OB_COLS: u32 = 8;

/// The derived clocks and geometry of one readout mode at one sensor size.
///
/// A line consists of `ob_cols` optical-black samples, `cols` active samples and
/// `hblank_px` blanking periods; a frame of `rows_read` lines and `vblank_lines`
/// blanking lines. Hence
///
/// - `line_period_s = pixel_period_s * (ob_cols + cols + hblank_px)` and
/// - `frame_period_s = line_period_s * (rows_read + vblank_lines)`.
///
/// At [`Resolution::Native`] the sensor has the camera's size and the pixel clock is
/// the camera's. At [`Resolution::MatchInput`] the sensor takes the input's size,
/// rounded down to even, and the optical-black and blanking counts scale with it.
/// The pixel clock is then chosen so that `frame_period_s` equals the native one:
/// an oscillator of a given frequency spans the same fraction of a frame, and a
/// time constant the same fraction of the readout, at every resolution.
///
/// Construct a `Timing` with [`Timing::derive`]; the fields are public for reading.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Timing {
    /// Still or movie readout.
    pub mode: Mode,
    /// How the sensor size was chosen.
    pub resolution: Resolution,
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
    /// Horizontal pixel clock in hertz.
    pub pixel_clock_hz: f64,
    /// Duration of one pixel period in seconds.
    pub pixel_period_s: f64,
    /// Duration of one line, including optical black and blanking, in seconds.
    pub line_period_s: f64,
    /// Duration of one frame, including vertical blanking, in seconds.
    pub frame_period_s: f64,
    /// Analog simulation sub-steps per pixel period.
    pub oversample: u32,
    /// Width of the developed frame in pixels.
    pub output_width: u32,
    /// Height of the developed frame in pixels.
    pub output_height: u32,
}

impl Timing {
    /// Derives the readout timing for a camera, a preset and an input size.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if the camera's clocking is invalid, and an input
    /// error if the input is empty or too small to hold one Bayer quad per output pixel.
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
        let native_samples = f64::from(native_rows + clock.vblank_lines)
            * f64::from(native_ob + native_w + clock.hblank_px);
        let native_frame_s = native_samples / clock.pixel_clock_hz;

        let (sensor_width, sensor_height) = match resolution {
            Resolution::Native => (native_w, native_h),
            Resolution::MatchInput => (input_width & !1, input_height & !1),
        };
        let rows = rows_read(sensor_height, clock.line_skip);
        if sensor_width < 2 || rows < 2 || sensor_width / h_bin < 1 {
            return Err(Error::input(format!(
                "a {input_width} x {input_height} input is too small for {mode} readout \
                 with line skip {} and column binning {h_bin}",
                clock.line_skip
            )));
        }

        let (ob_cols, hblank_px, vblank_lines) = match resolution {
            Resolution::Native => (native_ob, clock.hblank_px, clock.vblank_lines),
            Resolution::MatchInput => {
                let sx = f64::from(sensor_width) / f64::from(native_w);
                let sy = f64::from(rows) / f64::from(native_rows);
                let ob = ((f64::from(native_ob) * sx).round() as u32)
                    .max(MIN_OB_COLS.min(native_ob))
                    .max(1);
                let hblank = (f64::from(clock.hblank_px) * sx).round() as u32;
                let vblank = (f64::from(clock.vblank_lines) * sy).round() as u32;
                (ob, hblank, vblank)
            }
        };

        let line_len = ob_cols + sensor_width + hblank_px;
        let lines = rows + vblank_lines;
        let pixel_period_s = match resolution {
            Resolution::Native => 1.0 / clock.pixel_clock_hz,
            Resolution::MatchInput => native_frame_s / (f64::from(lines) * f64::from(line_len)),
        };
        let line_period_s = pixel_period_s * f64::from(line_len);
        let frame_period_s = line_period_s * f64::from(lines);

        let (output_width, output_height) = match (mode, resolution) {
            (Mode::Still, _) => (sensor_width, rows),
            (Mode::Movie, Resolution::MatchInput) => (sensor_width / h_bin, rows),
            (Mode::Movie, Resolution::Native) => {
                (camera.movie.output_width, camera.movie.output_height)
            }
        };

        Ok(Self {
            mode,
            resolution,
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
            line_period_s,
            frame_period_s,
            oversample: camera.afe.oversample.max(1),
            output_width,
            output_height,
        })
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

    /// Returns how many line periods one cycle of an oscillator at `freq_hz` lasts.
    #[must_use]
    pub fn lines_per_cycle(&self, freq_hz: f64) -> f64 {
        1.0 / (freq_hz * self.line_period_s)
    }

    /// Returns how many pixel periods one cycle of an oscillator at `freq_hz` lasts.
    #[must_use]
    pub fn samples_per_cycle(&self, freq_hz: f64) -> f64 {
        1.0 / (freq_hz * self.pixel_period_s)
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
            "pixel clock   {:.6} MHz ({:.3} ns per pixel)",
            self.pixel_clock_hz * 1e-6,
            self.pixel_period_s * 1e9
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
