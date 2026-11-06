//! The frame-buffer SDRAM: the raw frame stored and read back through its address pins.
//!
//! # Place in the chain
//!
//! Scene, sensor, readout, power, AFE, ADC, bus bend, capture, black clamp, then
//! **[`bend`]**, then development and codec. [`bend`] rewrites the clamped [`Raw`]
//! in place, between [`isp::clamp_black`](crate::isp::clamp_black) and
//! [`isp::develop`](crate::isp::develop).
//!
//! # Model
//!
//! The processor writes the clamped raw into its SDRAM in readout order and reads it
//! back a frame period later to develop it. The camera's `[memory]` section gives the
//! address layout (`column_bits`, `row_bits`, `bank_bits`) and the raw buffer's word
//! address `raw_base`; a preset's `[bend.memory]` lists faulted address pins, stuck
//! low or high, bridged, solid or marginal.
//!
//! The faulted store and load arrive in stage 12 of `docs/PLAN.md` (section 3.9,
//! "Memory"). Until then [`bend`] passes the raw through unchanged, with or without
//! `[bend.memory]`.

use crate::isp::Raw;
use crate::preset::AddressFault;
use crate::{Camera, Error, Preset, Timing};

/// The resolved address faults, present while the preset has a `[bend.memory]` section.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Bits of the SDRAM's linear word address: column, row and bank bits together.
    pub address_bits: u32,
    /// Word address of the raw frame buffer's first sample.
    pub raw_base: u64,
    /// The faulted pins, as the preset lists them.
    pub faults: Vec<AddressFault>,
}

impl Params {
    /// Resolves the faults against the camera's `[memory]` section; `None` without `[bend.memory]`.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a fault is invalid on its own or names a pin
    /// at or beyond the camera's address width, `column_bits + row_bits + bank_bits`.
    pub fn resolve(
        camera: &Camera,
        preset: &Preset,
        timing: &Timing,
    ) -> Result<Option<Self>, Error> {
        let _ = timing;
        let Some(bend) = &preset.bend.memory else {
            return Ok(None);
        };
        bend.validate()?;
        let address_bits = camera.memory.address_bits();
        for (index, fault) in bend.faults.iter().enumerate() {
            for (key, pin) in [("pin", Some(fault.pin)), ("with", fault.with)] {
                if let Some(pin) = pin.filter(|&pin| u32::from(pin) >= address_bits) {
                    return Err(Error::config(format!(
                        "bend.memory.faults[{index}].{key} names pin a{pin}, but the \
                         camera's SDRAM has {address_bits} address bits, a0 to a{}",
                        address_bits - 1
                    )));
                }
            }
        }
        Ok(Some(Self {
            address_bits,
            raw_base: camera.memory.raw_base,
            faults: bend.faults.clone(),
        }))
    }
}

/// The SDRAM's memory from frame to frame; the default is the power-on state.
///
/// Holds nothing yet: the SDRAM's content and the marginal contacts' states join it
/// with the faulted store and load.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct State;

/// Rewrites `raw` in place with the frame `index` as the processor reads it back from the SDRAM.
///
/// Without a bend (`params` is `None`) it does nothing. With one the raw passes
/// through unchanged, as the faulted store and load are not yet in place.
pub fn bend(params: Option<&Params>, state: &mut State, index: u64, raw: &mut Raw) {
    let Some(params) = params else {
        return;
    };
    let _ = (params, state, index, raw);
}
