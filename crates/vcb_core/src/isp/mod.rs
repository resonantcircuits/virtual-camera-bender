//! Image processing: from ADC codes to a developed 8-bit frame.
//!
//! The image processor (Canon's DIGIC in the A520) receives the ADC words line by
//! line, but reads only `dsp_bits` bus lines starting at `dsp_lsb` (on the A520, lines
//! `D11` to `D2` of the 12-bit bus, a 10-bit raw). [`clamp_black`] takes that view and
//! removes the black level line by line, as the DSP's clamp loop does within its
//! correction range (`[isp] clamp_range`), leaving a [`Raw`] Bayer plane of the active
//! area in processor codes above black, rows in image order. [`develop`] then turns it
//! into a [`Frame`]:
//!
//! 1. Normalisation: codes are divided by `white_code`, the code above black that
//!    reference white produces at the sensor's nominal exposure, so linear 1.0 is
//!    reference white again; a longer exposure develops brighter.
//! 2. White balance, on the mosaic: the red and blue sites are multiplied by the
//!    daylight gains, by grey-world gains that equalise the channel means
//!    ([`Wb::Auto`]), or by the gains of the white-balance servo ([`Wb::Servo`]). Gains
//!    are scaled so the smallest is 1, as dcraw does, and every site is clipped where
//!    the least-amplified channel saturates (full well or ADC, whichever comes first),
//!    so clipped highlights stay white instead of turning pink.
//! 3. Demosaic: bilinear interpolation, or Malvar, He and Cutler's gradient-corrected
//!    linear interpolation ([`Demosaic`]); borders are mirrored with the Bayer phase kept.
//! 4. Colour correction: `cam_to_srgb`, from white-balanced camera RGB to linear sRGB.
//! 5. Geometry: movie mode averages `h_bin` adjacent columns, and if the result
//!    differs from the output size it is resampled to it, both in linear light.
//! 6. Tone and encoding: the [`Tone`] curve in linear light, then the sRGB transfer
//!    curve, through one lookup table.
//! 7. Noise reduction and sharpening on BT.601 `Y'CbCr` of the encoded values, as camera
//!    DSPs do: Cb and Cr are blurred with a Gaussian of `chroma_nr_frac` of the frame
//!    width; luma is blurred with `luma_nr_frac` of the width and blended back in
//!    proportion to local flatness, `max(0, 1 - |Y - blur(Y)| / 8)` with the detail
//!    in 8-bit codes, so grain is smoothed and edges are kept; then an unsharp mask of
//!    standard deviation `sharpen.radius_frac` of the width adds `sharpen.gain` times
//!    the detail to luma, with no halo suppression: overshoots clip at 0 and 255.
//! 8. Output: back to RGB, rounded to 8 bits.
//!
//! Defective-pixel concealment is not modelled: DIGIC's defect map and its
//! interpolation rule for the A520 are undocumented, so hot photosites pass through
//! to the demosaic, where they become small coloured crosses.
//!
//! # Servos
//!
//! DIGIC meters exposure and white balance on the data it receives, so it sees the
//! bent raw. [`meter`] does the same after each frame: with `ae_mode = "servo"` it
//! measures the developed raw's mean luminance (the channel means through the white
//! balance and colour matrix of that frame) and moves the exposure scale for the next
//! frame towards `scale * ae_target / luminance`; with `wb = "servo"` it moves the red
//! and blue gains towards the frame's grey-world gains. Both approach their goal in
//! stops, exponentially with the time constant `ae_tau_s` or `wb_tau_s` in camera
//! time: each frame corrects `1 - e^(-frame_period_s / tau_s)` of the remaining error.
//! The exposure stays within two stops of the set exposure, as scene metering does.
//! After a reset the first frame is exposed as scene metering sets it and balanced
//! with the daylight gains, so a still with servos matches one with `ae_mode =
//! "scene"` and daylight white balance.
//!
//! Every pixel-parallel pass runs over rows on the rayon pool; working planes live in
//! [`State`] and are allocated only when the frame size changes.

mod demosaic;
mod filter;
mod tone;
pub(crate) mod ycc;

use std::fmt::{self, Debug, Formatter};

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::{ParallelSlice, ParallelSliceMut};

use crate::adc::Codes;
use crate::plane::Plane;
use crate::power::Rail;
use crate::preset::Mode;
use crate::resample::{self, Rect, Turn};
use crate::sensor::{self, Cfa};
use crate::validate::positive;
use crate::{Camera, Error, Frame, Preset, Scene, Timing, colour};

#[doc(inline)]
pub use tone::Tone;

