//! EXIF orientation and the transforms that make an image upright.

use std::fmt::{self, Display, Formatter};

use vcb_core::Scene;

/// How a stored image must be transformed to display upright (EXIF tag 0x0112).
///
/// The variant names describe the transform to apply, in the order of the EXIF
/// values 1 to 8.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Value 1: already upright.
    #[default]
    Normal,
    /// Value 2: mirror left to right.
    MirrorHorizontal,
    /// Value 3: rotate by 180 degrees.
    Rotate180,
    /// Value 4: mirror top to bottom.
    MirrorVertical,
    /// Value 5: swap rows and columns (mirror across the main diagonal).
    Transpose,
    /// Value 6: rotate 90 degrees clockwise.
    Rotate90,
    /// Value 7: mirror across the anti-diagonal.
    Transverse,
    /// Value 8: rotate 90 degrees counter-clockwise.
    Rotate270,
}

impl Orientation {
    /// Every orientation in EXIF order, values 1 to 8.
    pub const ALL: [Self; 8] = [
        Self::Normal,
        Self::MirrorHorizontal,
        Self::Rotate180,
        Self::MirrorVertical,
        Self::Transpose,
        Self::Rotate90,
        Self::Transverse,
        Self::Rotate270,
    ];

    /// Returns the orientation of an EXIF value, or `None` outside 1 to 8.
    #[must_use]
    pub fn from_exif(value: u32) -> Option<Self> {
        let index = usize::try_from(value).ok()?.checked_sub(1)?;
        Self::ALL.get(index).copied()
    }

    /// Returns the EXIF value, 1 to 8.
    #[must_use]
    pub fn exif(self) -> u32 {
        match self {
            Self::Normal => 1,
            Self::MirrorHorizontal => 2,
            Self::Rotate180 => 3,
            Self::MirrorVertical => 4,
            Self::Transpose => 5,
            Self::Rotate90 => 6,
            Self::Transverse => 7,
            Self::Rotate270 => 8,
        }
    }

    /// Returns `true` if the upright image has width and height exchanged.
    #[must_use]
    pub fn swaps_axes(self) -> bool {
        matches!(
            self,
            Self::Transpose | Self::Rotate90 | Self::Transverse | Self::Rotate270
        )
    }

    /// Maps stored pixel `(x, y)` of a `width` by `height` image to its upright position.
    fn place(self, x: u32, y: u32, width: u32, height: u32) -> (u32, u32) {
        let (right, bottom) = (width - 1 - x, height - 1 - y);
        match self {
            Self::Normal => (x, y),
            Self::MirrorHorizontal => (right, y),
            Self::Rotate180 => (right, bottom),
            Self::MirrorVertical => (x, bottom),
            Self::Transpose => (y, x),
            Self::Rotate90 => (bottom, x),
            Self::Transverse => (bottom, right),
            Self::Rotate270 => (y, right),
        }
    }
}

impl Display for Orientation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Normal => "normal",
            Self::MirrorHorizontal => "mirror horizontal",
            Self::Rotate180 => "rotate 180",
            Self::MirrorVertical => "mirror vertical",
            Self::Transpose => "transpose",
            Self::Rotate90 => "rotate 90 clockwise",
            Self::Transverse => "transverse",
            Self::Rotate270 => "rotate 90 counter-clockwise",
        };
        write!(f, "{name} (EXIF {})", self.exif())
    }
}

/// Returns `scene` transformed so that a stored image with `orientation` is upright.
///
/// # Examples
///
/// ```
/// use vcb_core::Scene;
/// use vcb_io::{Orientation, orient};
///
/// let scene = Scene::new(3, 2);
/// let upright = orient(&scene, Orientation::Rotate90);
/// assert_eq!((upright.width(), upright.height()), (2, 3));
/// ```
#[must_use]
pub fn orient(scene: &Scene, orientation: Orientation) -> Scene {
    let (width, height) = (scene.width(), scene.height());
    if orientation == Orientation::Normal {
        return scene.clone();
    }
    let (out_w, out_h) = if orientation.swaps_axes() {
        (height, width)
    } else {
        (width, height)
    };
    let mut upright = Scene::new(out_w, out_h);
    for (src, dst) in scene.planes().into_iter().zip(upright.planes_mut()) {
        for (y, row) in (0..height).zip(src.rows()) {
            for (x, &value) in (0..width).zip(row) {
                let (ux, uy) = orientation.place(x, y, width, height);
                dst.set(ux, uy, value);
            }
        }
    }
    upright
}
