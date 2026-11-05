//! Sections of a camera model file: the hardware in physical units.
//!
//! A [`Camera`](crate::Camera) mirrors a TOML file such as `cameras/a520.toml`; each
//! struct here is one `[section]` of that file. Field names carry their unit as a
//! suffix (`_hz`, `_v`, `_e`, `_ohm`, `_px`); dimensionless ratios are named for what
//! they are (`cte`, `prnu`, `white_fraction`).
//!
//! The structs hold configuration exactly as written. The simulator validates it with
//! [`Camera::validate`](crate::Camera::validate) and resolves it, together with a
//! [`Preset`](crate::Preset) and the derived [`Timing`](crate::Timing), into the
//! `Params` of each stage.

pub(crate) mod model;

use crate::codec::{ChromaUp, Quality, QuantTables, Subsampling};
use crate::isp::{AeMode, Demosaic};
use crate::readout::Direction;
use crate::sensor::Cfa;

/// The `[camera]` section: which camera model the file describes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Info {
    /// Identifier that presets use to reference the camera, for example `"a520"`.
    pub name: String,
    /// Manufacturer, for example `"Canon"`.
    pub maker: String,
    /// Marketing name of the model.
    pub model: String,
    /// Year the camera was released.
    pub year: u16,
}

/// The `[sensor]` section: the CCD and its colour filter array.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Sensor {
    /// Active photosite columns of the full sensor.
    pub native_width: u32,
    /// Active photosite rows of the full sensor.
    pub native_height: u32,
    /// Optical-black (masked) columns read at the start of every line.
    pub ob_cols: u32,
    /// Photosite pitch in micrometres.
    pub pitch_um: f64,
    /// Full-well capacity of one photosite in electrons.
    pub full_well_e: f64,
    /// Fraction of the full well that reference white fills at the nominal exposure.
    ///
    /// Reference white is scene value 1.0, before auto-exposure scales the light.
    pub white_fraction: f64,
    /// Exposure time in seconds at which reference white fills `white_fraction` of the well.
    ///
    /// It calibrates the sensor's sensitivity to the scene: the charge every photosite
    /// collects scales with the exposure time over this one, so a longer exposure
    /// brightens the frame until the well or the ADC clips.
    pub nominal_exposure_s: f64,
    /// Output amplifier read noise in electrons, one standard deviation.
    pub read_noise_e: f64,
    /// Mean dark current in electrons per second per photosite.
    pub dark_e_per_s: f64,
    /// Photo-response non-uniformity: relative gain spread, one standard deviation.
    pub prnu: f64,
    /// Dark-signal non-uniformity in electrons, one standard deviation.
    pub dsnu_e: f64,
    /// Conversion gain of the output amplifier in microvolts per electron.
    pub conv_gain_uv_per_e: f64,
    /// Overexposure, in multiples of the full well, that the overflow drain absorbs before charge spills.
    pub antibloom_ratio: f64,
    /// Interline smear relative to the signal, in decibels (negative).
    pub smear_db: f64,
    /// Charge transfer efficiency per transfer.
    pub cte: f64,
    /// Density of hot photosites per megapixel.
    pub hot_pixels_per_mpix: f64,
    /// Layout of the colour filter array.
    pub cfa: Cfa,
    /// Seed of this unit's fixed pattern: its PRNU, DSNU, hot photosites and bus drivers.
    ///
    /// Two cameras of the same model differ only in this seed, which also draws the
    /// spread of the ADC's data-line driver resistance; the preset's seed, which drives
    /// the temporal noise, leaves the pattern unchanged.
    #[cfg_attr(feature = "serde", serde(default = "default_unit_seed"))]
    pub unit_seed: u64,
}

/// Returns the unit seed of a camera file that does not set one.
#[cfg(feature = "serde")]
fn default_unit_seed() -> u64 {
    1
}

/// The `[lens]` section: the optics in front of the sensor.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Lens {
    /// Illumination falloff at the image corner relative to the centre, in stops.
    ///
    /// The falloff follows the cos^4 law of natural vignetting; 0 is a perfectly even lens.
    pub vignette_stops: f64,
}

