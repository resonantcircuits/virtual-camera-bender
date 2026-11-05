//! Exposure: scene radiance integrated into photosite charge behind the colour filters.
//!
//! The sensor is an interline CCD with an RGGB Bayer colour filter array: photosite
//! `(0, 0)` is red, `(1, 0)` and `(0, 1)` are green, `(1, 1)` is blue, repeating every
//! two columns and rows. [`expose`] computes the electrons of every photosite in this
//! order (research section 3 for the numbers):
//!
//! 1. Exposure: `radiance[filter] * white_fraction * full_well_e * exposure_scale`, so
//!    reference white fills `white_fraction` of the well. `exposure_scale` is 1 unless
//!    auto-exposure meters the scene ([`Ae::Scene`]): then it is `target / mean
//!    luminance` of the prepared scene, clamped to two stops either way.
//! 2. Photo-response non-uniformity: a fixed gain `1 + prnu * N(0, 1)` per photosite.
//! 3. Dark signal: `dark_e_per_s * exposure_s`, times 50 to 500 (log-uniform) at the
//!    `hot_pixels_per_mpix` hot photosites, plus a fixed offset `dsnu_e * N(0, 1)`.
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
//!    array. The visible charge is then clipped at `full_well_e`.
//! 6. Interline smear, when the readout uses the electronic shutter (movie mode):
//!    light leaking into the vertical register adds the same charge to every
//!    photosite of a column (below).
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
//! where `mean_e` is the mean photo-charge of the column before the well limit (the
//! register sees the light, not the clipped well), `readout_s` the time the frame's
//! rows take to read and `exposure_s` the integration time. Assumptions: the leak is
//! uniform along the column (one streak of constant height, no gradient from the
//! position of the packet), the register does not saturate, and the smear charge
//! carries its own shot noise. With the mechanical shutter (still mode) the array is
//! dark during the transfer and there is no smear.
//!
//! [`expose`] writes a [`Charge`] plane in electrons at sensor resolution, which
//! [`readout::read`](crate::readout::read) transfers out of the array.

use std::fmt::{self, Display, Formatter};

use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use crate::noise;
use crate::plane::Plane;
use crate::preset::Mode;
use crate::scene::Channel;
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
const AE_MIN_SCALE: f32 = 0.25;

/// Largest exposure scale auto-exposure applies: two stops over the set exposure.
const AE_MAX_SCALE: f32 = 4.0;

/// Rec. 709 luminance weights of linear sRGB, which the exposure meter reproduces.
const LUMINANCE: [f64; 3] = [0.2126, 0.7152, 0.0722];

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
    /// The light is scaled so the scene's mean luminance lands on `target`.
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
    /// Full-well capacity in electrons; charge clips here.
    pub full_well_e: f32,
    /// Fraction of the full well that reference white fills.
    pub white_fraction: f32,
    /// Mean dark current in electrons per second.
    pub dark_e_per_s: f32,
    /// Photo-response non-uniformity, relative gain spread (one standard deviation).
    pub prnu: f32,
    /// Dark-signal non-uniformity in electrons (one standard deviation).
    pub dsnu_e: f32,
    /// Overexposure, in multiples of the full well, the overflow drain absorbs before spill.
    pub antibloom_ratio: f32,
    /// Interline smear relative to the signal in decibels, by the V/10 method.
    pub smear_db: f32,
    /// Hot photosites per megapixel.
    pub hot_pixels_per_mpix: f32,
    /// Colour filter array layout.
    pub cfa: Cfa,
    /// Integration time in seconds.
    pub exposure_s: f32,
    /// Time the vertical transfer takes to read the frame's rows, in seconds.
    pub readout_s: f32,
    /// Whether the readout leaves the array exposed during transfer (electronic shutter).
    pub smear: bool,
    /// Exposure control.
    pub ae: Ae,
    /// Seed of the sensor's temporal noise.
    pub seed: u64,
    /// Seed of this unit's fixed pattern: PRNU, DSNU and hot photosites.
    pub unit_seed: u64,
}

impl Params {
    /// Resolves the sensor parameters from the camera, preset and derived timing.
    ///
    /// Movie mode integrates for one frame period; still mode uses the still exposure.
    /// Each mode's camera section says whether interline smear is present. Scene
    /// metering weighs the scene's channels as the image processor will see them
    /// after the daylight white balance and colour matrix.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if the camera's colour matrix is not invertible.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let sensor = &camera.sensor;
        let (exposure_s, smear) = match timing.mode {
            Mode::Still => (camera.still.exposure_s, camera.still.smear),
            Mode::Movie => (1.0 / camera.movie.fps, camera.movie.smear),
        };
        let isp = &camera.isp;
        let ae = match isp.ae_mode {
            crate::isp::AeMode::Fixed => Ae::Fixed,
            crate::isp::AeMode::Scene => {
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
            dark_e_per_s: sensor.dark_e_per_s as f32,
            prnu: sensor.prnu as f32,
            dsnu_e: sensor.dsnu_e as f32,
            antibloom_ratio: sensor.antibloom_ratio as f32,
            smear_db: sensor.smear_db as f32,
            hot_pixels_per_mpix: sensor.hot_pixels_per_mpix as f32,
            cfa: sensor.cfa,
            exposure_s: exposure_s as f32,
            readout_s: (f64::from(timing.rows_read) * timing.line_period_s) as f32,
            smear,
            ae,
            seed: noise::stage_seed(preset.seed, SALT),
            unit_seed: sensor.unit_seed,
        })
    }

    /// Returns the smear charge per electron of mean column photo-charge.
    #[must_use]
    pub fn smear_per_electron(&self) -> f32 {
        if !self.smear || self.exposure_s <= 0.0 {
            return 0.0;
        }
        10f32.powf(self.smear_db / 20.0) / V10_WINDOW_FRACTION * self.readout_s / self.exposure_s
    }
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
    let scale = exposure_scale(params, scene, &mut state.rows);
    state.exposure_scale = scale;
    let electrons_per_unit = params.white_fraction * params.full_well_e * scale;
    let spill_level = (1.0 + params.antibloom_ratio) * params.full_well_e;
    let any_spill = integrate(params, scene, index, electrons_per_unit, spill_level, out);
    if any_spill {
        bloom(
            spill_level,
            params.full_well_e,
            &mut out.plane,
            &mut state.carry,
        );
    } else {
        let full_well = params.full_well_e;
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
    let mean = rows.iter().sum::<f64>() / count;
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
    let width = scene.width();
    let cfa = params.cfa;
    out.plane
        .data_mut()
        .par_chunks_mut(width as usize)
        .enumerate()
        .map(|(y, row)| {
            let y = y as u32;
            let source = Channel::ALL.map(|channel| scene.plane(channel).row(y));
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