/// Time constant of the black-clamp loop in lines.
///
/// Clamp loops of CCD signal processors (AD9949 class, research section 5) correct
/// the black level from a 2 to 20 pixel optical-black window on every line and
/// integrate the error over a few lines, so a disturbed line pulls the level without
/// resetting it. Four lines is a typical loop setting.
const CLAMP_TAU_LINES: f64 = 4.0;

/// Local luma detail in 8-bit codes at which luma noise reduction stops.
///
/// About five times the A520's ISO 50 noise of 1.5 codes (dpreview, research
/// section 9): grain-sized differences are smoothed, edges are left alone.
const LUMA_NR_EDGE_CODES: f32 = 8.0;

/// Which white balance the processor applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum WbMode {
    /// Fixed daylight gains.
    #[default]
    Daylight,
    /// Gains measured from the frame (grey world).
    Auto,
    /// Gains that follow the grey-world gains of earlier frames with a lag.
    ///
    /// The first frame after a reset uses the daylight gains.
    Servo,
}

/// How the camera sets its exposure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum AeMode {
    /// The exposure stays as configured.
    #[default]
    Fixed,
    /// Every frame is exposed so that the scene's mean luminance lands on `ae_target`.
    Scene,
    /// The exposure follows the metering of earlier frames' raw data with a lag.
    ///
    /// The first frame after a reset is exposed as [`Scene`](Self::Scene) sets it.
    Servo,
}

/// A lagging control loop that moves a setting towards its metered goal once per frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Servo {
    /// Fraction of the remaining error, in stops, that one frame corrects.
    ///
    /// `1 - e^(-frame_period_s / tau_s)`, so the setting covers 63 % of a step after
    /// `tau_s` of camera time.
    pub step: f32,
}

impl Servo {
    /// Returns the servo of time constant `tau_s` for a frame period of `frame_period_s`.
    #[must_use]
    pub fn new(tau_s: f64, frame_period_s: f64) -> Self {
        Self {
            step: (1.0 - (-frame_period_s / tau_s).exp()) as f32,
        }
    }

    /// Returns `value` moved towards `goal` by one frame's step, in stops.
    #[must_use]
    pub fn toward(self, value: f32, goal: f32) -> f32 {
        value * (goal / value).powf(self.step)
    }
}

/// White balance with its resolved gains.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wb {
    /// Fixed gains of the red and blue channels relative to green.
    Daylight {
        /// Red gain.
        r_gain: f32,
        /// Blue gain.
        b_gain: f32,
    },
    /// Gains that make the mean of every channel equal to the green mean.
    Auto,
    /// Gains that follow grey world through a servo, from the daylight gains after a reset.
    Servo {
        /// Red gain of the first frame.
        r_gain: f32,
        /// Blue gain of the first frame.
        b_gain: f32,
        /// The servo that moves the gains.
        servo: Servo,
    },
}

/// Unsharp masking of luminance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sharpen {
    /// Standard deviation of the blur as a fraction of the frame width.
    pub radius_frac: f32,
    /// Fraction of the high-pass detail added back.
    pub gain: f32,
}

/// Demosaicing algorithm.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Demosaic {
    /// Bilinear interpolation of each channel from its nearest sites.
    Bilinear,
    /// Malvar, He and Cutler (2004): bilinear plus a 5 by 5 gradient correction.
    #[default]
    Malvar,
}

/// Size of the developed frame on the landscape sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Output {
    /// Adjacent columns averaged into one, 1 for stills.
    pub h_bin: u32,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
}

/// The active-area Bayer plane after the black clamp, in codes above black.
#[derive(Clone, PartialEq)]
pub struct Raw {
    plane: Plane<f32>,
    black: Box<[f32]>,
    /// The image row where the processor stopped capturing, if it did.
    dropped: Option<u32>,
}

impl Raw {
    /// Creates a zero raw plane of `width` by `height` sites.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            plane: Plane::new(width, height),
            black: vec![0.0; height as usize].into_boxed_slice(),
            dropped: None,
        }
    }

    /// Returns the first image row the processor did not capture, if a brownout stopped it.
    ///
    /// The row is numbered in image order, as the plane's rows are: the image row of
    /// the first readout line the processor dropped. Every line read after it is lost
    /// too: with a top-down readout the rows from it to the bottom, with a bottom-up
    /// one the rows from it to the top. Their sites are zero, until the frame buffer's
    /// [`memory::bend`](crate::memory::bend), with `[bend.memory]`, fills them with what
    /// the SDRAM held there. `None` when every row was captured; [`clamp_black`] sets it
    /// afresh for every frame.
    #[must_use]
    pub fn dropped_from(&self) -> Option<u32> {
        self.dropped
    }

    /// Returns the sites in processor codes above black, rows in image order, red at `(0, 0)`.
    #[must_use]
    pub fn plane(&self) -> &Plane<f32> {
        &self.plane
    }

    /// Returns the sites mutably.
    pub fn plane_mut(&mut self) -> &mut Plane<f32> {
        &mut self.plane
    }

    /// Returns the black level subtracted from each image row, in processor codes.
    #[must_use]
    pub fn black_levels(&self) -> &[f32] {
        &self.black
    }

    /// Returns the mean black level the clamp subtracted, in processor codes.
    #[must_use]
    pub fn black_level(&self) -> f32 {
        if self.black.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.black.iter().map(|&b| f64::from(b)).sum();
        (sum / self.black.len() as f64) as f32
    }
}

