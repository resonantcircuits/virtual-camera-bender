//! Exposure: scene radiance integrated into photosite charge behind the colour filters.
//!
//! The sensor is an interline CCD with an RGGB Bayer colour filter array: photosite
//! `(0, 0)` is red, `(1, 0)` and `(0, 1)` are green, `(1, 1)` is blue, repeating every
//! two columns and rows. [`expose`] computes the electrons of every photosite in this
//! order (research section 3 for the numbers):
//!
//! 1. Exposure: `radiance[filter] * white_fraction * full_well_e * exposure_s /
//!    nominal_exposure_s * exposure_scale`, so at the nominal exposure reference white
//!    fills `white_fraction` of the well and a longer exposure collects
//!    proportionally more charge. `exposure_scale` is auto-exposure's correction on
//!    top of the set exposure: 1 unless auto-exposure meters the scene
//!    ([`Ae::Scene`]), when it is `target / luminance`, the luminance being the
//!    prepared scene's mean times `exposure_s / nominal_exposure_s`, clamped to two
//!    stops either way. The image processor's auto-exposure servo sets it instead once
//!    it has metered a frame ([`Params::exposure_scale`]). Auto-exposure therefore
//!    compensates a changed exposure time within its two stops, as the camera's
//!    aperture would; with a fixed exposure the time alone sets the brightness.
//! 2. Photo-response non-uniformity: a fixed gain `1 + prnu * N(0, 1)` per photosite.
//! 3. Dark signal: `dark_e_per_s * k * exposure_s`, times 50 to 500 (log-uniform) at
//!    the `hot_pixels_per_mpix` hot photosites, plus the output amplifier's glow, plus
//!    a fixed offset `dsnu_e * N(0, 1)`; `k` is the Arrhenius factor of the sensor's
//!    temperature and the glow falls off from the readout's first corner (below).
//!    PRNU, DSNU and hot photosites hash the photosite coordinates with the camera's
//!    `unit_seed`, so they are the same in every frame and every preset.
//! 4. Shot noise: Gaussian with variance equal to the electron count, drawn per
//!    photosite and frame from the preset seed and the frame index. The Gaussian
//!    stands in for the Poisson distribution; it is accurate above a few tens of
//!    electrons, and below that it is symmetric where Poisson is skewed, so deep
//!    shadows are slightly too dark on average after the clip at zero charge.
//! 5. Full well and anti-blooming: the vertical overflow drain under each photosite
//!    absorbs up to `antibloom_ratio` full wells of excess charge. Charge beyond the
//!    well plus the drain spills into the column, half up and half down; spilled
//!    charge fills the wells and drains it meets until it is absorbed or leaves the
//!    array. The visible charge is then clipped at the well. Well and drain are the
//!    camera's at the nominal substrate bias and move with it (below).
//! 6. Interline smear, when the readout uses the electronic shutter (movie mode):
//!    light leaking into the vertical register adds the same charge to every
//!    photosite of a column (below).
//!
//! # Temperature and substrate bias
//!
//! A preset's `[bend.sensor]` sets the sensor's temperature `T`, by default the
//! camera's `reference_temp_c`, `T_ref`, and its substrate bias `V`, by default the
//! camera's nominal `substrate_v`, `V_nom`. At the defaults every factor below is
//! exactly 1 and every term added is absent, so the charge is bit-identical to the
//! camera's without the section.
//!
//! - **Dark current.** Thermal generation in silicon follows the Arrhenius rule; near
//!   room temperature a CCD's dark current doubles every `dark_doubling_c` degrees
//!   (C: 6 to 8 °C). The factor `k = 2^((T - T_ref) / dark_doubling_c)` multiplies
//!   `dark_e_per_s`, and with it every hot photosite, whose multiple of the mean stays
//!   its own. The masked optical-black photosites share the temperature
//!   ([`readout::Params::dark_e`](crate::readout::Params::dark_e)), so the image
//!   processor's black clamp removes the mean dark: what shows is its shot noise as
//!   grain, the hot photosites as crosses after the demosaic, the glow and clipping.
//! - **Read noise.** The output amplifier's noise rises `read_noise_per_c` per degree,
//!   `1 + read_noise_per_c * (T - T_ref)`, at least 0 (PROVISIONAL). The analog front
//!   end adds the read noise, so this stage only reports the factor on its
//!   `read_noise_e`, [`Params::read_noise_scale`].
//! - **Amplifier glow.** The output amplifier sits at the corner of the array that the
//!   readout samples first: the first column of the first row read, row 0 for a
//!   top-down readout and the last row for a bottom-up one, in the sensor's landscape
//!   orientation; the optical-black columns between it and the image are ignored. Its
//!   glow adds the dark charge `amp_glow_e_per_s * (k - 1) * exposure_s * e^(-d / r)`,
//!   `d` the distance from that photosite and `r = amp_glow_radius_frac`, both as
//!   fractions of the width (PROVISIONAL). The dark current at the reference
//!   temperature, `dark_e_per_s` with its non-uniformity and hot photosites, is the
//!   sensor as characterised, the amplifier's share included; heating adds to it,
//!   and near the amplifier more, so the glow grows with the dark current's rise
//!   `k - 1` and is absent at or below the reference temperature. It carries the
//!   shot noise of the dark charge it joins. Long exposures at temperature therefore
//!   glow in that corner, after short ones have shown grain and hot crosses.
//! - **Substrate bias.** The substrate voltage sets the barrier of the vertical
//!   overflow drain under each photodiode: raising it lowers the barrier, so the well
//!   holds less. The well is `full_well_e * w` with
//!   `w = clamp(1 - full_well_frac_per_v * (V - V_nom), 0.05, 2)` (PROVISIONAL: the
//!   well halves two volts above nominal).
//!   The drain and the barrier move together, so the anti-blooming ratio scales by the
//!   same `w` (PROVISIONAL): a raised bias clips early and spills further along the
//!   column at gross overexposure, a lowered one deepens both. Reference white's charge
//!   ([`Params::white_e`]) stays on the nominal well, so metering is unchanged and a
//!   raised bias clips reference white once the well falls below it; the image
//!   processor, which assumes the nominal well, develops the clipped highlights
//!   off-white. In movie mode the electronic shutter clears the photodiodes by pulsing
//!   the substrate; a bias more than a volt below nominal weakens the pulse, the charge
//!   it leaves behind leaks into the vertical register during the transfer, and the
//!   smear grows by the factor `1 + (V_nom - 1 - V)`, one more smear per volt
//!   (PROVISIONAL). Still mode, behind the mechanical shutter, has no smear to grow.
//!
//! The section has no state and draws no randomness of its own: the glow joins the
//! dark charge before its shot noise is drawn.
//!
//! # Smear
//!
//! Sony quotes smear in decibels by the V/10 method: the smear signal relative to the
//! signal of a bright window one tenth of the image height, at standard video timing,
//! where a charge packet takes about one exposure time to cross the array. The model
//! assumes the vertical register collects light at a fixed fraction of the
//! photodiode's rate, so a packet crossing the array gathers that fraction of the
//! light along its column for the transfer time. Hence, for each column,
//!
//! `smear_e = 10^(smear_db / 20) / 0.1 * mean_e * readout_s / exposure_s`,
//!
//! times the substrate bias's shutter factor above, where `mean_e` is the mean
//! photo-charge of the column before the well limit (the
//! register sees the light, not the clipped well), `readout_s` the time the frame's
//! rows take to read on the sensor's clock, at most the processor's frame period since
//! the sensor reads until VD, and `exposure_s` the integration time. Since `mean_e`
//! grows with the exposure time, the smear charge depends only on the light and the
//! transfer time: a shorter exposure leaves the same streak on a weaker signal.
//! Assumptions: the leak is uniform along the column (one streak of constant height,
//! no gradient from the position of the packet), the register does not saturate, and
//! the smear charge carries its own shot noise. With the mechanical shutter (still
//! mode) the array is dark during the transfer and there is no smear.
//!
//! [`expose`] writes a [`Charge`] plane in electrons at sensor resolution, which
//! [`readout::read`](crate::readout::read) transfers out of the array.