/// The `[still]` section: full-sensor readout behind the mechanical shutter.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Still {
    /// Horizontal pixel clock in hertz.
    pub pixel_clock_hz: f64,
    /// Horizontal blanking per line in pixel periods.
    pub hblank_px: u32,
    /// Vertical blanking per frame in line periods.
    pub vblank_lines: u32,
    /// Exposure (integration) time in seconds; a preset's `[sensor] exposure_s` replaces it.
    pub exposure_s: f64,
    /// Whether interline smear is present; false because the mechanical shutter closes during readout.
    pub smear: bool,
    /// JPEG quality setting of the still image.
    pub jpeg: Quality,
    /// Which sensor row is read first.
    pub direction: Direction,
}

/// The `[movie]` section: line-skipped readout with the electronic shutter.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Movie {
    /// Horizontal pixel clock in hertz.
    pub pixel_clock_hz: f64,
    /// Horizontal blanking per line in pixel periods.
    pub hblank_px: u32,
    /// Vertical blanking per frame in line periods.
    pub vblank_lines: u32,
    /// One row pair is read out of every `line_skip` row pairs.
    pub line_skip: u32,
    /// The image processor averages this many adjacent columns.
    pub h_bin: u32,
    /// Nominal frame rate in frames per second.
    pub fps: f64,
    /// Exposure (integration) time of the electronic shutter in seconds; at most `1 / fps`.
    ///
    /// A preset's `[sensor] exposure_s` replaces it, limited to one frame period.
    pub exposure_s: f64,
    /// Width of the movie frame in pixels at native resolution.
    pub output_width: u32,
    /// Height of the movie frame in pixels at native resolution.
    pub output_height: u32,
    /// JPEG quality setting of each movie frame.
    pub jpeg: Quality,
    /// Which sensor row is read first.
    pub direction: Direction,
    /// Whether interline smear is present; the electronic shutter leaves the CCD exposed during transfer.
    pub smear: bool,
}

/// The `[afe]` section: the analog front end between the CCD and the ADC.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Afe {
    /// Programmable gain amplifier gain in decibels.
    pub pga_gain_db: f64,
    /// Voltage at the ADC input for zero charge.
    pub pedestal_v: f64,
    /// Small-signal bandwidth of the analog chain in hertz.
    pub bandwidth_hz: f64,
    /// Maximum rate of change of the output in volts per microsecond.
    pub slew_v_per_us: f64,
    /// Read-noise corner frequency in hertz, where flicker noise equals the white floor.
    pub noise_1f_corner_hz: f64,
    /// Analog simulation sub-steps per pixel period.
    pub oversample: u32,
}

/// The `[adc]` section: the converter and its parallel output bus.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Adc {
    /// Resolution in bits; data lines are `D0` to `D(bits-1)`.
    pub bits: u8,
    /// Bus lines the image processor reads, starting at `dsp_lsb`.
    pub dsp_bits: u8,
    /// Lowest bus line the image processor reads; lines below it are unconnected.
    pub dsp_lsb: u8,
    /// Input voltage that maps to the largest code.
    pub full_scale_v: f64,
    /// Nominal code of the black pedestal in full-bus units; the AFE pedestal must map to it.
    pub black_level: u16,
    /// Output resistance of each data-line driver in ohms.
    pub driver_r_ohm: f64,
    /// Relative spread of the driver resistance between lines, one standard deviation.
    pub driver_r_spread: f64,
    /// Output-high voltage of the data-line drivers.
    pub v_oh: f64,
    /// Series resistor on each bus line between the ADC and the image processor in ohms.
    pub bus_series_ohm: f64,
    /// Capacitance of each bus line to ground in picofarads: pin, trace and processor input.
    pub line_pf: f64,
    /// Fastest rate at which a data-line driver's output moves, in volts per microsecond.
    ///
    /// A full swing, `v_oh / driver_slew_v_per_us`, must fit in a pixel period.
    pub driver_slew_v_per_us: f64,
    /// Logic threshold of the image processor's bus inputs in volts.
    pub dsp_threshold_v: f64,
    /// Thermal noise at the image processor's bus inputs in millivolts, one standard deviation.
    pub dsp_noise_mv: f64,
    /// Fraction of the pixel period at which the image processor latches the bus.
    pub latch_phase: f64,
}

