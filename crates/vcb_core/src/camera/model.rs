//! The camera model: every hardware section of a camera file.

use crate::camera::{
    Adc, Afe, Clocking, Info, Isp, Jpeg, Lens, Logic, Memory, Movie, Power, Sensor, Still,
};
use crate::preset::Mode;
use crate::sensor::{
    MAX_DARK_DOUBLING_C, MAX_SUBSTRATE_V, MAX_TEMPERATURE_C, MIN_DARK_DOUBLING_C, MIN_TEMPERATURE_C,
};
use crate::validate::{at_least, finite, fraction, non_negative, positive, within};
use crate::{Error, colour};

/// Largest supported ADC resolution; codes are stored as `u16`.
const MAX_ADC_BITS: u8 = 16;

/// Lowest accepted smear figure; datasheets quote about -80 to -120 dB.
const MIN_SMEAR_DB: f64 = -200.0;

/// Largest accepted corner falloff of the lens in stops.
///
/// Compact-camera lenses lose about 0.5 to 1.5 stops at the corner wide open; beyond
/// 8 stops the corners hold under 0.4 % of the centre's light and the value is a typo.
const MAX_VIGNETTE_STOPS: f64 = 8.0;

/// Largest accepted defocus or edge blur of the lens as a fraction of the frame.
///
/// A blur radius of half the frame or more has no edge left to blur and is a typo.
const MAX_LENS_BLUR_FRAC: f64 = 0.5;

/// Largest supported width of one SDRAM address field (column, row or bank) in bits.
const MAX_ADDRESS_FIELD_BITS: u8 = 16;

/// Allowed gap between the AFE pedestal and the nominal black code.
///
/// Expressed as a fraction of the code range. Both describe the same black level; a larger gap means one of them is mistyped.
const PEDESTAL_TOLERANCE: f64 = 0.01;

/// A camera model in physical units, as read from a camera file.
///
/// # Examples
///
/// ```
/// use vcb_core::Camera;
/// use vcb_core::preset::Mode;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
/// let camera: Camera = toml::from_str(&std::fs::read_to_string(path)?)?;
/// camera.validate()?;
/// assert!(camera.clocking(Mode::Movie).line_skip > 1);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Camera {
    /// The `[camera]` section.
    #[cfg_attr(feature = "serde", serde(rename = "camera"))]
    pub info: Info,
    /// The `[sensor]` section.
    pub sensor: Sensor,
    /// The `[lens]` section; a camera file without one has an even lens.
    #[cfg_attr(feature = "serde", serde(default))]
    pub lens: Lens,
    /// The `[still]` section.
    pub still: Still,
    /// The `[movie]` section.
    pub movie: Movie,
    /// The `[afe]` section.
    pub afe: Afe,
    /// The `[adc]` section.
    pub adc: Adc,
    /// The `[logic]` section.
    pub logic: Logic,
    /// The `[power]` section.
    pub power: Power,
    /// The `[isp]` section.
    pub isp: Isp,
    /// The `[memory]` section.
    pub memory: Memory,
    /// The `[jpeg]` section.
    pub jpeg: Jpeg,
}

impl Camera {
    /// Returns the clocking of the still or movie readout.
    #[must_use]
    pub fn clocking(&self, mode: Mode) -> Clocking {
        match mode {
            Mode::Still => Clocking {
                pixel_clock_hz: self.still.pixel_clock_hz,
                hblank_px: self.still.hblank_px,
                vblank_lines: self.still.vblank_lines,
                line_skip: 1,
                direction: self.still.direction,
            },
            Mode::Movie => Clocking {
                pixel_clock_hz: self.movie.pixel_clock_hz,
                hblank_px: self.movie.hblank_px,
                vblank_lines: self.movie.vblank_lines,
                line_skip: self.movie.line_skip,
                direction: self.movie.direction,
            },
        }
    }

    /// Returns the largest ADC code, `2^bits - 1`.
    #[must_use]
    pub fn max_code(&self) -> u16 {
        ((1u32 << self.adc.bits.min(MAX_ADC_BITS)) - 1) as u16
    }

    /// Checks that every value is physically meaningful and the sections are consistent.
    ///
    /// # Errors
    ///
    /// Returns a configuration error naming the first offending field.
    pub fn validate(&self) -> Result<(), Error> {
        if self.info.name.is_empty() {
            return Err(Error::config("camera.name must not be empty"));
        }
        self.validate_sensor()?;
        self.validate_modes()?;
        self.validate_analog()?;
        self.validate_logic_and_isp()?;
        self.validate_power()?;
        self.validate_memory()
    }

