//! The supply rail: the 3.3 V rail of the AFE, the bend's logic and the processor.
//!
//! # Place in the chain
//!
//! Scene, sensor, readout, then **[`track`]**, then the AFE, ADC, bus bend, capture,
//! black clamp, memory, development and codec. [`track`] fills a [`Rail`], the rail
//! voltage at the start and end of every readout line, before the AFE converts the
//! frame; [`afe::process`](crate::afe::process), [`bus::bend`] and
//! [`isp::clamp_black`](crate::isp::clamp_black) receive it, because the rail feeds
//! the AFE, the bend's 74HC logic and the processor's input stage.
//!
//! # Model
//!
//! The camera's `[power]` section describes the rail: a source of `rail_v` behind
//! `source_ohm`, decoupled by `decoupling_uf` and loaded by `quiescent_ma`, with the
//! thresholds below which the AFE, the logic and the processor fail. A preset's
//! `[bend.rail]` adds a steady load, seeded spikes and ripple and may replace the
//! source impedance; the bus bend reports the current it drew on each line of the
//! last frame in [`bus::State::line_current_ma`](crate::bus::State::line_current_ma).
//!
//! The sag itself arrives in stage 12 of `docs/PLAN.md` (section 3.9, "Power"). Until
//! then the rail holds the camera's nominal `rail_v` on every line, with or without
//! `[bend.rail]`, and its consumers do not read it, so the section changes no frame.
//!
//! # Indexing
//!
//! A [`Rail`] has one entry per readout line, in readout order (line 0 is read
//! first, whatever the readout direction), as [`Layout`] numbers them. Each entry
//! holds the voltage at the start of the line's first stored sample and at the end
//! of its horizontal blanking, the start of the next line period; within the line the
//! voltage is linear between the two ([`Rail::v_at`]). Vertical blanking has no
//! entries.

use std::fmt::{self, Debug, Formatter};

use crate::preset::RailBend;
use crate::readout::Layout;
use crate::{Camera, Error, Preset, Timing, bus};

/// The rail voltage at the start and end of every readout line, in volts.
///
/// Allocated once from the readout [`Layout`] and filled at the camera's nominal
/// voltage; [`track`] rewrites it every frame while `[bend.rail]` is present. See the
/// [module documentation](self) for how it is indexed.
///
/// # Examples
///
/// ```
/// use vcb_core::power::Rail;
/// use vcb_core::readout::{Direction, Layout};
/// use vcb_core::{Camera, Preset, Timing};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cameras/a520.toml");
/// let camera: Camera = toml::from_str(&std::fs::read_to_string(path)?)?;
/// let timing = Timing::derive(&camera, &Preset::new("a520"), 2272, 1704)?;
/// let mut rail = Rail::new(&Layout::from_timing(&timing, Direction::TopDown), 3.3);
/// rail.set(10, 3.3, 3.1);
/// assert!((rail.v_at(10, 0.5) - 3.2).abs() < 1e-12);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, PartialEq)]
pub struct Rail {
    /// Start and end voltage of every readout line.
    lines_v: Box<[[f64; 2]]>,
    /// The camera's nominal rail voltage.
    nominal_v: f64,
}

impl Rail {
    /// Creates a rail at `nominal_v` volts on every line of `layout`.
    #[must_use]
    pub fn new(layout: &Layout, nominal_v: f64) -> Self {
        Self {
            lines_v: vec![[nominal_v; 2]; layout.rows() as usize].into_boxed_slice(),
            nominal_v,
        }
    }

    /// Returns the camera's nominal rail voltage in volts.
    #[must_use]
    pub fn nominal_v(&self) -> f64 {
        self.nominal_v
    }

    /// Returns the number of readout lines.
    #[must_use]
    pub fn lines(&self) -> u32 {
        self.lines_v.len() as u32
    }

    /// Returns the voltage at the start of readout line `line`, in volts.
    ///
    /// # Panics
    ///
    /// Panics if `line` is not below [`lines`](Self::lines).
    #[must_use]
    pub fn start_v(&self, line: u32) -> f64 {
        self.lines_v[line as usize][0]
    }