impl Debug for Raw {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Raw")
            .field("width", &self.plane.width())
            .field("height", &self.plane.height())
            .field("black_level", &self.black_level())
            .finish_non_exhaustive()
    }
}

/// Parameters of the processing pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Processor code above black that reference white produces; divides every site.
    pub white_code: f32,
    /// Processor code above black at which a photosite's well is full.
    pub full_well_code: f32,
    /// Lowest bus line the processor reads.
    pub dsp_lsb: u8,
    /// Number of bus lines the processor reads.
    pub dsp_bits: u8,
    /// Supply voltage at the start of a line below which the processor captures nothing more of the frame.
    pub dsp_brownout_v: f64,
    /// Nominal black level in processor codes, the centre of the clamp's range.
    ///
    /// `[adc] black_level >> dsp_lsb`, the word the processor reads for the nominal black.
    pub black_code: f64,
    /// Correction range of the black clamp either side of `black_code`, in processor codes.
    pub clamp_range: f64,
    /// Colour filter array of the raw plane.
    pub cfa: Cfa,
    /// White balance.
    pub wb: Wb,
    /// Matrix from white-balanced camera RGB to linear sRGB, row major.
    pub cam_to_srgb: [[f32; 3]; 3],
    /// Tone curve.
    pub tone: Tone,
    /// Chroma noise reduction blur, standard deviation as a fraction of the frame width.
    pub chroma_nr_frac: f32,
    /// Luma noise reduction blur, standard deviation as a fraction of the frame width.
    pub luma_nr_frac: f32,
    /// Unsharp masking.
    pub sharpen: Sharpen,
    /// Demosaicing algorithm.
    pub demosaic: Demosaic,
    /// Auto-exposure target luminance; the sensor's exposure control meters towards it.
    pub ae_target: f32,
    /// The auto-exposure servo, when `ae_mode` is `"servo"`.
    pub ae_servo: Option<Servo>,
    /// Size of the developed frame on the landscape sensor.
    pub output: Output,
}

impl Params {
    /// Resolves the processing parameters from the camera, preset and derived timing.
    ///
    /// `white_code` follows from the analog chain: at the sensor's nominal exposure
    /// reference white fills `white_fraction` of the full well, which the conversion
    /// gain, PGA gain and ADC scale map to a code above the pedestal, divided by
    /// `2^dsp_lsb` for the processor's view of the bus; `full_well_code` is the same
    /// for a full well.
    ///
    /// `cam_to_srgb` is derived from the `xyz_to_cam` matrix as dcraw's `cam_xyz_coeff`
    /// does: `xyz_to_cam * srgb_to_xyz` (D65) gives sRGB to camera RGB; each row is
    /// normalised to sum to one, so white maps to white and the white balance stays a
    /// separate gain; the inverse of that matrix is `cam_to_srgb`.
    ///
    /// The tone curve's white point is the linear level at which the sensor or the
    /// ADC saturates, whichever comes first. Preset overrides replace camera values.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if the analog chain maps reference white to no code
    /// or the colour matrix is not invertible.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let sensor = &camera.sensor;
        let white_volts = sensor.white_fraction
            * sensor.full_well_e
            * sensor.conv_gain_uv_per_e
            * 1e-6
            * 10f64.powf(camera.afe.pga_gain_db / 20.0);
        let white_code = white_volts / camera.adc.full_scale_v * f64::from(camera.max_code())
            / f64::from(1u32 << camera.adc.dsp_lsb);
        if !(white_code.is_finite() && white_code >= 1.0) {
            return Err(Error::config(format!(
                "reference white reaches only {white_code:.3} ADC codes above black"
            )));
        }
        let full_well_code = white_code / sensor.white_fraction;
        let nominal_black = f64::from(camera.adc.black_level >> camera.adc.dsp_lsb);
        let adc_room = f64::from(camera.dsp_max_code()) - nominal_black;
        let white_point = full_well_code.min(adc_room) / white_code;

