//! Decoding image files into upright linear-light scenes.

use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Seek};
use std::path::Path;

use vcb_core::Scene;

use crate::Error;
use crate::orientation::{Orientation, orient};

/// Size and orientation of an image file, read without decoding the pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// Width of the upright image, after applying the orientation.
    pub width: u32,
    /// Height of the upright image, after applying the orientation.
    pub height: u32,
    /// The orientation recorded in the file's EXIF data.
    pub orientation: Orientation,
}

/// Decodes a JPEG or PNG file into an upright linear-light scene.
///
/// The EXIF orientation is applied, so the scene's size is the displayed size.
///
/// # Errors
///
/// Returns an I/O error if the file cannot be read and a decode error if it is not a
/// supported image.
pub fn read_image(path: impl AsRef<Path>) -> Result<Scene, Error> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| Error::io(path, source))?;
    let orientation = exif_orientation(&mut Cursor::new(&bytes));
    let decoded = image::load_from_memory(&bytes).map_err(|source| Error::decode(path, source))?;
    let rgb = decoded.to_rgb8();
    let stored = Scene::from_srgb8(rgb.width(), rgb.height(), rgb.as_raw());
    tracing::debug!(
        name: "io.read.done",
        path = %path.display(),
        width = stored.width(),
        height = stored.height(),
        orientation = orientation.exif(),
        "image decoded"
    );
    Ok(orient(&stored, orientation))
}

/// Reads the upright size and the orientation of an image file.
///
/// # Errors
///
/// Returns an I/O error if the file cannot be read and a decode error if its header is
/// not a supported image.
pub fn probe_image(path: impl AsRef<Path>) -> Result<Probe, Error> {
    let path = path.as_ref();
    let (width, height) = image::ImageReader::open(path)
        .and_then(image::ImageReader::with_guessed_format)
        .map_err(|source| Error::io(path, source))?
        .into_dimensions()
        .map_err(|source| Error::decode(path, source))?;
    let file = File::open(path).map_err(|source| Error::io(path, source))?;
    let orientation = exif_orientation(&mut BufReader::new(file));
    let (width, height) = if orientation.swaps_axes() {
        (height, width)
    } else {
        (width, height)
    };
    Ok(Probe {
        width,
        height,
        orientation,
    })
}

/// Reads the EXIF orientation of an image container; upright if absent or unreadable.
fn exif_orientation(reader: &mut (impl BufRead + Seek)) -> Orientation {
    let exif = match exif::Reader::new().read_from_container(reader) {
        Ok(exif) => exif,
        Err(error) => {
            tracing::trace!(name: "io.exif.missing", %error, "no usable EXIF data");
            return Orientation::Normal;
        }
    };
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|field| field.value.get_uint(0))
        .and_then(Orientation::from_exif)
        .unwrap_or_default()
}