use std::fmt::{self, Display, Formatter};

use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use crate::noise;
use crate::plane::Plane;
use crate::preset::Mode;
use crate::readout::Direction;
use crate::scene::Channel;
use crate::validate::positive;
use crate::{Camera, Error, Preset, Scene, Timing, colour};

/// Salt of the sensor's temporal noise sources.
const SALT: u32 = noise::salt("sensor");

/// Salt of the unit's fixed pattern seed.
const SALT_UNIT: u32 = noise::salt("sensor.unit");

/// Salt of the photo-response non-uniformity.
const SALT_PRNU: u32 = noise::salt("sensor.prnu");

/// Salt of the dark-signal non-uniformity.
const SALT_DSNU: u32 = noise::salt("sensor.dsnu");

/// Salt that selects the hot photosites.
const SALT_HOT: u32 = noise::salt("sensor.hot");

/// Salt of the dark-current multiplier of each hot photosite.
const SALT_HOT_RATE: u32 = noise::salt("sensor.hot.rate");

/// Salt of the shot noise of the photosite charge.
const SALT_SHOT: u32 = noise::salt("sensor.shot");

/// Salt of the shot noise of the smear charge.
const SALT_SMEAR: u32 = noise::salt("sensor.smear");

/// Dark current of the weakest hot photosite, in multiples of the mean dark current.
///
/// Hot photosites of consumer CCDs carry 50 to 500 times the mean dark current; the
/// multiplier is drawn log-uniformly over that range.
const HOT_MIN_RATIO: f32 = 50.0;