        let isp = &camera.isp;
        let o = preset.isp.clone().unwrap_or_default();
        let pick = |value: Option<f64>, default: f64| value.unwrap_or(default) as f32;
        let (r_gain, b_gain) = (
            pick(o.wb_r_gain, isp.wb_r_gain),
            pick(o.wb_b_gain, isp.wb_b_gain),
        );
        let wb = match o.wb.unwrap_or_default() {
            WbMode::Daylight => Wb::Daylight { r_gain, b_gain },
            WbMode::Auto => Wb::Auto,
            WbMode::Servo => {
                let tau_s = o.wb_tau_s.unwrap_or(isp.wb_tau_s);
                positive("isp.wb_tau_s", tau_s)?;
                Wb::Servo {
                    r_gain,
                    b_gain,
                    servo: Servo::new(tau_s, timing.frame_period_s),
                }
            }
        };
        let ae_servo = match o.ae_mode.unwrap_or(isp.ae_mode) {
            AeMode::Fixed | AeMode::Scene => None,
            AeMode::Servo => {
                let tau_s = o.ae_tau_s.unwrap_or(isp.ae_tau_s);
                positive("isp.ae_tau_s", tau_s)?;
                Some(Servo::new(tau_s, timing.frame_period_s))
            }
        };
        let cam_to_srgb = colour::cam_to_srgb(&o.xyz_to_cam.unwrap_or(isp.xyz_to_cam))?;
        let h_bin = match timing.mode {
            Mode::Still => 1,
            Mode::Movie => camera.movie.h_bin,
        };
        let (width, height) = timing.developed_size();
        Ok(Self {
            white_code: white_code as f32,
            full_well_code: full_well_code as f32,
            dsp_lsb: camera.adc.dsp_lsb,
            dsp_bits: camera.adc.dsp_bits,
            dsp_brownout_v: camera.power.dsp_brownout_v,
            black_code: nominal_black,
            clamp_range: isp.clamp_range,
            cfa: sensor.cfa,
            wb,
            cam_to_srgb: colour::to_f32(&cam_to_srgb),
            tone: Tone {
                contrast: pick(o.tone_contrast, isp.tone_contrast),
                toe: pick(o.tone_toe, isp.tone_toe),
                shoulder: pick(o.tone_shoulder, isp.tone_shoulder),
                white: white_point.max(1.0) as f32,
            },
            chroma_nr_frac: pick(o.chroma_nr_frac, isp.chroma_nr_frac),
            luma_nr_frac: pick(o.luma_nr_frac, isp.luma_nr_frac),
            sharpen: Sharpen {
                radius_frac: pick(o.sharpen_radius_frac, isp.sharpen_radius_frac),
                gain: pick(o.sharpen_gain, isp.sharpen_gain),
            },
            demosaic: isp.demosaic,
            ae_target: pick(o.ae_target, isp.ae_target),
            ae_servo,
            output: Output {
                h_bin,
                width,
                height,
            },
        })
    }
}

/// Persistent state of the image processor: the servos and working planes reused across frames.
///
/// The default is the state after a reset: no servo has metered a frame yet.
#[derive(Debug, Clone)]
pub struct State {
    /// Exposure scale the auto-exposure servo set for the next frame.
    exposure: Option<f32>,
    /// Red and blue gains the white-balance servo set for the next frame.
    gains: Option<[f32; 2]>,
    balanced: Plane<f32>,
    rgb: Scene,
    binned: Scene,
    sized: Scene,
    /// Luma, blue-difference and red-difference planes, and two blur scratch planes.
    ycc: [Plane<f32>; 5],
    lut: Vec<f32>,
    kernel: Vec<f32>,
}

impl State {
    /// Returns the exposure scale the auto-exposure servo set for the next frame.
    ///
    /// `None` until the servo has metered a frame, and whenever `ae_mode` is not `"servo"`.
    #[must_use]
    pub fn servo_exposure(&self) -> Option<f32> {
        self.exposure
    }

    /// Returns the red and blue gains the white-balance servo set for the next frame.
    ///
    /// `None` until the servo has metered a frame, and whenever `wb` is not `"servo"`.
    #[must_use]
    pub fn servo_gains(&self) -> Option<[f32; 2]> {
        self.gains
    }

    /// Forgets the value of every servo that `params` does not run.
    pub(crate) fn forget_stopped_servos(&mut self, params: &Params) {
        if params.ae_servo.is_none() {
            self.exposure = None;
        }
        if !matches!(params.wb, Wb::Servo { .. }) {
            self.gains = None;
        }
    }
}

