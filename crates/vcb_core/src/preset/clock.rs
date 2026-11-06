//! The `[bend.clock]` section: the master oscillator replaced by a detuned one.

use crate::Error;
use crate::validate::{finite, non_negative, positive};

/// Default rate of the oscillator's drift in hertz.
///
/// PROVISIONAL: a slow wander, a couple of seconds per excursion, as an RC oscillator
/// warming in the hand.
const DEFAULT_DRIFT_HZ: f64 = 0.5;

/// Default supply sensitivity of the oscillator in parts per million per volt.
///
/// The LTC1799 class: about 0.1 % per volt of supply (C).
const DEFAULT_PPM_PER_V: f64 = 1000.0;

/// The `[bend.clock]` section: the timing generator and AFE reclocked from a new oscillator.
///
/// The LTC1799 mod: the crystal of the timing generator and AFE is replaced by an
/// oscillator at `osc_mhz`, while the processor keeps its own crystal, generates
/// vertical sync and captures the bus at the nominal pixel period. The nominal is the
/// camera's `[still]` or `[movie] pixel_clock_hz`; an oscillator at the nominal
/// frequency changes nothing. See [`capture`](crate::capture).
///
/// The section is named `[bend.clock]`; `[bend.clocks]`
/// ([`ClockBend`](crate::preset::ClockBend)) is the CCD's transfer clocks.
///
/// ```toml
/// [bend.clock]
/// osc_mhz = 26.4         # required
/// drift_ppm = 300.0
/// drift_hz = 0.5
/// hd_lock_px = 12.0      # the processor re-synchronises each line within 12 pixels
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct ReclockBend {
    /// Frequency of the replacement oscillator in megahertz, positive; required.
    pub osc_mhz: f64,
    /// Peak slow wander of the oscillator in parts per million, at least 0.
    ///
    /// A seeded drift, as an RC oscillator's; 0 is a steady oscillator.
    #[cfg_attr(feature = "serde", serde(default))]
    pub drift_ppm: f64,
    /// Rate of the drift in hertz, positive.
    #[cfg_attr(feature = "serde", serde(default = "default_drift_hz"))]
    pub drift_hz: f64,
    /// Supply sensitivity of the oscillator in parts per million per volt, finite.
    ///
    /// Acts on the rail's deviation from nominal when `[bend.rail]` is present.
    #[cfg_attr(feature = "serde", serde(default = "default_ppm_per_v"))]
    pub ppm_per_v: f64,
    /// Width of the processor's line-start capture window in pixels, at least 0.
    ///
    /// Within the window the processor re-synchronises each line to the timing
    /// generator's horizontal sync; 0 is free-running.
    #[cfg_attr(feature = "serde", serde(default))]
    pub hd_lock_px: f64,
}

/// Returns the default drift rate of a section that does not set one.
#[cfg(feature = "serde")]
fn default_drift_hz() -> f64 {
    DEFAULT_DRIFT_HZ
}

/// Returns the default supply sensitivity of a section that does not set one.
#[cfg(feature = "serde")]
fn default_ppm_per_v() -> f64 {
    DEFAULT_PPM_PER_V
}

impl ReclockBend {
    /// Creates a steady oscillator at `osc_mhz` with the default drift rate and supply sensitivity.
    #[must_use]
    pub fn new(osc_mhz: f64) -> Self {
        Self {
            osc_mhz,
            drift_ppm: 0.0,
            drift_hz: DEFAULT_DRIFT_HZ,
            ppm_per_v: DEFAULT_PPM_PER_V,
            hd_lock_px: 0.0,
        }
    }

    /// Checks every value of the section.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        positive("bend.clock.osc_mhz", self.osc_mhz)?;
        non_negative("bend.clock.drift_ppm", self.drift_ppm)?;
        positive("bend.clock.drift_hz", self.drift_hz)?;
        finite("bend.clock.ppm_per_v", self.ppm_per_v)?;
        non_negative("bend.clock.hd_lock_px", self.hd_lock_px)
    }
}
