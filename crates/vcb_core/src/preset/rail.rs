//! The `[bend.rail]` section: a loaded or dying 3.3 V supply rail.

use crate::Error;
use crate::validate::{non_negative, positive};

/// Default duration of one load spike in milliseconds.
///
/// PROVISIONAL: of the order of a flash charger's or a lens motor's inrush.
const DEFAULT_SPIKE_MS: f64 = 2.0;

/// Default ripple frequency in hertz.
///
/// PROVISIONAL: 100 Hz, rectified 50 Hz mains through a cheap mains adapter.
const DEFAULT_RIPPLE_HZ: f64 = 100.0;

/// The `[bend.rail]` section: extra loads and disturbances on the camera's supply rail.
///
/// The camera's `[power]` section describes the rail: its nominal voltage, the
/// source impedance of the regulator and battery, the decoupling and the quiescent
/// current. This section adds a steady load, seeded load spikes and ripple, and can
/// replace the source impedance as a weak battery would. The rail feeds the AFE, the
/// bend's logic and the processor; see [`power`](crate::power).
///
/// ```toml
/// [bend.rail]
/// load_ma = 150.0        # LEDs and the LTC1799 on the rail
/// spike_ma = 400.0       # the flash charger kicking in
/// spike_hz = 3.0
/// ripple_mv = 40.0
/// source_ohm = 2.0       # a tired battery
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct RailBend {
    /// A steady load added to the rail above the camera's quiescent current, in milliamperes, at least 0.
    ///
    /// LEDs, an added oscillator, a potentiometer to ground.
    pub load_ma: f64,
    /// Current of each load spike in milliamperes, at least 0.
    pub spike_ma: f64,
    /// Mean rate of load spikes per second, at least 0; 0 has no spikes.
    ///
    /// Spikes arrive as a Poisson process in camera time; one that arrives while a
    /// spike runs extends it rather than adding to its current.
    pub spike_hz: f64,
    /// Duration of one load spike in milliseconds, positive.
    pub spike_ms: f64,
    /// Peak amplitude of the ripple on the source in millivolts, at least 0.
    ///
    /// The rail sees it through the source impedance and the decoupling, a low-pass.
    pub ripple_mv: f64,
    /// Frequency of the ripple in hertz, positive.
    pub ripple_hz: f64,
    /// Source impedance replacing the camera's `[power] source_ohm`, in ohms, positive.
    ///
    /// `None` keeps the camera's; a dying battery raises it.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub source_ohm: Option<f64>,
}

impl Default for RailBend {
    /// Returns an undisturbed rail: no added load, spikes or ripple.
    fn default() -> Self {
        Self {
            load_ma: 0.0,
            spike_ma: 0.0,
            spike_hz: 0.0,
            spike_ms: DEFAULT_SPIKE_MS,
            ripple_mv: 0.0,
            ripple_hz: DEFAULT_RIPPLE_HZ,
            source_ohm: None,
        }
    }
}

impl RailBend {
    /// Checks every value of the section.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        non_negative("bend.rail.load_ma", self.load_ma)?;
        non_negative("bend.rail.spike_ma", self.spike_ma)?;
        non_negative("bend.rail.spike_hz", self.spike_hz)?;
        positive("bend.rail.spike_ms", self.spike_ms)?;
        non_negative("bend.rail.ripple_mv", self.ripple_mv)?;
        positive("bend.rail.ripple_hz", self.ripple_hz)?;
        if let Some(source_ohm) = self.source_ohm {
            positive("bend.rail.source_ohm", source_ohm)?;
        }
        Ok(())
    }
}