/// Returns whether `new` turns off a servo that `old` runs.
pub(crate) fn stops_servo(old: &Params, new: &Params) -> bool {
    let wb_servo = |params: &Params| matches!(params.wb, Wb::Servo { .. });
    (old.ae_servo.is_some() && new.ae_servo.is_none()) || (wb_servo(old) && !wb_servo(new))
}

impl Default for State {
    fn default() -> Self {
        Self {
            exposure: None,
            gains: None,
            balanced: Plane::new(0, 0),
            rgb: Scene::new(0, 0),
            binned: Scene::new(0, 0),
            sized: Scene::new(0, 0),
            ycc: std::array::from_fn(|_| Plane::new(0, 0)),
            lut: Vec::new(),
            kernel: Vec::new(),
        }
    }
}

/// Reads `codes` as the processor sees the bus and subtracts the black level line by line.
///
/// Each word becomes `(code >> dsp_lsb) & (2^dsp_bits - 1)`. The clamp measures the
/// mean of every line's optical-black samples and follows it with a one-pole loop of
/// four lines' time constant, `b[n] = b[n-1] + (1 - e^(-1/4)) (ob[n] - b[n-1])`, in
/// readout order, starting from the first line's measurement. The loop corrects only
/// within its range: each estimate is held within [`Params::black_code`] `±`
/// [`Params::clamp_range`], so a line whose optical black carries a picture word, as
/// from an AFE locked out on a held word, keeps that word's colour instead of
/// developing black, while ordinary drift of a few codes is followed. Each line's
/// level is subtracted from its active samples, which are written into `out` with rows
/// in image order; [`Raw::black_levels`] records the levels.
///
/// `rail` is the supply rail of every readout line from
/// [`power::track`](crate::power::track), which powers the processor's capture. A
/// line that starts with the rail below [`Params::dsp_brownout_v`] browns the
/// processor out: it captures nothing more of the frame, so that line and every line
/// read after it become zero rows of `out`, their black levels hold the clamp's last
/// value, and [`Raw::dropped_from`] names the first such row. The next frame is
/// captured afresh.
///
/// # Panics
///
/// Panics if `out` is not `cols` by `rows` of the code layout, or `rail` has another
/// number of lines.
pub fn clamp_black(params: &Params, codes: &Codes, rail: &Rail, out: &mut Raw) {
    let layout = codes.layout();
    assert!(
        out.plane.width() == layout.cols() && out.plane.height() == layout.rows(),
        "raw plane is {} x {} but the readout is {} x {}",
        out.plane.width(),
        out.plane.height(),
        layout.cols(),
        layout.rows()
    );
    assert_eq!(
        rail.lines(),
        layout.rows(),
        "the rail has {} lines but the readout {}",
        rail.lines(),
        layout.rows()
    );
    // Lines from the first that starts in a brownout on are never captured.
    let captured = (0..layout.rows())
        .find(|&line| rail.start_v(line) < params.dsp_brownout_v)
        .unwrap_or(layout.rows());
    out.dropped = (captured < layout.rows()).then(|| layout.image_row(captured));
    let line_len = layout.line_len() as usize;
    let ob = layout.ob_cols() as usize;
    let shift = u32::from(params.dsp_lsb);
    let mask = ((1u32 << params.dsp_bits.min(16)) - 1) as u16;
    let view = |code: u16| (code >> shift) & mask;
    if line_len == 0 {
        return;
    }

    // Measure every captured line's optical black, stored by image row.
    let rows = layout.rows();
    for (line, words) in (0..captured).zip(codes.words().chunks_exact(line_len)) {
        let sum: u64 = words[..ob].iter().map(|&code| u64::from(view(code))).sum();
        let mean = if ob == 0 { 0.0 } else { sum as f64 / ob as f64 };
        out.black[layout.image_row(line) as usize] = mean as f32;
    }
    // The clamp loop runs in readout order, its estimate held within its range.
    let alpha = 1.0 - (-1.0 / CLAMP_TAU_LINES).exp();
    let (low, high) = (
        params.black_code - params.clamp_range,
        params.black_code + params.clamp_range,
    );
    let mut level = None;
    for line in 0..captured {
        let slot = &mut out.black[layout.image_row(line) as usize];
        let measured = f64::from(*slot);
        let next = level
            .map_or(measured, |b: f64| b + alpha * (measured - b))
            .clamp(low, high);
        level = Some(next);
        *slot = next as f32;
    }
    let held = level.unwrap_or(0.0) as f32;
    for line in captured..rows {
        out.black[layout.image_row(line) as usize] = held;
    }

    let width = out.plane.width() as usize;
    if width == 0 {
        return;
    }
    let black = &out.black;
    out.plane
        .data_mut()
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(row, sites)| {
            // The readout runs forwards or backwards, so the map from lines to rows is
            // its own inverse.
            let line = layout.image_row(row as u32);
            if line >= captured {
                sites.fill(0.0);
                return;
            }
            let line = line as usize;
            let words = &codes.words()[line * line_len + ob..(line + 1) * line_len];
            let level = black[row];
            for (site, &code) in sites.iter_mut().zip(words) {
                *site = f32::from(view(code)) - level;
            }
        });
}

