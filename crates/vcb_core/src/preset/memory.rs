//! The `[bend.memory]` section: faulted address pins on the frame-buffer SDRAM.

use std::fmt::{self, Display, Formatter};

use crate::Error;
use crate::validate::{fraction, non_negative};

/// The `[bend.memory]` section: stuck, bridged or marginal SDRAM address pins.
///
/// The processor writes the clamped raw frame into its SDRAM in readout order and
/// reads it back a frame period later. Each fault acts on one physical address pin
/// (`a0` upward) of the multiplexed row and column address, so a stuck pin corrupts
/// two bits of the linear address. See [`memory`](crate::memory).
///
/// ```toml
/// [bend.memory]
/// [[bend.memory.faults]]
/// pin = 3
/// kind = "high"
/// [[bend.memory.faults]]
/// pin = 5
/// kind = "bridge"
/// with = 6
/// on_frac = 0.3          # a marginal contact, closed 30 % of the time
/// switch_hz = 4.0
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct MemoryBend {
    /// The faulted address pins; empty leaves the memory intact.
    pub faults: Vec<AddressFault>,
}

/// One faulted SDRAM address pin.
///
/// `on_frac` and `switch_hz` are set together for a marginal contact, a random
/// telegraph process in camera time that is faulted for `on_frac` of the time and
/// switches about `switch_hz` times a second; without them the fault is solid.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct AddressFault {
    /// The address pin, `a0` upward; it must exist on the camera's SDRAM.
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
    /// Mean rate at which a marginal contact switches, per second, at least 0.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub switch_hz: Option<f64>,
}

/// What a fault does to an address pin.
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
    /// Checks every fault and that no pin is named twice.
    ///
    /// Whether the pins exist on the camera's SDRAM is checked when the simulator
    /// resolves the preset.
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let mut named = [None::<usize>; 256];
        for (index, fault) in self.faults.iter().enumerate() {
            let field = format!("bend.memory.faults[{index}]");
            fault.validate(&field)?;
            for (key, pin) in [("pin", Some(fault.pin)), ("with", fault.with)] {
                let Some(pin) = pin else { continue };
                if let Some(first) = named[usize::from(pin)] {
                    return Err(Error::config(format!(
                        "{field}.{key} names pin a{pin}, which bend.memory.faults[{first}] \
                         already names; each pin may carry one fault"
                    )));
                }
                named[usize::from(pin)] = Some(index);
            }
        }
        Ok(())
    }
}

impl AddressFault {
    /// Checks the fault on its own; `field` names it in errors.
    fn validate(&self, field: &str) -> Result<(), Error> {
        match (self.kind, self.with) {
            (FaultKind::Bridge, None) => {
                return Err(Error::config(format!(
                    "{field}.with must name the other pin of a bridge"
                )));
            }
            (FaultKind::Bridge, Some(with)) if with == self.pin => {
                return Err(Error::config(format!(
                    "{field}.with must name another pin than pin a{}",
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
