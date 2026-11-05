//! The bend circuit on the ADC's parallel data bus.
//!
//! The documented A520 modification taps the data lines between the ADC and the image
//! processor. A DIP-switch bank joins the selected source lines into node S; S couples
//! through a capacitor into node F, which a potentiometer loads to ground. F reaches
//! the target line directly ([`Function::Bypass`]), through a 74HC00 inverter
//! ([`Function::Invert`]) or through a 74HC74 divide-by-two flip-flop
//! ([`Function::Divide`]). The target selector joins one data line or ground.
//!
//! [`bend`] rewrites the [`Codes`] in place with the words the image processor
//! latches from the bus. Without a bend the words pass unchanged. The circuit is
//! described in physical units; the current model passes every word through as the
//! ADC drove it.

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use crate::adc::Codes;
use crate::noise;
use crate::{Camera, Error, Preset, Timing};

/// Salt of the bus circuit's noise sources.
const SALT: u32 = noise::salt("bus");

/// Where the target selector connects the function output.
///
/// In a file this is written `"pin:N"` for data line `DN`, or `"gnd"`.
///
/// # Examples
///
/// ```
/// use vcb_core::bus::Target;
///
/// let target: Target = "pin:10".parse()?;
/// assert_eq!(target, Target::Pin(10));
/// assert_eq!(target.to_string(), "pin:10");
/// # Ok::<(), vcb_core::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "String", into = "String"))]
pub enum Target {
    /// Data line `Dk` of the bus.
    Pin(u8),
    /// Circuit ground.
    Gnd,
}

impl Display for Target {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pin(pin) => write!(f, "pin:{pin}"),
            Self::Gnd => f.write_str("gnd"),
        }
    }
}

impl FromStr for Target {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "gnd" {
            return Ok(Self::Gnd);
        }
        s.strip_prefix("pin:")
            .and_then(|pin| pin.parse().ok())
            .map(Self::Pin)
            .ok_or_else(|| {
                Error::config(format!(
                    "bus target must be \"pin:N\" or \"gnd\", got \"{s}\""
                ))
            })
    }
}

impl TryFrom<String> for Target {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Target> for String {
    fn from(value: Target) -> Self {
        value.to_string()
    }
}

/// The gate between node F and the target line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Function {
    /// A wire: F drives the target directly.
    #[default]
    Bypass,
    /// A 74HC00 NAND gate with tied inputs.
    Invert,
    /// A 74HC74 D flip-flop clocked by F with `D` tied to `/Q`.
    Divide,
}

/// The bend circuit and the bus drivers it loads, in physical units.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Data lines switched into the source bank, by bit number.
    pub source_pins: Vec<u8>,
    /// Where the function output is connected.
    pub target: Target,
    /// The gate between node F and the target.
    pub function: Function,
    /// Coupling capacitor in microfarads.
    pub cap_uf: f64,
    /// Potentiometer from node F to ground in kilohms.
    pub pot_kohm: f64,
    /// Patch resistor in series with each source pin in ohms.
    pub series_ohm: f64,
    /// Whether the source and target banks share one bus wire.
    pub common_bus: bool,
    /// ADC resolution in bits; data lines are `D0` to `D(bits-1)`.
    pub bits: u8,
    /// Output resistance of each ADC data-line driver in ohms.
    pub driver_r_ohm: f64,
    /// Relative spread of the driver resistance between lines.
    pub driver_r_spread: f64,
    /// Output-high voltage of the ADC drivers.
    pub v_oh: f64,
    /// Logic threshold of the image processor's bus inputs in volts.
    pub dsp_threshold_v: f64,
    /// Thermal noise at the image processor's inputs in millivolts.
    pub dsp_noise_mv: f64,
    /// Output resistance of the 74HC gates in ohms.
    pub hc_r_out_ohm: f64,
    /// Input threshold of the 74HC gates in volts.
    pub hc_threshold_v: f64,
    /// Input hysteresis of the 74HC gates in volts.
    pub hc_hysteresis_v: f64,
    /// Propagation delay of the 74HC gates in nanoseconds.
    pub hc_prop_delay_ns: f64,
    /// Duration of one pixel period in seconds.
    pub pixel_period_s: f64,
    /// Circuit simulation sub-steps per pixel period.
    pub oversample: u32,
    /// Seed of the circuit's noise sources and component spreads.
    pub seed: u64,
}

impl Params {
    /// Resolves the bend circuit from the preset's `[bend.bus]` section; `None` without one.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a source or target pin does not exist on an
    /// ADC of the camera's width, or a pin is selected twice.
    pub fn resolve(
        camera: &Camera,
        preset: &Preset,
        timing: &Timing,
    ) -> Result<Option<Self>, Error> {
        let Some(bus) = &preset.bend.bus else {
            return Ok(None);
        };
        let bits = camera.adc.bits;
        let pins_in_range = bus.source_pins.iter().all(|&pin| pin < bits);
        let target_in_range = match bus.target {
            Target::Pin(pin) => pin < bits,
            Target::Gnd => true,
        };
        if !pins_in_range || !target_in_range {
            return Err(Error::config(format!(
                "bend.bus pins {:?} -> {} must be data lines D0 to D{} of a {bits}-bit ADC",
                bus.source_pins,
                bus.target,
                bits - 1
            )));
        }
        let mut sorted = bus.source_pins.clone();
        sorted.sort_unstable();
        if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Error::config(format!(
                "bend.bus.source_pins {:?} selects a pin twice",
                bus.source_pins
            )));
        }
        Ok(Some(Self {
            source_pins: bus.source_pins.clone(),
            target: bus.target,
            function: bus.function,
            cap_uf: bus.cap_uf,
            pot_kohm: bus.pot_kohm,
            series_ohm: bus.series_ohm,
            common_bus: bus.common_bus,
            bits,
            driver_r_ohm: camera.adc.driver_r_ohm,
            driver_r_spread: camera.adc.driver_r_spread,
            v_oh: camera.adc.v_oh,
            dsp_threshold_v: camera.adc.dsp_threshold_v,
            dsp_noise_mv: camera.adc.dsp_noise_mv,
            hc_r_out_ohm: camera.logic.hc_r_out_ohm,
            hc_threshold_v: camera.logic.hc_threshold_v,
            hc_hysteresis_v: camera.logic.hc_hysteresis_v,
            hc_prop_delay_ns: camera.logic.hc_prop_delay_ns,
            pixel_period_s: timing.pixel_period_s,
            oversample: timing.oversample,
            seed: noise::stage_seed(preset.seed, SALT),
        }))
    }
}

/// Persistent state of the bend circuit between frames.
///
/// The pass-through bus model holds no charge or logic state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct State;

/// Rewrites `codes` with the words the image processor latches through the bend.
///
/// Without a bend (`params` is `None`) the words are unchanged.
pub fn bend(params: Option<&Params>, state: &mut State, index: u64, codes: &mut Codes) {
    let _ = (params, state, index, codes);
}