/// Decades spanned by the hot photosites' dark-current multipliers: 50 to 500.
const HOT_DECADES: f32 = 1.0;

/// Fraction of the image height lit in the V/10 smear measurement (research section 3).
const V10_WINDOW_FRACTION: f32 = 0.1;

/// Smallest exposure scale auto-exposure applies: two stops under the set exposure.
///
/// The model scales the light, as the aperture and shutter of the A520 would within
/// their range at a fixed ISO; beyond two stops the camera would change gain instead.
pub(crate) const AE_MIN_SCALE: f32 = 0.25;

/// Largest exposure scale auto-exposure applies: two stops over the set exposure.
pub(crate) const AE_MAX_SCALE: f32 = 4.0;

/// Rec. 709 luminance weights of linear sRGB, which the exposure meter reproduces.
pub(crate) const LUMINANCE: [f64; 3] = [0.2126, 0.7152, 0.0722];

/// Coldest sensor temperature a camera or preset may give, in degrees Celsius.
///
/// The cold end of the industrial range, about where freeze spray holds a part; the
/// dark current there is a few thousandths of the room-temperature value.
pub(crate) const MIN_TEMPERATURE_C: f64 = -40.0;

/// Hottest sensor temperature a camera or preset may give, in degrees Celsius.
///
/// The usual maximum junction temperature of silicon parts; beyond it a heat gun
/// destroys the sensor rather than bending it.
pub(crate) const MAX_TEMPERATURE_C: f64 = 150.0;

/// Highest substrate bias a camera or preset may give, in volts.
///
/// PROVISIONAL: twice the 9 to 15 V range in which Sony sets the bias of its
/// interline parts; long before it the linear well loss has emptied the well.
pub(crate) const MAX_SUBSTRATE_V: f64 = 30.0;

/// Smallest accepted temperature rise per doubling of the dark current, in degrees Celsius.
///
/// Silicon doubles every 5 to 10 °C near room temperature. A camera file below 3 °C
/// is a typo, and the factor over the whole temperature range, `2^(190 / 3)`, stays
/// far inside single precision.
pub(crate) const MIN_DARK_DOUBLING_C: f64 = 3.0;

/// Largest accepted temperature rise per doubling of the dark current, in degrees Celsius.
///
/// Beyond 30 °C the dark current hardly follows the temperature, which is not silicon.
pub(crate) const MAX_DARK_DOUBLING_C: f64 = 30.0;

/// Smallest well, relative to the nominal, that a raised substrate bias leaves.
///
/// PROVISIONAL: the linear well loss stops at a twentieth of the well, so a bias far
/// above nominal holds a sliver of charge rather than none or a negative capacity.
const MIN_WELL_RATIO: f64 = 0.05;

/// Largest well, relative to the nominal, that a lowered substrate bias gives.
///
/// PROVISIONAL: the barrier cannot rise without limit; at twice the nominal well the
/// photodiode's own capacitance, not the drain's barrier, holds the charge.
const MAX_WELL_RATIO: f64 = 2.0;

/// Drop of the substrate bias below nominal, in volts, that the electronic shutter tolerates.
///
/// PROVISIONAL: the shutter pulse rides on the bias with margin to spare; only beyond
/// a volt below nominal does it stop clearing the photodiodes and the smear grow.
const SHUTTER_MARGIN_V: f64 = 1.0;

/// Layout of the colour filter array.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Cfa {
    /// Bayer pattern with red at `(0, 0)` and blue at `(1, 1)`.
    #[default]
    #[cfg_attr(feature = "serde", serde(rename = "RGGB"))]
    Rggb,
}

impl Cfa {
    /// Returns the filter colour of the photosite at column `x`, row `y`.
    #[must_use]
    pub fn channel(self, x: u32, y: u32) -> Channel {
        match self {
            Self::Rggb => match (x % 2, y % 2) {
                (0, 0) => Channel::Red,
                (1, 1) => Channel::Blue,
                _ => Channel::Green,
            },
        }
    }
}

