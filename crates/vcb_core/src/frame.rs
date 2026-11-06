//! The camera's 8-bit output image.

use std::fmt::{self, Debug, Formatter};

/// An 8-bit sRGB image with interleaved `R, G, B` bytes, the camera's output.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    width: u32,
    height: u32,
    rgb: Box<[u8]>,
}

impl Frame {
    /// Creates a black frame of `width` by `height` pixels.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let len = width as usize * height as usize * 3;
        Self {
            width,
            height,
            rgb: vec![0; len].into_boxed_slice(),
        }
    }

    /// Wraps interleaved RGB bytes as a frame.
    ///
    /// # Panics
    ///
    /// Panics if `rgb.len()` is not `width * height * 3`.
    #[must_use]
    pub fn from_rgb(width: u32, height: u32, rgb: Vec<u8>) -> Self {
        let expected = width as usize * height as usize * 3;
        assert_eq!(
            rgb.len(),
            expected,
            "frame data has {} bytes but {width} x {height} RGB needs {expected}",
            rgb.len()
        );
        Self {
            width,
            height,
            rgb: rgb.into_boxed_slice(),
        }
    }

    /// Returns the width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the interleaved RGB bytes, row by row.
    #[must_use]
    pub fn rgb(&self) -> &[u8] {
        &self.rgb
    }

    /// Returns the interleaved RGB bytes mutably.
    pub fn rgb_mut(&mut self) -> &mut [u8] {
        &mut self.rgb
    }

    /// Returns the `[r, g, b]` bytes of the pixel at column `x`, row `y`.
    ///
    /// # Panics
    ///
    /// Panics if `x` or `y` lies outside the frame.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        assert!(
            x < self.width && y < self.height,
            "pixel ({x}, {y}) is outside a {} x {} frame",
            self.width,
            self.height
        );
        let i = (y as usize * self.width as usize + x as usize) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }
}

/// Writes `src` turned 90 degrees counter-clockwise into `dst`.
///
/// # Panics
///
/// Panics if `dst` is not `src.height()` by `src.width()`.
pub(crate) fn rotate_ccw(src: &Frame, dst: &mut Frame) {
    assert!(
        dst.width == src.height && dst.height == src.width,
        "cannot turn a {} x {} frame into {} x {}",
        src.width,
        src.height,
        dst.width,
        dst.height
    );
    let src_w = src.width as usize;
    let dst_w = dst.width as usize;
    for (y, row) in dst.rgb.chunks_exact_mut(dst_w * 3).enumerate() {
        let sx = src_w - 1 - y;
        for (x, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            let i = (x * src_w + sx) * 3;
            pixel.copy_from_slice(&src.rgb[i..i + 3]);
        }
    }
}

impl Debug for Frame {
    /// Prints the shape only; frames hold millions of bytes.
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}
