//! Encoding camera frames into image files.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use vcb_core::Frame;

use crate::Error;

/// JPEG quality used when a format is chosen from a file extension.
///
/// High enough that the container adds little on top of the simulated camera codec.
const DEFAULT_JPEG_QUALITY: u8 = 95;

/// File format of a written frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// Lossless PNG, 8 bits per channel.
    Png,
    /// Baseline JPEG.
    Jpeg {
        /// Quality factor, 1 to 100.
        quality: u8,
    },
}

impl Format {
    /// Chooses the format from a path's extension: `png`, `jpg` or `jpeg`.
    ///
    /// JPEG uses quality 95. Returns `None` for other extensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use vcb_io::Format;
    ///
    /// assert_eq!(Format::from_path("out.PNG"), Some(Format::Png));
    /// assert_eq!(Format::from_path("out.tif"), None);
    /// ```
    #[must_use]
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        let extension = path.as_ref().extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg {
                quality: DEFAULT_JPEG_QUALITY,
            }),
            _ => None,
        }
    }
}

/// Writes `frame` to `path` in `format`, replacing any existing file.
///
/// # Errors
///
/// Returns an I/O error if the file cannot be created or written and an encode error
/// if the encoder rejects the frame.
pub fn write_frame(path: impl AsRef<Path>, frame: &Frame, format: Format) -> Result<(), Error> {
    let path = path.as_ref();
    let file = File::create(path).map_err(|source| Error::io(path, source))?;
    let mut writer = BufWriter::new(file);
    let (width, height) = (frame.width(), frame.height());
    let encoded = match format {
        Format::Png => PngEncoder::new(&mut writer).write_image(
            frame.rgb(),
            width,
            height,
            ExtendedColorType::Rgb8,
        ),
        Format::Jpeg { quality } => JpegEncoder::new_with_quality(&mut writer, quality)
            .write_image(frame.rgb(), width, height, ExtendedColorType::Rgb8),
    };
    encoded.map_err(|source| Error::encode(path, source))?;
    writer.flush().map_err(|source| Error::io(path, source))?;
    tracing::debug!(name: "io.write.done", path = %path.display(), width, height, "frame written");
    Ok(())
}