impl Display for Cfa {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Rggb => "RGGB",
        })
    }
}

/// Electrons collected by every photosite, in colour filter array layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Charge {
    plane: Plane<f32>,
}

impl Charge {
    /// Creates an empty charge plane of `width` by `height` photosites.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            plane: Plane::new(width, height),
        }
    }

    /// Returns the charge in electrons per photosite.
    #[must_use]
    pub fn plane(&self) -> &Plane<f32> {
        &self.plane
    }

    /// Returns the charge plane mutably.
    pub fn plane_mut(&mut self) -> &mut Plane<f32> {
        &mut self.plane
    }
}

/// How the exposure is set for each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ae {
    /// The light is integrated as configured.
    Fixed,
    /// The light is scaled so the scene's mean luminance, at the set exposure, lands on `target`.
    Scene {
        /// Mean linear luminance of the developed frame, reference white 1.0.
        target: f32,
        /// Luminance weights of the prepared scene's channels, in sensor primaries.
        weights: [f32; 3],
    },
}

/// Physical parameters of the photosite array for one readout mode.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Full-well capacity at the nominal substrate bias in electrons.
    ///
    /// Reference white's charge is set on it ([`white_e`](Self::white_e)); the charge
    /// clips at the well of the actual bias, [`well_e`](Self::well_e).
    pub full_well_e: f32,
    /// Fraction of the full well that reference white fills at the nominal exposure.
    pub white_fraction: f32,
    /// Exposure time at which reference white fills `white_fraction` of the well, in seconds.
    pub nominal_exposure_s: f32,
    /// Mean dark current at the sensor's temperature in electrons per second.
    ///
    /// The camera's `dark_e_per_s` times the Arrhenius factor `k`.
    pub dark_e_per_s: f32,
    /// Photo-response non-uniformity, relative gain spread (one standard deviation).
    pub prnu: f32,
    /// Dark-signal non-uniformity in electrons (one standard deviation).
    pub dsnu_e: f32,
    /// Overexposure, in multiples of the full well, the overflow drain absorbs before spill.
    ///
    /// The camera's, at the nominal substrate bias; the bias scales it by `well_ratio`.
    pub antibloom_ratio: f32,
    /// Interline smear relative to the signal in decibels, by the V/10 method.
    pub smear_db: f32,
    /// Hot photosites per megapixel.
    pub hot_pixels_per_mpix: f32,
    /// Extra dark current at the output amplifier's corner, in electrons per second.
    ///
    /// At the sensor's temperature: the camera's `amp_glow_e_per_s` times `k - 1`, so 0
    /// at or below the reference temperature.
    pub amp_glow_e_per_s: f32,
    /// Distance over which the amplifier's glow falls by `1/e`, as a fraction of the width.
    pub amp_glow_radius_frac: f32,
    /// Which sensor row the readout reads first; the amplifier sits at its first column.
    pub direction: Direction,
    /// Well and overflow drain at the substrate bias relative to the nominal bias's.
    ///
    /// The well holds `full_well_e * well_ratio` electrons and the drain absorbs
    /// `antibloom_ratio * well_ratio` of those wells; 1 at the nominal bias.
    pub well_ratio: f32,
    /// Interline smear relative to the camera's, raised by a weak electronic shutter.
    ///
    /// `1 + (V_nom - 1 - V)` for a substrate bias `V` more than a volt below the nominal
    /// `V_nom`, else 1; it scales [`smear_per_electron`](Self::smear_per_electron).
    pub smear_ratio: f32,
    /// Temperature of the sensor in degrees Celsius.
    ///
    /// The dark current and glow above are already at it; it sets
    /// [`read_noise_scale`](Self::read_noise_scale).
    pub temperature_c: f64,
    /// Temperature at which the camera's dark current and read noise hold, in degrees Celsius.
    pub reference_temp_c: f64,
    /// Relative rise of the read noise per degree above `reference_temp_c`.
    pub read_noise_per_c: f64,
    /// Colour filter array layout.
    pub cfa: Cfa,
    /// Integration time in seconds; the collected charge scales with it.
    pub exposure_s: f32,
    /// Time the vertical transfer takes to read the frame's rows, in seconds.
    ///
    /// The rows read on the sensor-side clock, capped at the processor's frame period:
    /// the timing generator reads until VD, which a slow `[bend.clock]` oscillator reaches
    /// before its last line.
    pub readout_s: f32,
    /// Whether the readout leaves the array exposed during transfer (electronic shutter).
    pub smear: bool,
    /// Exposure control.
    pub ae: Ae,
    /// Exposure scale of the image processor's auto-exposure servo; it replaces `ae` when set.
    ///
    /// `None` when resolved. The simulator sets it before each frame from
    /// [`isp::State::servo_exposure`](crate::isp::State::servo_exposure), the servo's
    /// value after metering the previous frame.
    pub exposure_scale: Option<f32>,
    /// Seed of the sensor's temporal noise.
    pub seed: u64,
    /// Seed of this unit's fixed pattern: PRNU, DSNU and hot photosites.
    pub unit_seed: u64,
}