/// Develops the raw plane into the 8-bit frame `out`.
///
/// # Panics
///
/// Panics if `out` is not the output size in `params`, or if the raw plane is
/// smaller than 2 by 2 sites.
pub fn develop(params: &Params, state: &mut State, raw: &Raw, out: &mut Frame) {
    let output = params.output;
    assert!(
        out.width() == output.width && out.height() == output.height,
        "frame is {} x {} but the output is {} x {}",
        out.width(),
        out.height(),
        output.width,
        output.height
    );
    let (width, height) = (raw.plane.width(), raw.plane.height());
    assert!(
        width >= 2 && height >= 2,
        "cannot demosaic a {width} x {height} raw plane"
    );
    let gains = frame_gains(params, state, raw).map(|gain| gain / params.white_code);
    let State {
        balanced,
        rgb,
        binned,
        sized,
        ycc,
        lut,
        kernel,
        ..
    } = state;

    let max_code = ((1u32 << params.dsp_bits.min(16)) - 1) as f32;
    let saturated = params.full_well_code.min(max_code - raw.black_level());
    let clip = saturated / params.white_code;
    fit_plane(balanced, width, height);
    white_balance(&raw.plane, params.cfa, gains, clip, balanced);
    fit(rgb, width, height);
    match params.demosaic {
        Demosaic::Bilinear => demosaic::bilinear(balanced, params.cfa, clip, rgb),
        Demosaic::Malvar => demosaic::malvar(balanced, params.cfa, clip, rgb),
    }
    correct_colour(&params.cam_to_srgb, rgb);

    let h_bin = output.h_bin.max(1);
    let binned: &Scene = if h_bin > 1 {
        fit(binned, width / h_bin, height);
        for (src, dst) in rgb.planes().into_iter().zip(binned.planes_mut()) {
            resample::bin_columns(src, h_bin, dst);
        }
        binned
    } else {
        rgb
    };
    let sized: &Scene = if (binned.width(), binned.height()) == (output.width, output.height) {
        binned
    } else {
        fit(sized, output.width, output.height);
        let whole = Rect {
            x: 0,
            y: 0,
            width: binned.width(),
            height: binned.height(),
        };
        for (src, dst) in binned.planes().into_iter().zip(sized.planes_mut()) {
            resample::resample(src, Turn::None, whole, dst);
        }
        sized
    };

    params.tone.fill_lut(lut);
    let frame_width = output.width as f32;
    let chroma_sigma = params.chroma_nr_frac * frame_width;
    let luma_sigma = params.luma_nr_frac * frame_width;
    let sharpen_sigma = params.sharpen.radius_frac * frame_width;
    let sharpen = params.sharpen.gain > 0.0 && sharpen_sigma > 0.0;
    if chroma_sigma <= 0.0 && luma_sigma <= 0.0 && !sharpen {
        encode_rgb(sized, &params.tone, lut, out);
        return;
    }

    for plane in ycc.iter_mut() {
        fit_plane(plane, output.width, output.height);
    }
    let [y, cb, cr, t1, t2] = ycc;
    to_ycc(sized, &params.tone, lut, [y, cb, cr]);
    if chroma_sigma > 0.0 {
        filter::gaussian_kernel(chroma_sigma, kernel);
        filter::blur(cb, t1, kernel);
        filter::blur(cr, t1, kernel);
    }
    if luma_sigma > 0.0 {
        filter::gaussian_kernel(luma_sigma, kernel);
        filter::blur_into(y, t1, t2, kernel);
        flatness_blend(y, t2);
    }
    if sharpen {
        filter::gaussian_kernel(sharpen_sigma, kernel);
        filter::blur_into(y, t1, t2, kernel);
        unsharp(y, t2, params.sharpen.gain);
    }
    from_ycc([y, cb, cr], out);
}

