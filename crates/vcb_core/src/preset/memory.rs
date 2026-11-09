//! The `[bend.memory]` section: faulted address and data pins and the heat of the frame-buffer SDRAM.

use std::fmt::{self, Display, Formatter};

use crate::Error;
use crate::sensor::{MAX_TEMPERATURE_C, MIN_TEMPERATURE_C};
use crate::validate::{fraction, non_negative, within};

/// The `[bend.memory]` section: faulted SDRAM address and data pins, and the chip's heat.
///
/// The processor writes the clamped raw frame into its SDRAM in readout order and
/// reads it back a frame period later. Each address fault acts on one physical address
/// pin (`a0` upward) of the multiplexed row and column address, so a stuck pin corrupts
/// two bits of the linear address: a solid fault folds the frame onto itself in
/// powers of two, a marginal one tears it into bands and lets stale frames through.
/// Each data fault acts on one data line (`d0` upward), one bit of every stored
/// sample, so a stuck line shifts black with the signal: stuck high, it adds its
/// weight to every sample that lacks the bit; stuck low, it takes it from every
/// sample that carries it, folding the highlights down. A heat gun
/// on the board shortens the cells' retention until the weakest lose their charge
/// before the refresh comes: isolated bit errors first, then a frame eaten by them.
/// With `reaches_jpeg` the same pins and cells corrupt the compressed scan the encoder
/// keeps in the chip. See [`memory`](crate::memory).
///
/// ```toml
/// [bend.memory]
/// temperature_c = 110.0  # a heat gun on the board; the camera's reference is 25
/// reaches_jpeg = true    # the compressed scan lies in the bent chip too
/// [[bend.memory.faults]]
/// pin = 3
/// kind = "high"
/// [[bend.memory.faults]]
/// pin = 5
/// kind = "bridge"
/// with = 6
/// on_frac = 0.3          # a marginal contact, closed 30 % of the time
/// switch_hz = 4.0
/// [[bend.memory.data_faults]]
/// pin = 9                # data line d9, the top bit of a 10-bit sample
/// kind = "low"
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct MemoryBend {
    /// The faulted address pins; empty leaves the addresses intact.
    pub faults: Vec<AddressFault>,
    /// The faulted data pins; empty leaves the stored words intact.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Vec::is_empty"))]
    pub data_faults: Vec<DataFault>,
    /// Temperature of the SDRAM in degrees Celsius, in `[-40, 150]`.
    ///
    /// `None` keeps the camera's `[memory] reference_temp_c`, the temperature its
    /// `retention_s` is given at, where no cell loses its charge before the refresh.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub temperature_c: Option<f64>,
    /// Whether the faults reach the compressed buffer as well as the raw.
    ///
    /// The encoder keeps its entropy-coded scan in an SDRAM until the card writer takes
    /// it; whether that is the bent chip, and so within the faults' reach, is the
    /// memory map's doing. With `true` the faults and the hot cells corrupt the scan
    /// as they do the raw ([`memory::bend_scan`](crate::memory::bend_scan)), and the
    /// decoder reads it out of step: shifted macroblocks, tints, grey tails. The
    /// default `false` is PROVISIONAL: the A520's memory map is unknown, and the
    /// Fujifilm S9000's documented bend produced readable files with torn raws, so its
    /// compressed buffer lay out of the faults' reach.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "std::ops::Not::not"))]
    pub reaches_jpeg: bool,
}

/// One faulted SDRAM address pin.
///
/// `on_frac` and `switch_hz` are set together for a marginal contact, a random
/// telegraph process in camera time that is faulted for `on_frac` of the time and
/// completes `switch_hz` on-off cycles a second on average: it stays faulted for
/// `on_frac / switch_hz` seconds on average and clear for `(1 - on_frac) / switch_hz`.
/// Without them, or with `on_frac = 1`, the fault is solid.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct AddressFault {
    /// The address pin, `a0` upward; one of the pins that carry the multiplexed row and
    /// column address, `a0` to `a(max(column_bits, row_bits) - 1)` of the camera.
    pub pin: u8,
    /// What the fault does to the pin.
    pub kind: FaultKind,
    /// The other pin of a bridge; set exactly when `kind` is `"bridge"`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub with: Option<u8>,
    /// Fraction of the time a marginal contact is faulted, in `(0, 1]`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub on_frac: Option<f64>,
    /// Mean on-off cycles per second of a marginal contact, at least 0; 0 never switches.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub switch_hz: Option<f64>,
}

