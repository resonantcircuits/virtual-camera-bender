//! The linear-light input image.

use std::fmt::{self, Debug, Formatter};

use crate::plane::Plane;
use crate::scene::Channel;
use crate::srgb;

/// Linear-light RGB radiance, one plane per channel; 1.0 is reference white.
///
/// Values may exceed 1.0 to represent highlight headroom; negative values are not
/// physical and are treated as zero by the sensor.
///
/// # Examples
///
/// ```
/// use vcb_core::Scene;
///
/// let scene = Scene::from_srgb8(2, 1, &[255, 255, 255, 0, 0, 0]);
/// assert_eq!(scene.to_srgb8(), vec![255, 255, 255, 0, 0, 0]);
/// ```
#[derive(Clone, PartialEq)]
pub struct Scene {
    r: Plane<f32>,
    g: Plane<f32>,
    b: Plane<f32>,
}

impl Scene {
    /// Creates a black scene of `width` by `height` pixels.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            r: Plane::new(width, height),
            g: Plane::new(width, height),
            b: Plane::new(width, height),
        }
    }

    /// Decodes interleaved 8-bit sRGB bytes into linear light.
    ///
    /// # Panics
    ///
    /// Panics if `rgb.len()` is not `width * height * 3`.
    #[must_use]
    pub fn from_srgb8(width: u32, height: u32, rgb: &[u8]) -> Self {
        let expected = width as usize * height as usize * 3;
        assert_eq!(
            rgb.len(),
            expected,
            "sRGB data has {} bytes but {width} x {height} RGB needs {expected}",
            rgb.len()
        );
        let table = srgb::decode8_table();
        let mut scene = Self::new(width, height);
        let [r, g, b] = scene.planes_mut();
        for (((pixel, r), g), b) in rgb
            .as_chunks::<3>()
            .0
            .iter()
            .zip(r.data_mut())
            .zip(g.data_mut())
            .zip(b.data_mut())
        {
            *r = table[usize::from(pixel[0])];
            *g = table[usize::from(pixel[1])];
            *b = table[usize::from(pixel[2])];
        }
        scene
    }

    /// Encodes the scene as interleaved 8-bit sRGB bytes, clipping to `[0, 1]`.
    #[must_use]
    pub fn to_srgb8(&self) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(self.r.data().len() * 3);
        for ((r, g), b) in self.r.data().iter().zip(self.g.data()).zip(self.b.data()) {
            rgb.extend([srgb::encode8(*r), srgb::encode8(*g), srgb::encode8(*b)]);
        }
        rgb
    }

    /// Returns the width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.r.width()
    }

    /// Returns the height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.r.height()
    }

    /// Returns the plane of one channel.
    #[must_use]
    pub fn plane(&self, channel: Channel) -> &Plane<f32> {
        match channel {
            Channel::Red => &self.r,
            Channel::Green => &self.g,
            Channel::Blue => &self.b,
        }
    }

    /// Returns the plane of one channel mutably.
    pub fn plane_mut(&mut self, channel: Channel) -> &mut Plane<f32> {
        match channel {
            Channel::Red => &mut self.r,
            Channel::Green => &mut self.g,
            Channel::Blue => &mut self.b,
        }
    }

    /// Returns the red, green and blue planes.
    #[must_use]
    pub fn planes(&self) -> [&Plane<f32>; 3] {
        [&self.r, &self.g, &self.b]
    }

    /// Returns the red, green and blue planes mutably, for writing all three at once.
    pub fn planes_mut(&mut self) -> [&mut Plane<f32>; 3] {
        [&mut self.r, &mut self.g, &mut self.b]
    }
}

impl Debug for Scene {
    /// Prints the shape only; scenes hold millions of samples.
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scene")
            .field("width", &self.width())
            .field("height", &self.height())
            .finish_non_exhaustive()
    }
}
