//! JPEG simulation: the compression artefacts of the camera's encoder.
//!
//! The camera stores every still, and every movie frame, as a baseline JPEG. Its
//! artefacts are part of the look: chroma subsampling, 8 by 8 block quantisation with
//! the camera's own tables for the selected quality setting, and the chroma
//! upsampling of the decoder that displays the file. [`encode`] applies them to a
//! [`Frame`] in place, leaving its size unchanged. The current model passes the frame
//! through losslessly.

use crate::preset::Mode;
use crate::{Camera, Error, Frame, Preset, Timing};

/// Chroma subsampling of the encoder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Subsampling {
    /// Full-resolution chroma.
    #[cfg_attr(feature = "serde", serde(rename = "4:4:4"))]
    S444,
    /// Chroma at half resolution horizontally, full resolution vertically.
    #[default]
    #[cfg_attr(feature = "serde", serde(rename = "4:2:2"))]
    S422,
    /// Chroma at half resolution in both directions.
    #[cfg_attr(feature = "serde", serde(rename = "4:2:0"))]
    S420,
}

/// Chroma upsampling of the decoder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum ChromaUp {
    /// Each chroma sample covers its block of pixels, giving chunky colour edges.
    Nearest,
    /// Chroma interpolated between sample centres.
    #[default]
    Bilinear,
}

/// The camera's JPEG quality setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Quality {
    /// The finest setting.
    #[default]
    Superfine,
    /// The middle setting.
    Fine,
    /// The coarsest setting.
    Normal,
}

/// One luma and one chroma quantisation table, rows of 8 in natural order.
///
/// Entry `[v][u]` is the quantiser step of vertical frequency `v` and horizontal
/// frequency `u`; `[0][0]` is the DC step.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
pub struct QuantTables {
    /// Quantiser steps of the luma channel.
    pub luma: [[u16; 8]; 8],
    /// Quantiser steps of both chroma channels.
    pub chroma: [[u16; 8]; 8],
}

/// Parameters of the JPEG encoder and decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Params {
    /// Quality setting of the readout mode.
    pub quality: Quality,
    /// Quantisation tables of that setting.
    pub tables: QuantTables,
    /// Chroma subsampling.
    pub subsampling: Subsampling,
    /// Chroma upsampling of the decoder.
    pub chroma_upsample: ChromaUp,
}

impl Params {
    /// Resolves the codec parameters from the camera, preset and derived timing.
    ///
    /// The still and movie sections each select a quality setting; its tables come
    /// from the camera's `[jpeg.tables]`.
    ///
    /// # Errors
    ///
    /// Returns a configuration error if a value is out of range.
    pub fn resolve(camera: &Camera, preset: &Preset, timing: &Timing) -> Result<Self, Error> {
        let _ = preset;
        let quality = match timing.mode {
            Mode::Still => camera.still.jpeg,
            Mode::Movie => camera.movie.jpeg,
        };
        Ok(Self {
            quality,
            tables: camera.jpeg.tables.get(quality).clone(),
            subsampling: camera.jpeg.subsampling,
            chroma_upsample: camera.jpeg.chroma_upsample,
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