/// Meters the developed raw for the servos and sets their values for the next frame.
///
/// `exposure_scale` is the scale the sensor applied to this frame. With an
/// auto-exposure servo the next frame's scale moves from it towards the scale that
/// would put the mean luminance on `ae_target`; with a white-balance servo the next
/// frame's gains move from this frame's towards its grey-world gains. A servo that is
/// off forgets its value, so turning it on starts afresh. A black frame opens the
/// exposure towards its two-stop limit; a colour channel without signal keeps its gain.
pub fn meter(params: &Params, state: &mut State, raw: &Raw, exposure_scale: f32) {
    if params.ae_servo.is_none() && !matches!(params.wb, Wb::Servo { .. }) {
        state.exposure = None;
        state.gains = None;
        return;
    }
    let means = channel_means(&raw.plane, params.cfa);
    state.exposure = params.ae_servo.map(|servo| {
        let gains = frame_gains(params, state, raw);
        let luminance: f64 = (0..3)
            .map(|c| {
                let weight: f32 = (0..3)
                    .map(|k| sensor::LUMINANCE[k] as f32 * params.cam_to_srgb[k][c])
                    .sum();
                f64::from(weight * gains[c]) * means[c]
            })
            .sum::<f64>()
            / f64::from(params.white_code);
        let goal = if luminance > 0.0 {
            (f64::from(exposure_scale) * f64::from(params.ae_target) / luminance) as f32
        } else {
            sensor::AE_MAX_SCALE
        };
        let goal = goal.clamp(sensor::AE_MIN_SCALE, sensor::AE_MAX_SCALE);
        servo
            .toward(exposure_scale, goal)
            .clamp(sensor::AE_MIN_SCALE, sensor::AE_MAX_SCALE)
    });
    state.gains = match params.wb {
        Wb::Servo {
            r_gain,
            b_gain,
            servo,
        } => {
            let [red, blue] = state.gains.unwrap_or([r_gain, b_gain]);
            let goal = |mean: f64, gain: f32| {
                if mean > 0.0 && means[1] > 0.0 {
                    servo.toward(gain, (means[1] / mean) as f32)
                } else {
                    gain
                }
            };
            Some([goal(means[0], red), goal(means[2], blue)])
        }
        Wb::Daylight { .. } | Wb::Auto => None,
    };
}

/// Returns the white-balance gains of this frame, scaled so the smallest is 1.
fn frame_gains(params: &Params, state: &State, raw: &Raw) -> [f32; 3] {
    let balance = match params.wb {
        Wb::Daylight { r_gain, b_gain } => [r_gain, 1.0, b_gain],
        Wb::Auto => grey_world(&raw.plane, params.cfa),
        Wb::Servo { r_gain, b_gain, .. } => {
            let [red, blue] = state.gains.unwrap_or([r_gain, b_gain]);
            [red, 1.0, blue]
        }
    };
    let least = balance.iter().copied().fold(f32::MAX, f32::min);
    balance.map(|gain| gain / least)
}

/// Reallocates `scene` if it is not `width` by `height`.
fn fit(scene: &mut Scene, width: u32, height: u32) {
    if scene.width() != width || scene.height() != height {
        *scene = Scene::new(width, height);
    }
}

/// Reallocates `plane` if it is not `width` by `height`.
fn fit_plane(plane: &mut Plane<f32>, width: u32, height: u32) {
    if plane.width() != width || plane.height() != height {
        *plane = Plane::new(width, height);
    }
}

/// Writes every site of `raw` times its channel's gain, clipped at `clip`, into `out`.
fn white_balance(raw: &Plane<f32>, cfa: Cfa, gains: [f32; 3], clip: f32, out: &mut Plane<f32>) {
    let width = raw.width() as usize;
    out.data_mut()
        .par_chunks_mut(width)
        .zip(raw.data().par_chunks(width))
        .enumerate()
        .for_each(|(y, (sites, codes))| {
            let y = y as u32;
            let row_gains = [cfa.channel(0, y), cfa.channel(1, y)].map(|c| gains[c.index()]);
            for (x, (site, &code)) in sites.iter_mut().zip(codes).enumerate() {
                *site = (code * row_gains[x % 2]).min(clip);
            }
        });
}

/// Returns the mean of every channel's sites, in codes above black.
fn channel_means(raw: &Plane<f32>, cfa: Cfa) -> [f64; 3] {
    let mut sums = [0.0f64; 3];
    let mut counts = [0u64; 3];
    for (y, row) in (0..raw.height()).zip(raw.rows()) {
        for (x, &value) in (0..raw.width()).zip(row) {
            let channel = cfa.channel(x, y).index();
            sums[channel] += f64::from(value);
            counts[channel] += 1;
        }
    }
    std::array::from_fn(|c| sums[c] / counts[c].max(1) as f64)
}

/// Returns `[r, g, b]` gains that equalise the red and blue means with green.
fn grey_world(raw: &Plane<f32>, cfa: Cfa) -> [f32; 3] {
    let means = channel_means(raw, cfa);
    let gain = |mean: f64| {
        if mean > 0.0 && means[1] > 0.0 {
            (means[1] / mean) as f32
        } else {
            1.0
        }
    };
    [gain(means[0]), 1.0, gain(means[2])]
}