/// One faulted SDRAM data pin.
///
/// The fields and their rules are those of an [`AddressFault`], on the data lines:
/// `with` names the other data pin of a bridge, and `on_frac` with `switch_hz` make the
/// contact marginal, the same telegraph process. A fault acts as the word is written
/// and as it is read, so a solid fault reads stuck whatever was stored and a marginal
/// one leaves a corrupted bit in the word until the next write.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct DataFault {
    /// The data pin, `d0` upward; one of the camera's DQ lines, `d0` to `d(data_pins - 1)`.
    ///
    /// A word holds the processor's sample in its low bits, so a pin at or above the
    /// sample's `[adc] dsp_bits` carries no sample bit and its fault changes nothing.
    pub pin: u8,
    /// What the fault does to the pin.
    pub kind: FaultKind,
    /// The other data pin of a bridge; set exactly when `kind` is `"bridge"`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub with: Option<u8>,
    /// Fraction of the time a marginal contact is faulted, in `(0, 1]`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub on_frac: Option<f64>,
    /// Mean on-off cycles per second of a marginal contact, at least 0; 0 never switches.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub switch_hz: Option<f64>,
}

/// What a fault does to an address or data pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum FaultKind {
    /// The pin is stuck at logic 0.
    Low,
    /// The pin is stuck at logic 1.
    High,
    /// The pin is shorted to the pin `with`; both read the OR of the two.
    Bridge,
}

impl Display for FaultKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Low => "low",
            Self::High => "high",
            Self::Bridge => "bridge",
        })
    }
}

impl MemoryBend {
    /// Checks every fault, that no pin is named twice in a list, and the temperature.
    ///
    /// An address pin and a data pin of the same number are different pins. Whether
    /// the pins exist on the camera's SDRAM is checked when the simulator resolves the
    /// preset.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        validate_faults(
            "bend.memory.faults",
            'a',
            self.faults.iter().map(PinFault::from),
        )?;
        validate_faults(
            "bend.memory.data_faults",
            'd',
            self.data_faults.iter().map(PinFault::from),
        )?;
        if let Some(temperature_c) = self.temperature_c {
            within(
                "bend.memory.temperature_c",
                temperature_c,
                MIN_TEMPERATURE_C,
                MAX_TEMPERATURE_C,
            )?;
        }
        Ok(())
    }
}

/// One fault of either list, address or data, as validation sees it.
#[derive(Clone, Copy)]
struct PinFault {
    pin: u8,
    kind: FaultKind,
    with: Option<u8>,
    on_frac: Option<f64>,
    switch_hz: Option<f64>,
}

impl From<&AddressFault> for PinFault {
    fn from(fault: &AddressFault) -> Self {
        Self {
            pin: fault.pin,
            kind: fault.kind,
            with: fault.with,
            on_frac: fault.on_frac,
            switch_hz: fault.switch_hz,
        }
    }
}

impl From<&DataFault> for PinFault {
    fn from(fault: &DataFault) -> Self {
        Self {
            pin: fault.pin,
            kind: fault.kind,
            with: fault.with,
            on_frac: fault.on_frac,
            switch_hz: fault.switch_hz,
        }
    }
}

/// Checks each fault of one list and that no pin of the list is named twice.
///
/// `list` names the list in errors, `letter` the prefix of its pins' names.
fn validate_faults(
    list: &str,
    letter: char,
    faults: impl Iterator<Item = PinFault>,
) -> Result<(), Error> {
    let mut named = [None::<usize>; 256];
    for (index, fault) in faults.enumerate() {
        let field = format!("{list}[{index}]");
        fault.validate(&field, letter)?;
        for (key, pin) in [("pin", Some(fault.pin)), ("with", fault.with)] {
            let Some(pin) = pin else { continue };
            if let Some(first) = named[usize::from(pin)] {
                return Err(Error::config(format!(
                    "{field}.{key} names pin {letter}{pin}, which {list}[{first}] already \
                     names; each pin may carry one fault"
                )));
            }
            named[usize::from(pin)] = Some(index);
        }
    }
    Ok(())
}

impl PinFault {
    /// Checks the fault on its own; `field` names it in errors, `letter` prefixes its pins.
    fn validate(&self, field: &str, letter: char) -> Result<(), Error> {
        match (self.kind, self.with) {
            (FaultKind::Bridge, None) => {
                return Err(Error::config(format!(
                    "{field}.with must name the other pin of a bridge"
                )));
            }
            (FaultKind::Bridge, Some(with)) if with == self.pin => {
                return Err(Error::config(format!(
                    "{field}.with must name another pin than pin {letter}{}",
                    self.pin
                )));
            }
            (FaultKind::Low | FaultKind::High, Some(with)) => {
                return Err(Error::config(format!(
                    "{field}.with = {with} is only for a bridge, but the kind is \"{}\"",
                    self.kind
                )));
            }
            _ => {}
        }
        match (self.on_frac, self.switch_hz) {
            (Some(on_frac), Some(switch_hz)) => {
                fraction(&format!("{field}.on_frac"), on_frac)?;
                non_negative(&format!("{field}.switch_hz"), switch_hz)
            }
            (None, None) => Ok(()),
            (Some(_), None) => Err(Error::config(format!(
                "{field}.on_frac needs switch_hz too: a marginal contact sets both"
            ))),
            (None, Some(_)) => Err(Error::config(format!(
                "{field}.switch_hz needs on_frac too: a marginal contact sets both"
            ))),
        }
    }
}