/// The `[logic]` section: the 74HC00 and 74HC74 of the bend circuit.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Logic {
    /// Output resistance of a 74HC gate in ohms.
    pub hc_r_out_ohm: f64,
    /// Input switching threshold of a 74HC gate in volts.
    pub hc_threshold_v: f64,
    /// Input hysteresis of a 74HC gate in volts.
    pub hc_hysteresis_v: f64,
    /// Propagation delay of a 74HC00 gate in nanoseconds.
    pub hc_prop_delay_ns: f64,
    /// Highest clock frequency of the 74HC74 flip-flop in hertz.
    pub ff_fmax_hz: f64,
    /// Shortest clock pulse, high or low, the flip-flop responds to, in nanoseconds.
    pub ff_min_pulse_ns: f64,
    /// Setup time of the flip-flop's `D` input before the clock edge in nanoseconds.
    pub ff_setup_ns: f64,
    /// Clock-to-output propagation delay of the flip-flop in nanoseconds.
    pub ff_prop_delay_ns: f64,
    /// Forward voltage of a gate input's protection diodes, beyond ground or the supply.
    pub clamp_diode_v: f64,
    /// Series resistance of a conducting input protection diode in ohms.
    pub clamp_r_ohm: f64,
    /// Input leakage of a gate input as a resistance to half the supply, in megohms.
    pub input_leak_mohm: f64,
}

/// The `[isp]` section: the image processor's colour and detail pipeline.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Isp {
    /// Daylight white-balance gain of the red channel relative to green.
    pub wb_r_gain: f64,
    /// Daylight white-balance gain of the blue channel relative to green.
    pub wb_b_gain: f64,
    /// Colour matrix from CIE XYZ (D65) to camera RGB, row major, in dcraw's convention.
    pub xyz_to_cam: [[f64; 3]; 3],
    /// Slope of the tone curve at mid-grey.
    pub tone_contrast: f64,
    /// Linear level below which the tone curve compresses shadows towards black.
    pub tone_toe: f64,
    /// Fraction of the output range below white given to the highlight roll-off.
    pub tone_shoulder: f64,
    /// Chroma noise reduction blur: Gaussian standard deviation as a fraction of the frame width.
    pub chroma_nr_frac: f64,
    /// Luma noise reduction blur: Gaussian standard deviation as a fraction of the frame width.
    pub luma_nr_frac: f64,
    /// Unsharp-mask blur: Gaussian standard deviation as a fraction of the frame width.
    pub sharpen_radius_frac: f64,
    /// Unsharp-mask gain: the fraction of high-pass detail added back.
    pub sharpen_gain: f64,
    /// Auto-exposure target: mean linear luminance of the developed frame.
    pub ae_target: f64,
    /// Time constant of the auto-exposure servo in seconds of camera time.
    pub ae_tau_s: f64,
    /// Time constant of the auto-white-balance servo in seconds of camera time.
    pub wb_tau_s: f64,
    /// Demosaicing algorithm.
    #[cfg_attr(feature = "serde", serde(default))]
    pub demosaic: Demosaic,
    /// Auto-exposure mode: a fixed exposure, one metered from the scene, or a lagging servo.
    #[cfg_attr(feature = "serde", serde(default))]
    pub ae_mode: AeMode,
}

/// The `[jpeg]` section: the camera's JPEG encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Jpeg {
    /// Chroma subsampling of the encoder.
    pub subsampling: Subsampling,
    /// Chroma upsampling of the decoder that displays the file.
    pub chroma_upsample: ChromaUp,
    /// The camera's quantisation tables for each quality setting.
    pub tables: JpegTables,
}

/// The `[jpeg.tables]` section: quantisation tables measured from camera files.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct JpegTables {
    /// Tables of the superfine setting.
    pub superfine: QuantTables,
    /// Tables of the fine setting.
    pub fine: QuantTables,
    /// Tables of the normal setting.
    pub normal: QuantTables,
}

impl JpegTables {
    /// Returns the tables of one quality setting.
    #[must_use]
    pub fn get(&self, quality: Quality) -> &QuantTables {
        match quality {
            Quality::Superfine => &self.superfine,
            Quality::Fine => &self.fine,
            Quality::Normal => &self.normal,
        }
    }
}

/// The clocking of one readout mode, common to the still and movie sections.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clocking {
    /// Horizontal pixel clock in hertz.
    pub pixel_clock_hz: f64,
    /// Horizontal blanking per line in pixel periods.
    pub hblank_px: u32,
    /// Vertical blanking per frame in line periods.
    pub vblank_lines: u32,
    /// One row pair is read out of every `line_skip` row pairs; 1 for stills.
    pub line_skip: u32,
    /// Which sensor row is read first.
    pub direction: Direction,
}
