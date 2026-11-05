//! The 8 by 8 discrete cosine transform of baseline JPEG, in floating point.
//!
//! JPEG's forward DCT, `F(u, v) = 1/4 C(u) C(v) sum f(x, y) cos((2x + 1) u pi / 16)
//! cos((2y + 1) v pi / 16)` with `C(0) = 1 / sqrt(2)`, is the orthonormal 2-D DCT-II,
//! so the inverse is its transpose. Both run separably as two 8 by 8 matrix products.

/// An 8 by 8 block of samples or coefficients; `[v][u]` is row `v`, column `u`.
pub(super) type Block = [[f32; 8]; 8];

/// The orthonormal 8-point DCT-II matrix: `m[k][n] = c(k) cos((2n + 1) k pi / 16)`.
#[derive(Debug, Clone, Copy)]
pub(super) struct Dct {
    m: Block,
}

impl Dct {
    /// Computes the transform matrix.
    pub(super) fn new() -> Self {
        let m = std::array::from_fn(|k| {
            let scale = if k == 0 {
                (1.0f64 / 8.0).sqrt()
            } else {
                (2.0f64 / 8.0).sqrt()
            };
            std::array::from_fn(|n| {
                let angle = (2 * n + 1) as f64 * k as f64 * std::f64::consts::PI / 16.0;
                (scale * angle.cos()) as f32
            })
        });
        Self { m }
    }

    /// Returns the coefficients of a block of samples: `M B M^T`.
    pub(super) fn forward(&self, block: &Block) -> Block {
        let m = &self.m;
        let mut columns = [[0.0f32; 8]; 8];
        for (v, out) in columns.iter_mut().enumerate() {
            for (y, row) in block.iter().enumerate() {
                let weight = m[v][y];
                for (o, &s) in out.iter_mut().zip(row) {
                    *o += weight * s;
                }
            }
        }
        let mut coefficients = [[0.0f32; 8]; 8];
        for (out, row) in coefficients.iter_mut().zip(&columns) {
            for (o, basis) in out.iter_mut().zip(m) {
                *o = row.iter().zip(basis).map(|(&s, &b)| s * b).sum();
            }
        }
        coefficients
    }

    /// Returns the samples of a block of coefficients: `M^T F M`.
    pub(super) fn inverse(&self, coefficients: &Block) -> Block {
        let m = &self.m;
        let mut rows = [[0.0f32; 8]; 8];
        for (y, out) in rows.iter_mut().enumerate() {
            for (v, row) in coefficients.iter().enumerate() {
                let weight = m[v][y];
                for (o, &c) in out.iter_mut().zip(row) {
                    *o += weight * c;
                }
            }
        }
        let mut block = [[0.0f32; 8]; 8];
        for (out, row) in block.iter_mut().zip(&rows) {
            for (x, o) in out.iter_mut().enumerate() {
                *o = row.iter().zip(m).map(|(&c, basis)| c * basis[x]).sum();
            }
        }
        block
    }
}