    /// Returns the voltage at the end of readout line `line`'s blanking, in volts.
    ///
    /// # Panics
    ///
    /// Panics if `line` is not below [`lines`](Self::lines).
    #[must_use]
    pub fn end_v(&self, line: u32) -> f64 {
        self.lines_v[line as usize][1]
    }

    /// Returns the voltage at fraction `frac` of readout line `line`'s period, in volts.
    ///
    /// `frac` 0 is the start of the line's first stored sample and 1 the end of its
    /// horizontal blanking; between them the voltage is
    /// `start_v + frac * (end_v - start_v)`.
    ///
    /// # Panics
    ///
    /// Panics if `line` is not below [`lines`](Self::lines).
    #[must_use]
    pub fn v_at(&self, line: u32, frac: f64) -> f64 {
        let [start_v, end_v] = self.lines_v[line as usize];
        start_v + frac * (end_v - start_v)
    }

    /// Sets the voltage at the start and end of readout line `line`, in volts.
    ///
    /// # Panics
    ///
    /// Panics if `line` is not below [`lines`](Self::lines).
    pub fn set(&mut self, line: u32, start_v: f64, end_v: f64) {
        self.lines_v[line as usize] = [start_v, end_v];
    }

    /// Returns the start and end voltage of every line, in readout order.
    #[must_use]
    pub fn lines_v(&self) -> &[[f64; 2]] {
        &self.lines_v
    }

    /// Returns the start and end voltage of every line mutably.
    pub fn lines_v_mut(&mut self) -> &mut [[f64; 2]] {
        &mut self.lines_v
    }

    /// Returns every line to the nominal voltage.
    pub fn reset(&mut self) {
        self.lines_v.fill([self.nominal_v; 2]);
    }

    /// Returns whether every line holds the nominal voltage.
    #[must_use]
    pub fn is_nominal(&self) -> bool {
        self.lines_v
            .iter()
            .flatten()
            .all(|&v| v.to_bits() == self.nominal_v.to_bits())
    }
}

impl Debug for Rail {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rail")
            .field("lines", &self.lines_v.len())
            .field("nominal_v", &self.nominal_v)
            .finish_non_exhaustive()
    }
}

/// The resolved supply rail, present while the preset has a `[bend.rail]` section.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Nominal rail voltage in volts, the camera's `[power] rail_v`.
    pub rail_v: f64,
    /// Source impedance of the rail in ohms: the bend's `source_ohm`, else the camera's.
    pub source_ohm: f64,
    /// The preset's `[bend.rail]` section as written.
    pub bend: RailBend,
}

impl Params {
    /// Resolves the rail from the camera's `[power]` and the preset's `[bend.rail]`; `None` without the bend.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value of the section is out of range.
    pub fn resolve(
        camera: &Camera,
        preset: &Preset,
        timing: &Timing,
    ) -> Result<Option<Self>, Error> {
        let _ = timing;
        let Some(bend) = preset.bend.rail else {
            return Ok(None);
        };
        bend.validate()?;
        Ok(Some(Self {
            rail_v: camera.power.rail_v,
            source_ohm: bend.source_ohm.unwrap_or(camera.power.source_ohm),
            bend,
        }))
    }
}

/// The rail's memory from frame to frame; the default is the power-on state.
///
/// Holds nothing yet: the decoupling capacitor's voltage and the spike process
/// join it with the sag model.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct State;

/// Fills `rail` with the supply voltage of every readout line of frame `index`.
///
/// Without a bend (`params` is `None`) it does nothing: the rail already holds the
/// camera's nominal `rail_v`, at which the simulator builds it and to which it
/// returns it when the bend is removed. With one it writes the nominal voltage on
/// every line, as the sag model is not yet in place; `bus` is the bus bend's state
/// after the previous frame, whose per-line current loads the rail.
pub fn track(
    params: Option<&Params>,
    state: &mut State,
    bus: &bus::State,
    index: u64,
    rail: &mut Rail,
) {
    let Some(params) = params else {
        return;
    };
    let _ = (state, bus, index);
    rail.lines_v.fill([params.rail_v; 2]);
}
