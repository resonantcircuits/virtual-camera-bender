//! The `[bend.sensor]` section: the sensor heated or cooled, its substrate bias turned.

use crate::Error;
use crate::sensor::{MAX_SUBSTRATE_V, MAX_TEMPERATURE_C, MIN_TEMPERATURE_C};
use crate::validate::within;

/// The `[bend.sensor]` section: the CCD's temperature and its substrate bias.
///
/// Two things a bender does to the sensor itself. A heat gun or freeze spray moves its
/// temperature, and with it the dark current, which doubles every `[sensor]
/// dark_doubling_c` degrees, the output amplifier's glow at the readout's first corner
/// and the read noise. A pot on the substrate bias (VSUB) moves the vertical overflow
/// drain's barrier: a raised bias shrinks the well and the drain, so highlights clip
/// early and gross overexposure spills further along the columns; a lowered one deepens
/// them, until in movie mode the electronic shutter, which pulses the substrate, no
/// longer clears the photodiodes and the smear grows. See [`sensor`](crate::sensor).
///
/// Every field is optional; `None` keeps the camera's value, so the default section is
/// the unmodified camera and renders the same bytes as no section.
///
/// ```toml
/// [bend.sensor]
/// temperature_c = 70.0     # a heat gun on the sensor; the camera's reference is 25
/// substrate_v = 13.5       # the VSUB pot 1.5 V above the nominal 12 V
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct SensorBend {
    /// Temperature of the sensor in degrees Celsius, in `[-40, 150]`.
    ///
    /// `None` keeps the camera's `[sensor] reference_temp_c`, the temperature its dark
    /// current and read noise are given at.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub temperature_c: Option<f64>,
    /// Substrate bias of the CCD in volts, in `[0, 30]`.
    ///
    /// `None` keeps the camera's nominal `[sensor] substrate_v`.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub substrate_v: Option<f64>,
}

impl SensorBend {
    /// Checks every value of the section.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if let Some(temperature_c) = self.temperature_c {
            within(
                "bend.sensor.temperature_c",
                temperature_c,
                MIN_TEMPERATURE_C,
                MAX_TEMPERATURE_C,
            )?;
        }
        if let Some(substrate_v) = self.substrate_v {
            within("bend.sensor.substrate_v", substrate_v, 0.0, MAX_SUBSTRATE_V)?;
        }
        Ok(())
    }
}
