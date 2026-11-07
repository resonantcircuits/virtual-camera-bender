//! The `[bend.serial]` section: a noisy contact on the AFE's serial programming link.

use crate::Error;
use crate::validate::within;

/// Default probability that a bit of a serial write reads wrong at the AFE.
///
/// PROVISIONAL: a contact that leaves most frames clean. The processor sends a few
/// hundred bits a frame (seven register writes of an 8-bit address and a data field),
/// so one bit in a thousand corrupts about one frame's writes in five, many of them
/// in padding or low bits that show nothing.
const DEFAULT_BIT_ERROR: f64 = 0.001;

/// The `[bend.serial]` section: bit errors on the AFE's three-wire serial interface.
///
/// The processor programs the AFE's registers (the VGA and pixel gains, the clamp
/// level, the power and output controls) over a three-wire serial link (SL, SCK,
/// SDATA), rewriting the whole set in every vertical blanking. A noisy contact on the
/// data line flips bits of the writes, address and data alike: now and then a frame
/// takes a wrong gain, a tinted or checkered mosaic, a shifted black or a dropped
/// output, and a write that lands on the wrong address leaves the intended register
/// as it was until a clean write. See [`afe`](crate::afe).
///
/// `bit_error = 0` leaves the camera as it is.
///
/// ```toml
/// [bend.serial]
/// bit_error = 0.001      # one bit in a thousand reads wrong at the AFE
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct SerialBend {
    /// Probability that a bit of a serial write reads wrong at the AFE, in `[0, 1]`.
    ///
    /// Each bit of every write, its register address and its data, flips on its own
    /// with this probability; 0 corrupts none.
    pub bit_error: f64,
}

impl Default for SerialBend {
    /// Returns a contact that flips one bit in a thousand.
    fn default() -> Self {
        Self {
            bit_error: DEFAULT_BIT_ERROR,
        }
    }
}

impl SerialBend {
    /// Checks every value of the section.
    pub(crate) fn validate(self) -> Result<(), Error> {
        within("bend.serial.bit_error", self.bit_error, 0.0, 1.0)
    }
}
