//! A two-dimensional, row-major buffer of samples.
//!
//! [`Plane`] is the storage type of every image-shaped buffer in the crate: scene
//! channels, photosite charge, the raw Bayer plane. Its size is fixed at construction,
//! so a plane allocated once can be rewritten every frame without reallocating.
//!
//! # Examples
//!
//! ```
//! use vcb_core::plane::Plane;
//!
//! let mut plane = Plane::<f32>::new(4, 3);
//! plane.row_mut(1)[2] = 0.5;
//! assert_eq!(plane.get(2, 1), 0.5);
//! assert_eq!(plane.rows().count(), 3);
//! ```

use std::fmt::{self, Debug, Formatter};
use std::slice::{ChunksExact, ChunksExactMut};

/// A `width` by `height` grid of samples stored row by row.
#[derive(Clone, PartialEq)]
pub struct Plane<T> {
    width: u32,
    height: u32,
    data: Box<[T]>,
}

impl<T: Default + Clone> Plane<T> {
    /// Creates a plane filled with `T::default()`.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let len = width as usize * height as usize;
        Self {
            width,
            height,
            data: vec![T::default(); len].into_boxed_slice(),
        }
    }
}

impl<T> Plane<T> {
    /// Wraps row-major `data` as a `width` by `height` plane.
    ///
    /// # Panics
    ///
    /// Panics if `data.len()` is not `width * height`.
    #[must_use]
    pub fn from_vec(width: u32, height: u32, data: Vec<T>) -> Self {
        let expected = width as usize * height as usize;
        assert_eq!(
            data.len(),
            expected,
            "plane data has {} samples but {width} x {height} needs {expected}",
            data.len()
        );
        Self {
            width,
            height,
            data: data.into_boxed_slice(),
        }
    }

    /// Returns the number of columns.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the number of rows.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the sample at column `x`, row `y`.
    ///
    /// # Panics
    ///
    /// Panics if `x` or `y` lies outside the plane.
    #[must_use]
    pub fn get(&self, x: u32, y: u32) -> T
    where
        T: Copy,
    {
        self.data[self.index(x, y)]
    }

    /// Stores `value` at column `x`, row `y`.
    ///
    /// # Panics
    ///
    /// Panics if `x` or `y` lies outside the plane.
    pub fn set(&mut self, x: u32, y: u32, value: T) {
        let index = self.index(x, y);
        self.data[index] = value;
    }

    /// Returns row `y` as a slice of `width` samples.
    ///
    /// # Panics
    ///
    /// Panics if `y` is not below the height.
    #[must_use]
    pub fn row(&self, y: u32) -> &[T] {
        assert!(
            y < self.height,
            "row {y} is outside a plane of height {}",
            self.height
        );
        let start = y as usize * self.width as usize;
        &self.data[start..start + self.width as usize]
    }

    /// Returns row `y` as a mutable slice of `width` samples.
    ///
    /// # Panics
    ///
    /// Panics if `y` is not below the height.
    pub fn row_mut(&mut self, y: u32) -> &mut [T] {
        assert!(
            y < self.height,
            "row {y} is outside a plane of height {}",
            self.height
        );
        let start = y as usize * self.width as usize;
        &mut self.data[start..start + self.width as usize]
    }

    /// Iterates over the rows from top to bottom; a zero-width plane yields no rows.
    pub fn rows(&self) -> ChunksExact<'_, T> {
        self.data.chunks_exact(self.width.max(1) as usize)
    }

    /// Iterates mutably over the rows from top to bottom; a zero-width plane yields no rows.
    pub fn rows_mut(&mut self) -> ChunksExactMut<'_, T> {
        self.data.chunks_exact_mut(self.width.max(1) as usize)
    }

    /// Returns all samples in row-major order.
    #[must_use]
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// Returns all samples in row-major order, mutably.
    pub fn data_mut(&mut self) -> &mut [T] {
        &mut self.data
    }

    /// Sets every sample to `value`.
    pub fn fill(&mut self, value: T)
    where
        T: Clone,
    {
        self.data.fill(value);
    }

    fn index(&self, x: u32, y: u32) -> usize {
        assert!(
            x < self.width && y < self.height,
            "sample ({x}, {y}) is outside a {} x {} plane",
            self.width,
            self.height
        );
        y as usize * self.width as usize + x as usize
    }
}

impl<T> Debug for Plane<T> {
    /// Prints the shape only; planes hold millions of samples.
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plane")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}
