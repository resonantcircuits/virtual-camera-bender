//! The preset: a camera reference plus readout mode, bends and overrides.

use crate::Error;
use crate::preset::{Bend, IspOverride, Readout};
use crate::validate::{finite, fraction, non_negative, positive, within};

/// A camera, a readout mode, the bends patched into it and processing overrides.
///
/// # Examples
///
/// ```
/// use vcb_core::Preset;
/// use vcb_core::preset::Mode;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let preset: Preset = toml::from_str(r#"camera = "a520""#)?;
/// assert_eq!(preset.readout.mode, Mode::Still);
/// assert!(preset.bend.bus.is_none());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct Preset {
    /// Name of the camera model, matching the camera file's `[camera] name`.
    pub camera: String,
    /// Seed of every noise source; the same seed renders the same bytes.
    #[cfg_attr(feature = "serde", serde(default))]
    pub seed: u64,
    /// Readout mode and resolution.
    #[cfg_attr(feature = "serde", serde(default))]
    pub readout: Readout,
    /// Circuit bends.
    #[cfg_attr(feature = "serde", serde(default))]
    pub bend: Bend,
    /// Image-processor overrides.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub isp: Option<IspOverride>,
}

impl Preset {
    /// Creates an unmodified still preset for the named camera.
    #[must_use]
    pub fn new(camera: impl Into<String>) -> Self {
        Self {
            camera: camera.into(),
            seed: 0,
            readout: Readout::default(),
            bend: Bend::default(),
            isp: None,
        }
    }

    /// Checks that every value set in the preset is physically meaningful.
    ///
    /// The camera's own values are checked by [`Camera::validate`](crate::Camera::validate).
    ///
    /// Checks that depend on the camera, such as bus pins against the ADC width, happen
    /// when the simulator resolves the preset.
    ///
    /// # Errors
    ///
    /// Returns a configuration error naming the first offending field.
    pub fn validate(&self) -> Result<(), Error> {
        if self.camera.is_empty() {
            return Err(Error::config("preset camera must not be empty"));
        }
        if let Some(bus) = &self.bend.bus {
            if bus.source_pins.is_empty() {
                return Err(Error::config(
                    "bend.bus.source_pins must select at least one pin",
                ));
            }
            positive("bend.bus.cap_uf", bus.cap_uf)?;
            non_negative("bend.bus.pot_kohm", bus.pot_kohm)?;
            non_negative("bend.bus.series_ohm", bus.series_ohm)?;
        }
        if let Some(clocks) = &self.bend.clocks {
            if let Some(cte) = clocks.cte {
                fraction("bend.clocks.cte", cte)?;
            }
            non_negative("bend.clocks.v_pulse_drop_hz", clocks.v_pulse_drop_hz)?;
            if !(clocks.v_stall_lines.is_finite() && clocks.v_stall_lines >= 1.0) {
                return Err(Error::config(format!(
                    "bend.clocks.v_stall_lines must be at least 1, got {}",
                    clocks.v_stall_lines
                )));
            }
            non_negative("bend.clocks.h_glitch_hz", clocks.h_glitch_hz)?;
            within("bend.clocks.h_shift_frac", clocks.h_shift_frac, 0.0, 1.0)?;
        }
        if let Some(afe) = &self.bend.afe {
            if let Some(inject) = &afe.inject {
                non_negative("bend.afe.inject.freq_hz", inject.freq_hz)?;
                non_negative("bend.afe.inject.amplitude_v", inject.amplitude_v)?;
                finite("bend.afe.inject.phase_skew", inject.phase_skew)?;
            }
            if let Some(gain_mod) = &afe.gain_mod {
                non_negative("bend.afe.gain_mod.freq_hz", gain_mod.freq_hz)?;
                within("bend.afe.gain_mod.depth", gain_mod.depth, 0.0, 1.0)?;
            }
            non_negative("bend.afe.cds_lag_px", afe.cds_lag_px)?;
            within("bend.afe.cds_mix", afe.cds_mix, 0.0, 1.0)?;
        }
        if let Some(isp) = &self.isp {
            validate_isp(isp)?;
        }
        Ok(())
    }
}

/// A range check from [`crate::validate`].
type Check = fn(&str, f64) -> Result<(), Error>;

fn validate_isp(isp: &IspOverride) -> Result<(), Error> {
    let checks: [(&str, Option<f64>, Check); 4] = [
        ("isp.wb_r_gain", isp.wb_r_gain, positive),
        ("isp.wb_b_gain", isp.wb_b_gain, positive),
        ("isp.tone_contrast", isp.tone_contrast, positive),
        ("isp.sharpen_gain", isp.sharpen_gain, non_negative),
    ];
    for (field, value, check) in checks {
        if let Some(value) = value {
            check(field, value)?;
        }
    }
    let unit_range = [
        ("isp.tone_toe", isp.tone_toe),
        ("isp.tone_shoulder", isp.tone_shoulder),
        ("isp.chroma_nr_frac", isp.chroma_nr_frac),
        ("isp.luma_nr_frac", isp.luma_nr_frac),
        ("isp.sharpen_radius_frac", isp.sharpen_radius_frac),
    ];
    for (field, value) in unit_range {
        if let Some(value) = value {
            within(field, value, 0.0, 1.0)?;
        }
    }
    if let Some(target) = isp.ae_target {
        fraction("isp.ae_target", target)?;
    }
    if let Some(matrix) = &isp.xyz_to_cam {
        if matrix.iter().flatten().any(|value| !value.is_finite()) {
            return Err(Error::config("isp.xyz_to_cam must contain finite values"));
        }
        crate::colour::srgb_to_cam(matrix)?;
    }
    Ok(())
}