    fn validate_sensor(&self) -> Result<(), Error> {
        let s = &self.sensor;
        at_least("sensor.native_width", s.native_width, 2)?;
        at_least("sensor.native_height", s.native_height, 2)?;
        if !s.native_width.is_multiple_of(2) || !s.native_height.is_multiple_of(2) {
            return Err(Error::config(format!(
                "sensor native size must be even to hold whole Bayer quads, got {} x {}",
                s.native_width, s.native_height
            )));
        }
        at_least("sensor.ob_cols", s.ob_cols, 1)?;
        positive("sensor.pitch_um", s.pitch_um)?;
        positive("sensor.full_well_e", s.full_well_e)?;
        fraction("sensor.white_fraction", s.white_fraction)?;
        positive("sensor.nominal_exposure_s", s.nominal_exposure_s)?;
        non_negative("sensor.read_noise_e", s.read_noise_e)?;
        non_negative("sensor.dark_e_per_s", s.dark_e_per_s)?;
        non_negative("sensor.prnu", s.prnu)?;
        non_negative("sensor.dsnu_e", s.dsnu_e)?;
        positive("sensor.conv_gain_uv_per_e", s.conv_gain_uv_per_e)?;
        non_negative("sensor.antibloom_ratio", s.antibloom_ratio)?;
        within("sensor.smear_db", s.smear_db, MIN_SMEAR_DB, 0.0)?;
        fraction("sensor.cte", s.cte)?;
        non_negative("sensor.hot_pixels_per_mpix", s.hot_pixels_per_mpix)?;
        within(
            "sensor.reference_temp_c",
            s.reference_temp_c,
            MIN_TEMPERATURE_C,
            MAX_TEMPERATURE_C,
        )?;
        within(
            "sensor.dark_doubling_c",
            s.dark_doubling_c,
            MIN_DARK_DOUBLING_C,
            MAX_DARK_DOUBLING_C,
        )?;
        non_negative("sensor.read_noise_per_c", s.read_noise_per_c)?;
        non_negative("sensor.amp_glow_e_per_s", s.amp_glow_e_per_s)?;
        positive("sensor.amp_glow_radius_frac", s.amp_glow_radius_frac)?;
        within("sensor.substrate_v", s.substrate_v, 0.0, MAX_SUBSTRATE_V)?;
        within(
            "sensor.full_well_frac_per_v",
            s.full_well_frac_per_v,
            0.0,
            1.0,
        )?;
        positive("sensor.drain_loss_v", s.drain_loss_v)?;
        for (channel, response) in ["r", "g", "b"].iter().zip(s.nir_response) {
            non_negative(&format!("sensor.nir_response ({channel})"), response)?;
        }
        let lens = &self.lens;
        within(
            "lens.vignette_stops",
            lens.vignette_stops,
            0.0,
            MAX_VIGNETTE_STOPS,
        )?;
        for (field, frac) in [
            ("lens.nir_defocus_frac", lens.nir_defocus_frac),
            ("lens.filter_edge_blur_frac", lens.filter_edge_blur_frac),
        ] {
            if !(frac.is_finite() && (0.0..MAX_LENS_BLUR_FRAC).contains(&frac)) {
                return Err(Error::config(format!(
                    "{field} must lie in [0, {MAX_LENS_BLUR_FRAC}), got {frac}"
                )));
            }
        }
        Ok(())
    }

    fn validate_modes(&self) -> Result<(), Error> {
        let still = &self.still;
        positive("still.pixel_clock_hz", still.pixel_clock_hz)?;
        positive("still.exposure_s", still.exposure_s)?;

        let movie = &self.movie;
        positive("movie.pixel_clock_hz", movie.pixel_clock_hz)?;
        at_least("movie.line_skip", movie.line_skip, 1)?;
        at_least("movie.h_bin", movie.h_bin, 1)?;
        positive("movie.fps", movie.fps)?;
        positive("movie.exposure_s", movie.exposure_s)?;
        if movie.exposure_s > 1.0 / movie.fps {
            return Err(Error::config(format!(
                "movie.exposure_s {} s is longer than the frame period of {} s at {} fps",
                movie.exposure_s,
                1.0 / movie.fps,
                movie.fps
            )));
        }
        at_least("movie.output_width", movie.output_width, 1)?;
        at_least("movie.output_height", movie.output_height, 1)?;
        let rows = self.sensor.native_height / (2 * movie.line_skip) * 2;
        if rows < 2 || self.sensor.native_width / movie.h_bin < 1 {
            return Err(Error::config(format!(
                "movie line_skip {} and h_bin {} leave no image on a {} x {} sensor",
                movie.line_skip, movie.h_bin, self.sensor.native_width, self.sensor.native_height
            )));
        }
        Ok(())
    }

