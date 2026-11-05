//! Image and video input and output for the virtual camera bender.
//!
//! [`read_image`] decodes a JPEG or PNG file into a linear-light
//! [`Scene`](vcb_core::Scene), applying the EXIF orientation so the scene is upright
//! as a viewer would display it. [`write_frame`] encodes a camera
//! [`Frame`](vcb_core::Frame) as PNG or JPEG. [`probe_image`] reads only the header
//! and the EXIF block, for sizing a simulator before decoding.
//!
//! 8-bit images are decoded exactly; 16-bit images are reduced to 8 bits first.
//! Missing or unreadable EXIF data counts as orientation 1 (upright).
//!
//! The [`video`] module reads and writes video files through `ffmpeg` child
//! processes; it is the only part of the project that starts processes.
//!
//! # Examples
//!
//! ```no_run
//! use vcb_io::{Format, read_image, write_frame};
//! use vcb_core::Frame;
//!
//! # fn main() -> Result<(), vcb_io::Error> {
//! let scene = read_image("photo.jpg")?;
//! let frame = Frame::from_rgb(scene.width(), scene.height(), scene.to_srgb8());
//! write_frame("copy.png", &frame, Format::Png)?;
//! # Ok(())
//! # }
//! ```

mod error;
mod orientation;
mod read;
pub mod video;
mod write;

#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use orientation::{Orientation, orient};
#[doc(inline)]
pub use read::{Probe, probe_image, read_image};
#[doc(inline)]
pub use write::{Format, write_frame};