impl Params {
    /// Resolves the sensor parameters from the camera, preset and derived timing.
    ///
    /// The exposure time is the preset's `[sensor] exposure_s` if set, otherwise the
    /// `exposure_s` of the readout mode's camera section; the movie readout's
    /// electronic shutter integrates at most one frame period, `1 / fps`. Each mode's
    /// camera section says whether interline smear is present. Scene metering weighs
    /// the scene's channels as the image processor will see them after the daylight
    /// white balance and colour matrix.
    ///
    /// The preset's `[bend.sensor]` sets the temperature and substrate bias, which
    /// resolve into the dark current, the amplifier's glow, the well and drain and the
    /// smear as the [module documentation](self) describes; without it they are the
    /// camera's.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if the preset's exposure time is not positive and
    /// finite, its `[bend.sensor]` values lie out of range, or the camera's colour
    /// matrix is not invertible.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let sensor = &camera.sensor;
        let set_s = preset.sensor.and_then(|o| o.exposure_s);
        if let Some(exposure_s) = set_s {
            positive("sensor.exposure_s", exposure_s)?;
        }
        let bend = preset.bend.sensor.unwrap_or_default();
        bend.validate()?;
        let temperature_c = bend.temperature_c.unwrap_or(sensor.reference_temp_c);
        let dark_ratio = dark_ratio(sensor, temperature_c);
        let substrate_v = bend.substrate_v.unwrap_or(sensor.substrate_v);
        let well_ratio = (1.0 - sensor.full_well_frac_per_v * (substrate_v - sensor.substrate_v))
            .clamp(MIN_WELL_RATIO, MAX_WELL_RATIO);
        // Volts by which the bias lies further below nominal than the shutter tolerates.
        let shutter_drop_v = (sensor.substrate_v - SHUTTER_MARGIN_V - substrate_v).max(0.0);
        let smear_ratio = 1.0 + shutter_drop_v;
        let (exposure_s, smear) = match timing.mode {
            Mode::Still => (set_s.unwrap_or(camera.still.exposure_s), camera.still.smear),
            Mode::Movie => (
                set_s
                    .unwrap_or(camera.movie.exposure_s)
                    .min(1.0 / camera.movie.fps),
                camera.movie.smear,
            ),
        };
        let isp = &camera.isp;
        let mode = preset
            .isp
            .as_ref()
            .and_then(|o| o.ae_mode)
            .unwrap_or(isp.ae_mode);
        let ae = match mode {
            crate::isp::AeMode::Fixed => Ae::Fixed,
            // The servo's first frame is metered on the scene.
            crate::isp::AeMode::Scene | crate::isp::AeMode::Servo => {
                let cam_to_srgb = colour::cam_to_srgb(&isp.xyz_to_cam)?;
                let wb = [isp.wb_r_gain, 1.0, isp.wb_b_gain];
                let weights = std::array::from_fn(|c| {
                    let through_matrix: f64 =
                        (0..3).map(|k| LUMINANCE[k] * cam_to_srgb[k][c]).sum();
                    (through_matrix * wb[c]) as f32
                });
                let target = preset
                    .isp
                    .as_ref()
                    .and_then(|o| o.ae_target)
                    .unwrap_or(isp.ae_target) as f32;
                Ae::Scene { target, weights }
            }
        };
        Ok(Self {
            full_well_e: sensor.full_well_e as f32,
            white_fraction: sensor.white_fraction as f32,
            nominal_exposure_s: sensor.nominal_exposure_s as f32,
            dark_e_per_s: (sensor.dark_e_per_s * dark_ratio) as f32,
            prnu: sensor.prnu as f32,
            dsnu_e: sensor.dsnu_e as f32,
            antibloom_ratio: sensor.antibloom_ratio as f32,
            smear_db: sensor.smear_db as f32,
            hot_pixels_per_mpix: sensor.hot_pixels_per_mpix as f32,
            amp_glow_e_per_s: (sensor.amp_glow_e_per_s * (dark_ratio - 1.0).max(0.0)) as f32,
            amp_glow_radius_frac: sensor.amp_glow_radius_frac as f32,
            direction: camera.clocking(timing.mode).direction,
            well_ratio: well_ratio as f32,
            smear_ratio: smear_ratio as f32,
            temperature_c,
            reference_temp_c: sensor.reference_temp_c,
            read_noise_per_c: sensor.read_noise_per_c,
            cfa: sensor.cfa,
            exposure_s: exposure_s as f32,
            readout_s: (f64::from(timing.rows_read) * timing.line_period_s)
                .min(timing.frame_period_s) as f32,
            smear,
            ae,
            exposure_scale: None,
            seed: noise::stage_seed(preset.seed, SALT),
            unit_seed: sensor.unit_seed,
        })
    }

    /// Returns the electrons reference white collects at the set exposure, before auto-exposure.
    ///
    /// `white_fraction * full_well_e` at the nominal exposure, scaled by
    /// `exposure_s / nominal_exposure_s`.
    #[must_use]
    pub fn white_e(&self) -> f32 {
        self.white_fraction * self.full_well_e * self.relative_exposure()
    }

    /// Returns the exposure time relative to the nominal exposure.
    fn relative_exposure(&self) -> f32 {
        self.exposure_s / self.nominal_exposure_s
    }

    /// Returns the full well at the substrate bias, in electrons; the charge clips here.
    ///
    /// `full_well_e * well_ratio`.
    #[must_use]
    pub fn well_e(&self) -> f32 {
        self.full_well_e * self.well_ratio
    }

    /// Returns the smear charge per electron of mean column photo-charge.
    ///
    /// Zero without the electronic shutter; `smear_ratio` scales it.
    #[must_use]
    pub fn smear_per_electron(&self) -> f32 {
        if !self.smear || self.exposure_s <= 0.0 {
            return 0.0;
        }
        10f32.powf(self.smear_db / 20.0) / V10_WINDOW_FRACTION * self.readout_s / self.exposure_s
            * self.smear_ratio
    }

    /// Returns the factor on the analog front end's read noise at the sensor's temperature.
    ///
    /// `1 + read_noise_per_c * (temperature_c - reference_temp_c)`, at least 0: exactly 1
    /// at the reference temperature. The output amplifier's read noise is added by the
    /// AFE ([`afe::Params::read_noise_e`](crate::afe::Params::read_noise_e)), which
    /// takes the camera's `read_noise_e`; this is the factor that brings it to the
    /// sensor's temperature.
    #[must_use]
    pub fn read_noise_scale(&self) -> f64 {
        self.read_noise_per_c
            .mul_add(self.temperature_c - self.reference_temp_c, 1.0)
            .max(0.0)
    }
}

