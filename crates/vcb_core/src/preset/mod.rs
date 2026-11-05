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
//! orientation = "auto"        # or "landscape", "portrait"
//! [scene]
//! headroom = 2.5              # clipped white is 2.5 times reference white
//! shoulder_start = 0.8
//! [bend.bus]
//! source_pins = [9, 8, 7]
//! target = "pin:10"           # or "gnd"
//! function = "bypass"         # or "invert", "divide"
//! cap_uf = 1.0
//! pot_kohm = 10.0
//! pot_node = "f"              # or "s"
//! [bend.clocks]
//! cte = 0.9995
//! v_pulse_drop_hz = 2.0
//! h_glitch_hz = 1.0
//! [bend.afe]
//! inject = { wave = "square", freq_hz = 1200.0, amplitude_v = 0.15 }
//! ```
//!
//! The structs hold the file's values as written; each stage resolves the sections it
//! consumes into its own `Params`.

pub(crate) mod model;

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use crate::Error;
use crate::afe::{GainMod, Inject};
use crate::bus::{Function, PotNode, Target};
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

/// How the camera is held relative to the scene.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum Orientation {
    /// Portrait when the input is taller than wide, landscape otherwise.
    #[default]
    Auto,
    /// The camera is held level; sensor rows run across the scene.
    Landscape,
    /// The camera is held vertically; sensor rows run up the scene.
    ///
    /// The scene is rotated 90 degrees clockwise onto the landscape sensor and the
    /// developed frame is rotated back, so readout effects run vertically in the image.
    Portrait,
}

impl Display for Orientation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Landscape => "landscape",
            Self::Portrait => "portrait",
        })
    }
}

impl FromStr for Orientation {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "landscape" => Ok(Self::Landscape),
            "portrait" => Ok(Self::Portrait),
            other => Err(Error::config(format!(
                "orientation must be \"auto\", \"landscape\" or \"portrait\", got \"{other}\""
            ))),
        }
    }
}

/// The `[readout]` section: readout mode, sensor resolution and camera orientation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct Readout {
    /// Still or movie readout.
    pub mode: Mode,
    /// Match the input's size or use the camera's native size.
    pub resolution: Resolution,
    /// How the camera is held.
    pub orientation: Orientation,
}

/// The `[scene]` section: how bright the photographed scene was beyond the input's white.
///
/// An 8-bit input photo was tone-mapped by the camera that took it: its highlights
/// were rolled off by a shoulder and clipped at code 255. Scene preparation inverts a
/// soft shoulder that starts at `shoulder_start` (linear light, reference white 1.0)
/// and maps input white to `headroom`, so clipped skies and lights reach the virtual
/// sensor brighter than reference white. `headroom = 1` takes the input as it is.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct SceneInput {
    /// Brightness of the input's clipped white in multiples of reference white.
    pub headroom: f64,
    /// Linear input level, as a fraction of white, where the highlight shoulder begins.
    pub shoulder_start: f64,
}

impl Default for SceneInput {
    /// Returns a headroom of 2.5 above a shoulder that starts at 0.8 of white.
    fn default() -> Self {
        Self {
            headroom: 2.5,
            shoulder_start: 0.8,
        }
    }
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
/// through the `cap_uf` capacitor to node F, and the `pot_kohm` potentiometer loads one
/// of the two nodes to ground. `function` routes F to the target pin directly, through
/// an inverter or through a divide-by-two flip-flop; the target pin joins through
/// `series_ohm` too. Source pins lie in `D2` to `D11` and the target in `D4` to `D11`
/// or ground, the positions of the bend's selector switches.
///
/// `pot_node` picks the potentiometer's node:
///
/// - `"f"`: the pot returns F to ground, so the capacitor and pot form a high-pass
///   filter and F swings around 0 V with the edges of the source mix.
/// - `"s"`, as the schematic draws it: the pot loads the source bus, F keeps only the
///   gate inputs' leakage and clamp diodes, so F follows the source mix shifted by the
///   capacitor's charge, which the diodes restore whenever F passes a supply rail.
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
    /// Potentiometer to ground in kilohms; 0 grounds its node.
    pub pot_kohm: f64,
    /// The node the potentiometer loads: `"f"` (default) or `"s"`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub pot_node: PotNode,
    /// Patch resistor between each tapped pin and its bus in ohms; 0 is a direct wire.
    #[cfg_attr(feature = "serde", serde(default))]
    pub series_ohm: f64,
    /// Whether the source and target banks share one bus wire.
    #[cfg_attr(feature = "serde", serde(default))]
    pub common_bus: bool,
}

/// Mean duration of a vertical clock fault when a preset does not set one, in native lines.
///
/// PROVISIONAL: long enough to show as a visible slip, short enough to stay a glitch.
const DEFAULT_V_STALL_LINES: f64 = 4.0;

/// Largest horizontal glitch shift when a preset does not set one, as a fraction of the line.
///
/// The `0.2 * W` bound of the horizontal clock delay model in
/// `docs/research/circuit-bend-reference.md`.
const DEFAULT_H_SHIFT_FRAC: f64 = 0.2;

/// The `[bend.clocks]` section: degraded CCD transfer clocks.
///
/// See [`readout`](crate::readout) for the transfer model these values drive.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct ClockBend {
    /// Charge transfer efficiency replacing the sensor's own; `None` keeps the camera's.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub cte: Option<f64>,
    /// Mean rate of vertical clock faults (stalls and double pulses) per second of readout.
    pub v_pulse_drop_hz: f64,
    /// Mean duration of a vertical clock fault in native lines; at least 1.
    pub v_stall_lines: f64,
    /// Mean rate of horizontal register glitch bands per second of readout.
    pub h_glitch_hz: f64,
    /// Largest glitch-band shift as a fraction of the line, in `[0, 1]`.
    pub h_shift_frac: f64,
}

impl Default for ClockBend {
    fn default() -> Self {
        Self {
            cte: None,
            v_pulse_drop_hz: 0.0,
            v_stall_lines: DEFAULT_V_STALL_LINES,
            h_glitch_hz: 0.0,
            h_shift_frac: DEFAULT_H_SHIFT_FRAC,
        }
    }
}

/// The `[bend.afe]` section: modifications of the analog front end.
///
/// See [`afe`](crate::afe) for the analog model these values drive.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default, deny_unknown_fields))]
pub struct AfeBend {
    /// An oscillator coupled into the signal path ahead of the ADC.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub inject: Option<Inject>,
    /// An oscillator modulating the programmable gain.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub gain_mod: Option<GainMod>,
    /// Delay of the CDS reset sample in pixel periods; 0 is the correct clamp timing.
    pub cds_lag_px: f64,
    /// Fraction of the delayed level the late reset sample subtracts, in `[0, 1]`.
    pub cds_mix: f64,
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
    /// Colour matrix from XYZ to camera RGB that the processor assumes, row major.
    ///
    /// The sensor's response stays the camera file's; a different matrix here renders
    /// colours as a mis-profiled processor would.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub xyz_to_cam: Option<[[f64; 3]; 3]>,
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
