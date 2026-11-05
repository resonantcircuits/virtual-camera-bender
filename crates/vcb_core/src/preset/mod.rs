//! Sections of a preset file: readout mode, bends and image-processor overrides.
//!
//! A [`Preset`](crate::Preset) references a camera by name, picks a readout mode and
//! resolution, declares the circuit bends patched into the camera, and optionally
//! overrides image-processor settings. Only `camera` is required; every other section
//! defaults to the unmodified camera in still mode at the input's resolution:
//!
//! ```toml
//! camera = "a520"
//! seed = 1
//! [readout]
//! mode = "still"              # or "movie"
//! resolution = "match-input"  # or "native"
//! [bend.bus]
//! source_pins = [9, 8, 7]
//! target = "pin:10"           # or "gnd"
//! function = "bypass"         # or "invert", "divide"
//! cap_uf = 1.0
//! pot_kohm = 10.0
//! [bend.clocks]
//! cte = 0.9995
//! [bend.afe.inject]
//! wave = "square"
//! freq_hz = 1200.0
//! amplitude_v = 0.15
//! ```
//!
//! The structs hold the file's values as written; each stage resolves the sections it
//! consumes into its own `Params`.

pub(crate) mod model;

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use crate::Error;
use crate::afe::Inject;
use crate::bus::{Function, Target};
use crate::isp::WbMode;

/// Which readout the sensor performs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum Mode {
    /// Every row of the sensor, behind the mechanical shutter.
    #[default]
    Still,
    /// Line-skipped live-view readout with the electronic shutter, as in movie recording.
    Movie,
}

impl Display for Mode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Still => "still",
            Self::Movie => "movie",
        })
    }
}

impl FromStr for Mode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "still" => Ok(Self::Still),
            "movie" => Ok(Self::Movie),
            other => Err(Error::config(format!(
                "readout mode must be \"still\" or \"movie\", got \"{other}\""
            ))),
        }
    }
}

/// How the virtual sensor's size is chosen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum Resolution {
    /// The sensor has the input's size; clocks scale so the frame period stays the camera's.
    #[default]
    MatchInput,
    /// The sensor has the camera's real size and clocks; the input is cropped and resampled.
    Native,
}

impl Display for Resolution {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MatchInput => "match-input",
            Self::Native => "native",
        })
    }
}

impl FromStr for Resolution {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "match-input" => Ok(Self::MatchInput),
            "native" => Ok(Self::Native),
            other => Err(Error::config(format!(
                "resolution must be \"match-input\" or \"native\", got \"{other}\""
            ))),
        }
    }
}

/// The `[readout]` section: readout mode and sensor resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct Readout {
    /// Still or movie readout.
    pub mode: Mode,
    /// Match the input's size or use the camera's native size.
    pub resolution: Resolution,
}

/// The `[bend]` section: circuit modifications patched into the camera.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct Bend {
    /// The bend circuit on the ADC data bus.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub bus: Option<BusBend>,
    /// Faults on the CCD's vertical and horizontal clocks.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub clocks: Option<ClockBend>,
    /// Modifications of the analog front end.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub afe: Option<AfeBend>,
}

/// The `[bend.bus]` section: the DIP-switch, capacitor and gate circuit on the data bus.
///
/// The selected source pins join node S, optionally through `series_ohm`; S couples
/// through the `cap_uf` capacitor to node F, which the `pot_kohm` potentiometer loads
/// to ground. `function` routes F to the target pin directly, through an inverter or
/// through a divide-by-two flip-flop.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct BusBend {
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
    /// Patch resistor in series with each source pin in ohms; 0 is a direct wire.
    #[cfg_attr(feature = "serde", serde(default))]
    pub series_ohm: f64,
    /// Whether the source and target banks share one bus wire.
    #[cfg_attr(feature = "serde", serde(default))]
    pub common_bus: bool,
}

/// The `[bend.clocks]` section: degraded CCD transfer clocks.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct ClockBend {
    /// Charge transfer efficiency replacing the sensor's own; `None` keeps the camera's.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub cte: Option<f64>,
    /// Mean rate of dropped vertical transfer pulses per second.
    pub v_pulse_drop_hz: f64,
    /// Mean rate of horizontal register phase glitches per second.
    pub h_glitch_hz: f64,
}

/// The `[bend.afe]` section: modifications of the analog front end.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct AfeBend {
    /// An oscillator coupled into the signal path ahead of the ADC.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub inject: Option<Inject>,
}

/// The `[isp]` section of a preset: replacements for the camera's processing values.
///
/// Every field is optional; `None` keeps the value from the camera file. Field
/// meanings match [`camera::Isp`](crate::camera::Isp).
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct IspOverride {
    /// White-balance mode; daylight gains unless set to auto.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub wb: Option<WbMode>,
    /// Daylight red gain.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub wb_r_gain: Option<f64>,
    /// Daylight blue gain.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub wb_b_gain: Option<f64>,
    /// Colour correction matrix, row major.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub ccm: Option<[[f64; 3]; 3]>,
    /// Tone curve slope at mid-grey.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub tone_contrast: Option<f64>,
    /// Tone curve shadow lift.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub tone_toe: Option<f64>,
    /// Tone curve highlight compression.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub tone_shoulder: Option<f64>,
    /// Chroma noise reduction radius as a fraction of the frame width.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub chroma_nr_frac: Option<f64>,
    /// Luma noise reduction radius as a fraction of the frame width.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub luma_nr_frac: Option<f64>,
    /// Unsharp-mask radius as a fraction of the frame width.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub sharpen_radius_frac: Option<f64>,
    /// Unsharp-mask gain.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub sharpen_gain: Option<f64>,
    /// Auto-exposure target luminance.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub ae_target: Option<f64>,
}