/// Returns the dark current at `temperature_c` relative to the reference temperature's.
///
/// The Arrhenius rule as a doubling every `dark_doubling_c` degrees,
/// `2^((temperature_c - reference_temp_c) / dark_doubling_c)`, exactly 1 at the
/// reference temperature.
fn dark_ratio(sensor: &crate::camera::Sensor, temperature_c: f64) -> f64 {
    ((temperature_c - sensor.reference_temp_c) / sensor.dark_doubling_c).exp2()
}

/// Returns the dark current of the preset's sensor temperature relative to the camera's.
///
/// The factor `k` of the [module documentation](self), for the stages that integrate
/// dark current outside this one: the masked optical-black photosites of the readout.
/// The preset's `[bend.sensor] temperature_c`, else the camera's reference.
pub(crate) fn dark_factor(camera: &Camera, preset: &Preset) -> f64 {
    let sensor = &camera.sensor;
    let temperature_c = preset
        .bend
        .sensor
        .and_then(|bend| bend.temperature_c)
        .unwrap_or(sensor.reference_temp_c);
    dark_ratio(sensor, temperature_c)
}

/// Persistent state of the photosite array: the last exposure and working buffers.
///
/// An interline CCD empties every photosite at each transfer, so no charge carries
/// over between frames.
#[derive(Debug, Clone)]
pub struct State {
    exposure_scale: f32,
    rows: Vec<f64>,
    columns: Vec<f64>,
    carry: Vec<f32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            exposure_scale: 1.0,
            rows: Vec::new(),
            columns: Vec::new(),
            carry: Vec::new(),
        }
    }
}

impl State {
    /// Returns the exposure scale applied to the last frame; 1 is the set exposure.
    #[must_use]
    pub fn exposure_scale(&self) -> f32 {
        self.exposure_scale
    }
}

