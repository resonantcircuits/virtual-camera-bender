//! The `[bend.reset]` section: missed and weak pulses on the CCD's reset gate.

use crate::Error;
use crate::validate::{non_negative, within};

/// Default mean length of a run of missed reset pulses, in native pixel periods.
///
/// PROVISIONAL: a flaky contact on the reset-gate wire that opens for a few pixel
/// periods at a time, long enough to use up the diffusion's swing on bright packets.
const DEFAULT_HOLD_PX: f64 = 8.0;

/// The `[bend.reset]` section: the reset gate's pulses missed or weakened.
///
/// The reset gate empties the CCD's floating diffusion before each charge packet; its
/// pulse is a wire on the sensor ribbon. A bender's flaky contact drops pulses in runs
/// (`drop_hz`, `hold_px`); a weakened pulse clears only `reset_frac` of the
/// diffusion's charge. Correlated double sampling cancels either until the charge left
/// on the diffusion uses up the output stage's swing (the camera's `[afe] fd_swing_v`),
/// after which packets are lost and read black. See [`afe`](crate::afe) for the model.
///
/// `drop_hz = 0` with `reset_frac = 1`, the default, leaves the camera as it is.
///
/// ```toml
/// [bend.reset]
/// drop_hz = 2000.0       # missed-pulse runs per second of readout
/// hold_px = 8.0          # mean run length in native pixel periods
/// reset_frac = 0.9       # a weak pulse clears 90 % of the diffusion's charge
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct ResetBend {
    /// Mean rate of missed-pulse runs per second of readout, at least 0; 0 misses none.
    ///
    /// Runs start as a Poisson process in camera time, `drop_hz` times the pixel
    /// period per period while none runs; the rate is physical and used as given at
    /// every resolution.
    pub drop_hz: f64,
    /// Mean length of a missed-pulse run in native pixel periods, at least 1.
    ///
    /// Lengths are geometric. A duration shorter than a line: it is scaled with the
    /// resolution, so a run spans the same number of pixels at every sensor size, and
    /// stays fixed in seconds under a reclocked timing generator.
    pub hold_px: f64,
    /// Fraction of the diffusion's charge a reset pulse clears, in `[0, 1]`; 1 is a full reset.
    pub reset_frac: f64,
}

impl Default for ResetBend {
    /// Returns a sound reset gate: no missed pulses, each one a full reset.
    fn default() -> Self {
        Self {
            drop_hz: 0.0,
            hold_px: DEFAULT_HOLD_PX,
            reset_frac: 1.0,
        }
    }
}

impl ResetBend {
    /// Returns whether every reset pulse arrives and clears the diffusion fully.
    ///
    /// An inert section renders byte-identically to none, whatever its `hold_px`.
    #[must_use]
    pub fn is_inert(&self) -> bool {
        self.drop_hz == 0.0 && self.reset_frac >= 1.0
    }

    /// Checks every value of the section.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        non_negative("bend.reset.drop_hz", self.drop_hz)?;
        if !(self.hold_px.is_finite() && self.hold_px >= 1.0) {
            return Err(Error::config(format!(
                "bend.reset.hold_px must be at least 1, got {}",
                self.hold_px
            )));
        }
        within("bend.reset.reset_frac", self.reset_frac, 0.0, 1.0)
    }
}
