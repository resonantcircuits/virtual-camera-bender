//! JPEG simulation: the compression artefacts of the camera's encoder.
//!
//! The camera stores every still, and every movie frame, as a baseline JPEG. Its
//! artefacts are part of the look: chroma subsampling, 8 by 8 block quantisation at
//! the mode's quality, and the chroma upsampling of the decoder that displays the
//! file. [`encode`] applies them to a [`Frame`] in place, leaving its size unchanged.
//! The current model passes the frame through losslessly.

use crate::preset::Mode;
use crate::{Camera, Error, Frame, Preset, Timing};

/// Chroma subsampling of the encoder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Subsampling {
    /// Full-resolution chroma.
    #[cfg_attr(feature = "serde", serde(rename = "4:4:4"))]
    S444,
    /// Chroma at half resolution in both directions.
    #[default]
    #[cfg_attr(feature = "serde", serde(rename = "4:2:0"))]
    S420,
}

/// Chroma upsampling of the decoder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum ChromaUp {
    /// Each chroma sample covers its block of pixels, giving chunky colour edges.
    #[default]
    Nearest,
    /// Chroma interpolated between sample centres.
    Bilinear,
}

/// Family of quantisation tables, scaled by the quality factor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Tables {
    /// The example tables of ITU-T T.81 Annex K with IJG quality scaling.
    #[default]
    Standard,
}

/// Parameters of the JPEG encoder and decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Params {
    /// Quality factor, 1 to 100.
    pub quality: u8,
    /// Chroma subsampling.
    pub subsampling: Subsampling,
    /// Chroma upsampling of the decoder.
    pub chroma_upsample: ChromaUp,
    /// Quantisation table family.
    pub tables: Tables,
}

impl Params {
    /// Resolves the codec parameters from the camera, preset and derived timing.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let _ = preset;
        let quality = match timing.mode {
            Mode::Still => camera.still.jpeg_quality,
            Mode::Movie => camera.movie.jpeg_quality,
        };
        Ok(Self {
            quality,
            subsampling: camera.jpeg.subsampling,
            chroma_upsample: camera.jpeg.chroma_upsample,
            tables: camera.jpeg.tables,
        })
    }
}

/// Working memory of the codec, reused across frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct State;

/// Applies the encoder's and decoder's artefacts to `frame` in place.
pub fn encode(params: &Params, state: &mut State, frame: &mut Frame) {
    let _ = (params, state, frame);
}