    fn validate_analog(&self) -> Result<(), Error> {
        let afe = &self.afe;
        finite("afe.pga_gain_db", afe.pga_gain_db)?;
        positive("afe.bandwidth_hz", afe.bandwidth_hz)?;
        positive("afe.slew_v_per_us", afe.slew_v_per_us)?;
        non_negative("afe.noise_1f_corner_hz", afe.noise_1f_corner_hz)?;
        at_least("afe.oversample", afe.oversample, 1)?;
        positive("afe.fd_swing_v", afe.fd_swing_v)?;

        let adc = &self.adc;
        within(
            "adc.bits",
            f64::from(adc.bits),
            1.0,
            f64::from(MAX_ADC_BITS),
        )?;
        at_least("adc.dsp_bits", u32::from(adc.dsp_bits), 1)?;
        if u32::from(adc.dsp_lsb) + u32::from(adc.dsp_bits) > u32::from(adc.bits) {
            return Err(Error::config(format!(
                "adc.dsp_lsb {} + adc.dsp_bits {} exceed the {}-bit bus",
                adc.dsp_lsb, adc.dsp_bits, adc.bits
            )));
        }
        positive("adc.full_scale_v", adc.full_scale_v)?;
        within("afe.pedestal_v", afe.pedestal_v, 0.0, adc.full_scale_v)?;
        let max_code = f64::from(self.max_code());
        within("adc.black_level", f64::from(adc.black_level), 0.0, max_code)?;
        let pedestal_code = afe.pedestal_v / adc.full_scale_v * max_code;
        if (pedestal_code - f64::from(adc.black_level)).abs() > PEDESTAL_TOLERANCE * max_code {
            return Err(Error::config(format!(
                "afe.pedestal_v {} V maps to code {pedestal_code:.1}, but adc.black_level is {}",
                afe.pedestal_v, adc.black_level
            )));
        }
        positive("adc.driver_r_ohm", adc.driver_r_ohm)?;
        within("adc.driver_r_spread", adc.driver_r_spread, 0.0, 0.5)?;
        positive("adc.v_oh", adc.v_oh)?;
        non_negative("adc.bus_series_ohm", adc.bus_series_ohm)?;
        non_negative("adc.line_pf", adc.line_pf)?;
        positive("adc.driver_slew_v_per_us", adc.driver_slew_v_per_us)?;
        let swing_s = adc.v_oh / (adc.driver_slew_v_per_us * 1e6);
        for (mode, clock_hz) in [
            ("still", self.still.pixel_clock_hz),
            ("movie", self.movie.pixel_clock_hz),
        ] {
            if swing_s * clock_hz > 1.0 {
                return Err(Error::config(format!(
                    "adc.driver_slew_v_per_us {}: a full swing takes {:.1} ns, longer than \
                     the {:.1} ns pixel period of the {mode} readout",
                    adc.driver_slew_v_per_us,
                    swing_s * 1e9,
                    1e9 / clock_hz
                )));
            }
        }
        within("adc.dsp_threshold_v", adc.dsp_threshold_v, 0.0, adc.v_oh)?;
        non_negative("adc.dsp_noise_mv", adc.dsp_noise_mv)?;
        if !(0.0..1.0).contains(&adc.latch_phase) {
            return Err(Error::config(format!(
                "adc.latch_phase must lie in [0, 1), got {}",
                adc.latch_phase
            )));
        }
        Ok(())
    }

