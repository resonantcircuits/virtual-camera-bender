//! Physical simulation of a circuit-bent early-2000s CCD compact camera.
//!
//! The crate models the signal chain of a camera such as the Canon `PowerShot` A520 as
//! a sequence of hardware stages, each a module with a `Params` struct in physical
//! units, an optional persistent `State`, and a function that advances one frame:
//!
//! | Stage | Module | Output |
//! |---|---|---|
//! | crop, resample, scene to sensor | [`scene`] | [`Scene`] at sensor size |
//! | exposure into the colour filter array | [`sensor`] | [`sensor::Charge`] |
//! | vertical and horizontal transfer | [`readout`] | [`readout::Stream`] |
//! | supply rail of the AFE, logic and processor | [`power`] | [`power::Rail`] |
//! | analog front end | [`afe`] | [`afe::Analog`] |
//! | quantisation | [`adc`] | [`adc::Codes`] |
//! | bend circuit on the data bus | [`bus`] | [`adc::Codes`] (in place) |
//! | the processor's capture of the bus | [`capture`] | [`adc::Codes`] (in place) |
//! | black clamp | [`isp`] | [`isp::Raw`] |
//! | frame-buffer SDRAM | [`memory`] | [`isp::Raw`] (in place) |
//! | development | [`isp`] | [`Frame`] |
//! | JPEG simulation | [`codec`] | [`Frame`] (in place) |
//!
//! A [`Camera`] describes the hardware, a [`Preset`] picks a readout mode and declares
//! bends, and [`Timing`] derives the clocks that tie both to the input resolution.
//! The [`Simulator`] owns the resolved parameters, the persistent analog state and
//! all working buffers, and renders one [`Frame`] per call.
//!
//! The crate performs no I/O, reads no clocks and draws no entropy: it takes buffers
//! and a seed, so a given input, preset, seed and frame index always produce the same
//! bytes. All randomness comes from [`noise`].
//!
//! # Examples
//!
//! ```
//! use vcb_core::{Camera, Preset, Scene, Simulator};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
//! let camera: Camera = toml::from_str(&std::fs::read_to_string(format!("{root}/cameras/a520.toml"))?)?;
//! // The sensor takes the scene's size here; the default is the camera's native size.
//! let preset: Preset = toml::from_str("camera = \"a520\"\n[readout]\nresolution = \"match-input\"")?;
//!
//! let scene = Scene::from_srgb8(64, 48, &vec![128; 64 * 48 * 3]);
//! let mut sim = Simulator::builder((camera, preset)).input_size(64, 48).build()?;
//! let frame = sim.frame(&scene, 0);
//! assert_eq!((frame.width(), frame.height()), (64, 48));
//! # Ok(())
//! # }
//! ```

pub mod adc;
pub mod afe;
pub mod bus;
pub mod camera;
pub mod capture;
pub mod codec;
mod colour;
mod error;
mod frame;
pub mod isp;
pub mod memory;
pub mod noise;
pub mod plane;
pub mod post;
pub mod power;
pub mod preset;
pub mod readout;
mod resample;
pub mod scene;
pub mod sensor;
mod simulator;
mod srgb;
mod timing;
mod validate;

#[doc(inline)]
pub use camera::model::Camera;
#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use frame::Frame;
#[doc(inline)]
pub use preset::model::Preset;
#[doc(inline)]
pub use scene::model::Scene;
#[doc(inline)]
pub use simulator::{Reconfigured, SimDeps, Simulator, SimulatorBuilder};
#[doc(inline)]
pub use timing::Timing;