/// Integrates `scene` into photosite charge for frame `index`.
///
/// # Panics
///
/// Panics if `out` does not have the size of `scene`.
pub fn expose(params: &Params, state: &mut State, scene: &Scene, index: u64, out: &mut Charge) {
    let (width, height) = (scene.width(), scene.height());
    assert!(
        out.plane.width() == width && out.plane.height() == height,
        "charge plane is {} x {} but the scene is {width} x {height}",
        out.plane.width(),
        out.plane.height()
    );
    if width == 0 || height == 0 {
        return;
    }
    let scale = params
        .exposure_scale
        .unwrap_or_else(|| exposure_scale(params, scene, &mut state.rows));
    state.exposure_scale = scale;
    let electrons_per_unit = params.white_e() * scale;
    let full_well = params.well_e();
    let spill_level = params.antibloom_ratio.mul_add(params.well_ratio, 1.0) * full_well;
    let any_spill = integrate(params, scene, index, electrons_per_unit, spill_level, out);
    if any_spill {
        bloom(spill_level, full_well, &mut out.plane, &mut state.carry);
    } else {
        out.plane
            .data_mut()
            .par_chunks_mut(width as usize)
            .for_each(|row| row.iter_mut().for_each(|e| *e = e.min(full_well)));
    }
    let smear = params.smear_per_electron();
    if smear > 0.0 {
        column_light(params.cfa, scene, &mut state.columns);
        let scale = smear * electrons_per_unit / height as f32;
        let frame_seed = params.seed ^ index;
        let columns = &state.columns;
        out.plane
            .data_mut()
            .par_chunks_mut(width as usize)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, (e, &light)) in row.iter_mut().zip(columns).enumerate() {
                    let added = scale * light as f32;
                    let noise = noise::gauss(x as u32, y as u32, SALT_SMEAR, frame_seed);
                    *e += (added + added.sqrt() * noise).max(0.0);
                }
            });
    }
}

/// Returns the exposure scale for `scene`, metering it when auto-exposure is on.
///
/// The meter sees the scene's light integrated for the set exposure time, so the
/// scale it returns compensates a longer or shorter exposure within its two stops.
fn exposure_scale(params: &Params, scene: &Scene, rows: &mut Vec<f64>) -> f32 {
    let Ae::Scene { target, weights } = params.ae else {
        return 1.0;
    };
    let height = scene.height() as usize;
    rows.clear();
    rows.resize(height, 0.0);
    let planes = scene.planes();
    rows.par_iter_mut().enumerate().for_each(|(y, sum)| {
        let y = y as u32;
        let channels = planes.map(|plane| plane.row(y));
        *sum = (0..channels[0].len())
            .map(|x| {
                (0..3)
                    .map(|c| f64::from(weights[c]) * f64::from(channels[c][x].max(0.0)))
                    .sum::<f64>()
            })
            .sum();
    });
    let count = height as f64 * f64::from(scene.width());
    let mean = rows.iter().sum::<f64>() / count * f64::from(params.relative_exposure());
    if mean > 0.0 {
        ((f64::from(target) / mean) as f32).clamp(AE_MIN_SCALE, AE_MAX_SCALE)
    } else {
        AE_MAX_SCALE
    }
}

/// Writes every photosite's charge before the well limit; returns whether any spills.
fn integrate(
    params: &Params,
    scene: &Scene,
    index: u64,
    electrons_per_unit: f32,
    spill_level: f32,
    out: &mut Charge,
) -> bool {
    let unit = noise::stage_seed(params.unit_seed, SALT_UNIT);
    let frame_seed = params.seed ^ index;
    let dark_mean = params.dark_e_per_s * params.exposure_s;
    let hot_fraction = params.hot_pixels_per_mpix * 1e-6;
    let (width, height) = (scene.width(), scene.height());
    let glow = Glow::new(params, width, height);
    let cfa = params.cfa;
    out.plane
        .data_mut()
        .par_chunks_mut(width as usize)
        .enumerate()
        .map(|(y, row)| {
            let y = y as u32;
            let source = Channel::ALL.map(|channel| scene.plane(channel).row(y));
            let glow_row = glow.map(|glow| (glow, glow.dy2(y)));
            let mut spills = false;
            for (x, charge) in (0..width).zip(row.iter_mut()) {
                let radiance = source[cfa.channel(x, y).index()][x as usize].max(0.0);
                let mut e = radiance * electrons_per_unit;
                if params.prnu > 0.0 {
                    e *= params
                        .prnu
                        .mul_add(noise::gauss(x, y, SALT_PRNU, unit), 1.0);
                }
                let mut dark = dark_mean;
                if hot_fraction > 0.0 && noise::unit(x, y, SALT_HOT, unit) < hot_fraction {
                    let decades = HOT_DECADES * noise::unit(x, y, SALT_HOT_RATE, unit);
                    dark *= HOT_MIN_RATIO * 10f32.powf(decades);
                }
                if let Some((glow, dy2)) = glow_row {
                    dark += glow.at(x, dy2);
                }
                if params.dsnu_e > 0.0 {
                    dark += params.dsnu_e * noise::gauss(x, y, SALT_DSNU, unit);
                }
                e = (e + dark).max(0.0);
                e = e
                    .sqrt()
                    .mul_add(noise::gauss(x, y, SALT_SHOT, frame_seed), e)
                    .max(0.0);
                spills |= e > spill_level;
                *charge = e;
            }
            spills
        })
        .reduce(|| false, |a, b| a | b)
}