    fn validate_logic_and_isp(&self) -> Result<(), Error> {
        let logic = &self.logic;
        positive("logic.hc_r_out_ohm", logic.hc_r_out_ohm)?;
        within(
            "logic.hc_threshold_v",
            logic.hc_threshold_v,
            0.0,
            self.adc.v_oh,
        )?;
        non_negative("logic.hc_hysteresis_v", logic.hc_hysteresis_v)?;
        non_negative("logic.hc_prop_delay_ns", logic.hc_prop_delay_ns)?;
        positive("logic.ff_fmax_hz", logic.ff_fmax_hz)?;
        non_negative("logic.ff_min_pulse_ns", logic.ff_min_pulse_ns)?;
        non_negative("logic.ff_setup_ns", logic.ff_setup_ns)?;
        non_negative("logic.ff_prop_delay_ns", logic.ff_prop_delay_ns)?;
        non_negative("logic.clamp_diode_v", logic.clamp_diode_v)?;
        positive("logic.clamp_r_ohm", logic.clamp_r_ohm)?;
        positive("logic.input_leak_mohm", logic.input_leak_mohm)?;

        let isp = &self.isp;
        positive("isp.wb_r_gain", isp.wb_r_gain)?;
        positive("isp.wb_b_gain", isp.wb_b_gain)?;
        for (row, coefficients) in isp.xyz_to_cam.iter().enumerate() {
            for (col, value) in coefficients.iter().enumerate() {
                finite(&format!("isp.xyz_to_cam[{row}][{col}]"), *value)?;
            }
        }
        colour::srgb_to_cam(&isp.xyz_to_cam)?;
        positive("isp.tone_contrast", isp.tone_contrast)?;
        within("isp.tone_toe", isp.tone_toe, 0.0, 1.0)?;
        within("isp.tone_shoulder", isp.tone_shoulder, 0.0, 1.0)?;
        within("isp.chroma_nr_frac", isp.chroma_nr_frac, 0.0, 1.0)?;
        within("isp.luma_nr_frac", isp.luma_nr_frac, 0.0, 1.0)?;
        within("isp.sharpen_radius_frac", isp.sharpen_radius_frac, 0.0, 1.0)?;
        non_negative("isp.sharpen_gain", isp.sharpen_gain)?;
        fraction("isp.ae_target", isp.ae_target)?;
        positive("isp.ae_tau_s", isp.ae_tau_s)?;
        positive("isp.wb_tau_s", isp.wb_tau_s)?;
        non_negative("isp.clamp_range", isp.clamp_range)?;

        let tables = &self.jpeg.tables;
        for (name, pair) in [
            ("superfine", &tables.superfine),
            ("fine", &tables.fine),
            ("normal", &tables.normal),
        ] {
            if pair
                .luma
                .iter()
                .chain(&pair.chroma)
                .flatten()
                .any(|&step| step == 0)
            {
                return Err(Error::config(format!(
                    "jpeg.tables.{name} contains a zero quantiser step"
                )));
            }
        }
        Ok(())
    }

    fn validate_power(&self) -> Result<(), Error> {
        let power = &self.power;
        positive("power.rail_v", power.rail_v)?;
        positive("power.source_ohm", power.source_ohm)?;
        positive("power.decoupling_uf", power.decoupling_uf)?;
        positive("power.quiescent_ma", power.quiescent_ma)?;
        at_least("power.afe_restart_lines", power.afe_restart_lines, 1)?;
        for (field, threshold_v) in [
            ("power.afe_uvlo_v", power.afe_uvlo_v),
            ("power.logic_min_v", power.logic_min_v),
            ("power.dsp_brownout_v", power.dsp_brownout_v),
        ] {
            positive(field, threshold_v)?;
            if threshold_v >= power.rail_v {
                return Err(Error::config(format!(
                    "{field} {threshold_v} V must lie below power.rail_v {} V",
                    power.rail_v
                )));
            }
        }
        Ok(())
    }

    fn validate_memory(&self) -> Result<(), Error> {
        let memory = &self.memory;
        for (field, bits) in [
            ("memory.column_bits", memory.column_bits),
            ("memory.row_bits", memory.row_bits),
            ("memory.bank_bits", memory.bank_bits),
        ] {
            if !(1..=MAX_ADDRESS_FIELD_BITS).contains(&bits) {
                return Err(Error::config(format!(
                    "{field} must lie in 1 to {MAX_ADDRESS_FIELD_BITS}, got {bits}"
                )));
            }
        }
        let words = 1u64 << memory.address_bits();
        if memory.raw_base >= words {
            return Err(Error::config(format!(
                "memory.raw_base {} lies beyond the SDRAM's {words} words",
                memory.raw_base
            )));
        }
        Ok(())
    }

    /// Returns the largest code the image processor reads, `2^dsp_bits - 1`.
    #[must_use]
    pub fn dsp_max_code(&self) -> u16 {
        ((1u32 << self.adc.dsp_bits.min(MAX_ADC_BITS)) - 1) as u16
    }
}