/// Multiplies every pixel by the colour matrix.
fn correct_colour(matrix: &[[f32; 3]; 3], image: &mut Scene) {
    let row_len = image.width().max(1) as usize;
    let [red, green, blue] = image.planes_mut();
    red.data_mut()
        .par_chunks_mut(row_len)
        .zip(green.data_mut().par_chunks_mut(row_len))
        .zip(blue.data_mut().par_chunks_mut(row_len))
        .for_each(|((red_row, green_row), blue_row)| {
            for ((r, g), b) in red_row.iter_mut().zip(green_row).zip(blue_row) {
                [*r, *g, *b] = colour::apply(matrix, [*r, *g, *b]);
            }
        });
}

/// Tone-maps and encodes a linear-light image straight into the 8-bit frame.
fn encode_rgb(image: &Scene, tone: &Tone, lut: &[f32], out: &mut Frame) {
    let width = out.width() as usize;
    if width == 0 {
        return;
    }
    let [red, green, blue] = image.planes();
    out.rgb_mut()
        .par_chunks_mut(width * 3)
        .enumerate()
        .for_each(|(y, bytes)| {
            let y = y as u32;
            let rows = [red.row(y), green.row(y), blue.row(y)];
            for (x, pixel) in bytes.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                for (byte, row) in pixel.iter_mut().zip(rows) {
                    *byte = to_byte(tone.lookup(lut, row[x]));
                }
            }
        });
}

/// Tone-maps and encodes a linear-light image into `Y'CbCr` planes on the 8-bit scale.
fn to_ycc(image: &Scene, tone: &Tone, lut: &[f32], [y, cb, cr]: [&mut Plane<f32>; 3]) {
    let width = image.width().max(1) as usize;
    let [red, green, blue] = image.planes();
    y.data_mut()
        .par_chunks_mut(width)
        .zip(cb.data_mut().par_chunks_mut(width))
        .zip(cr.data_mut().par_chunks_mut(width))
        .zip(red.data().par_chunks(width))
        .zip(green.data().par_chunks(width))
        .zip(blue.data().par_chunks(width))
        .for_each(|(((((ys, cbs), crs), rs), gs), bs)| {
            for i in 0..ys.len() {
                let rgb = [rs[i], gs[i], bs[i]].map(|v| tone.lookup(lut, v));
                [ys[i], cbs[i], crs[i]] = ycc::from_rgb(rgb);
            }
        });
}

/// Converts `Y'CbCr` planes on the 8-bit scale back to RGB bytes.
fn from_ycc([y, cb, cr]: [&mut Plane<f32>; 3], out: &mut Frame) {
    let width = out.width() as usize;
    if width == 0 {
        return;
    }
    out.rgb_mut()
        .par_chunks_mut(width * 3)
        .zip(y.data().par_chunks(width))
        .zip(cb.data().par_chunks(width))
        .zip(cr.data().par_chunks(width))
        .for_each(|(((bytes, ys), cbs), crs)| {
            for (i, pixel) in bytes.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let rgb = ycc::to_rgb([ys[i], cbs[i], crs[i]]);
                for (byte, value) in pixel.iter_mut().zip(rgb) {
                    *byte = to_byte(value);
                }
            }
        });
}

/// Blends `luma` towards `blurred` in proportion to the local flatness.
fn flatness_blend(luma: &mut Plane<f32>, blurred: &Plane<f32>) {
    let width = luma.width().max(1) as usize;
    luma.data_mut()
        .par_chunks_mut(width)
        .zip(blurred.data().par_chunks(width))
        .for_each(|(ys, bs)| {
            for (y, &b) in ys.iter_mut().zip(bs) {
                let flatness = (1.0 - (*y - b).abs() / LUMA_NR_EDGE_CODES).max(0.0);
                *y += flatness * (b - *y);
            }
        });
}

/// Adds `gain` times the detail above `blurred` to `luma`, clipping to the 8-bit range.
fn unsharp(luma: &mut Plane<f32>, blurred: &Plane<f32>, gain: f32) {
    let width = luma.width().max(1) as usize;
    luma.data_mut()
        .par_chunks_mut(width)
        .zip(blurred.data().par_chunks(width))
        .for_each(|(ys, bs)| {
            for (y, &b) in ys.iter_mut().zip(bs) {
                *y = gain.mul_add(*y - b, *y).clamp(0.0, 255.0);
            }
        });
}

/// Rounds a value on the 8-bit scale to a byte, clipping to `[0, 255]`.
fn to_byte(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}