/// The output amplifier's glow over one exposure, falling off from its corner.
#[derive(Debug, Clone, Copy)]
struct Glow {
    /// Glow charge at the corner photosite over the exposure, in electrons.
    corner_e: f32,
    /// Row of the corner photosite: the first row the readout reads.
    row: u32,
    /// Inverse of the falloff distance, per photosite pitch.
    per_px: f32,
}

impl Glow {
    /// Returns the glow of this exposure, or `None` where there is none to add.
    fn new(params: &Params, width: u32, height: u32) -> Option<Self> {
        let corner_e = params.amp_glow_e_per_s * params.exposure_s;
        (corner_e > 0.0).then(|| Self {
            corner_e,
            row: match params.direction {
                Direction::TopDown => 0,
                Direction::BottomUp => height - 1,
            },
            per_px: 1.0 / (params.amp_glow_radius_frac * width as f32),
        })
    }

    /// Returns the squared distance of row `y` from the corner's row, in photosite pitches.
    fn dy2(self, y: u32) -> f32 {
        let dy = y.abs_diff(self.row) as f32;
        dy * dy
    }

    /// Returns the glow charge at column `x` of a row `dy2` squared pitches from the corner.
    fn at(self, x: u32, dy2: f32) -> f32 {
        let dx = x as f32;
        let d = dx.mul_add(dx, dy2).sqrt();
        self.corner_e * (-d * self.per_px).exp()
    }
}

/// Drains or spills the charge above `spill_level` along each column, then clips at the full well.
///
/// A photosite above the spill level sends half its excess down and half up. In the
/// downward pass each spilled packet travels down its column and is absorbed by the
/// free capacity, well plus drain, of the photosites it meets; the upward pass does
/// the same towards the top, after which every photosite is clipped at `full_well`.
/// Charge that reaches the end of a column is lost. Linear in the photosite count.
fn bloom(spill_level: f32, full_well: f32, plane: &mut Plane<f32>, carry: &mut Vec<f32>) {
    carry.clear();
    carry.resize(plane.width() as usize, 0.0);
    for row in plane.rows_mut() {
        for (e, c) in row.iter_mut().zip(carry.iter_mut()) {
            if *e > spill_level {
                let half = 0.5 * (*e - spill_level);
                *e = spill_level + half;
                *c += half;
            } else if *c > 0.0 {
                let taken = c.min(spill_level - *e);
                *e += taken;
                *c -= taken;
            }
        }
    }
    carry.fill(0.0);
    for row in plane.rows_mut().rev() {
        for (e, c) in row.iter_mut().zip(carry.iter_mut()) {
            if *e > spill_level {
                *c += *e - spill_level;
                *e = spill_level;
            } else if *c > 0.0 {
                let taken = c.min(spill_level - *e);
                *e += taken;
                *c -= taken;
            }
            *e = e.min(full_well);
        }
    }
}

/// Sums the light on each column, in scene units through each photosite's filter.
fn column_light(cfa: Cfa, scene: &Scene, columns: &mut Vec<f64>) {
    /// Columns summed by one task; each task walks all rows of its columns.
    const CHUNK: usize = 64;
    let (width, height) = (scene.width(), scene.height());
    columns.clear();
    columns.resize(width as usize, 0.0);
    columns
        .par_chunks_mut(CHUNK)
        .enumerate()
        .for_each(|(chunk, sums)| {
            let first = chunk * CHUNK;
            for y in 0..height {
                let rows = Channel::ALL.map(|channel| scene.plane(channel).row(y));
                for (i, sum) in sums.iter_mut().enumerate() {
                    let x = first + i;
                    let channel = cfa.channel(x as u32, y).index();
                    *sum += f64::from(rows[channel][x].max(0.0));
                }
            }
        });
}
