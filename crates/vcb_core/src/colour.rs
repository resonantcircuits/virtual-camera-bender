//! Colour matrices between linear sRGB and camera RGB, derived as dcraw does.
//!
//! A camera file gives `xyz_to_cam`, the matrix from CIE XYZ (D65) to the camera's raw
//! RGB, in the convention of dcraw's `adobe_coeff` table. Following dcraw's
//! `cam_xyz_coeff`, the matrix from linear sRGB to camera RGB is
//! `xyz_to_cam * SRGB_TO_XYZ`, with each row then normalised to sum to one so that
//! sRGB white maps to camera white (the white balance is applied separately, as gains).
//! Its inverse takes white-balanced camera RGB back to linear sRGB.

use crate::Error;

/// Linear sRGB (Rec. 709 primaries, D65 white) to CIE XYZ, as in dcraw's `xyz_rgb`.
const SRGB_TO_XYZ: [[f64; 3]; 3] = [
    [0.412_453, 0.357_580, 0.180_423],
    [0.212_671, 0.715_160, 0.072_169],
    [0.019_334, 0.119_193, 0.950_227],
];

/// Smallest determinant accepted as invertible; camera matrices are of order 0.1 to 1.
const MIN_DETERMINANT: f64 = 1e-9;

/// Returns the row-normalised matrix from linear sRGB to white-balanced camera RGB.
///
/// # Errors
///
/// Returns a configuration error if the matrix maps white to a non-positive channel
/// or is not invertible.
pub(crate) fn srgb_to_cam(xyz_to_cam: &[[f64; 3]; 3]) -> Result<[[f64; 3]; 3], Error> {
    let mut cam = multiply(xyz_to_cam, &SRGB_TO_XYZ);
    for (index, row) in cam.iter_mut().enumerate() {
        let sum: f64 = row.iter().sum();
        if !(sum.is_finite() && sum > 0.0) {
            return Err(Error::config(format!(
                "xyz_to_cam maps white to {sum} in camera channel {index}; it must be positive"
            )));
        }
        for value in row.iter_mut() {
            *value /= sum;
        }
    }
    if determinant(&cam).abs() < MIN_DETERMINANT {
        return Err(Error::config("xyz_to_cam is not invertible"));
    }
    Ok(cam)
}

/// Returns the matrix from white-balanced camera RGB to linear sRGB.
///
/// # Errors
///
/// Returns a configuration error under the conditions of [`srgb_to_cam`].
pub(crate) fn cam_to_srgb(xyz_to_cam: &[[f64; 3]; 3]) -> Result<[[f64; 3]; 3], Error> {
    let forward = srgb_to_cam(xyz_to_cam)?;
    Ok(invert(&forward))
}

/// Narrows a matrix to `f32` for per-pixel use.
pub(crate) fn to_f32(m: &[[f64; 3]; 3]) -> [[f32; 3]; 3] {
    m.map(|row| row.map(|value| value as f32))
}

/// Applies a 3 by 3 matrix to one RGB triple.
pub(crate) fn apply(m: &[[f32; 3]; 3], rgb: [f32; 3]) -> [f32; 3] {
    m.map(|row| row[0].mul_add(rgb[0], row[1].mul_add(rgb[1], row[2] * rgb[2])))
}

fn multiply(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn determinant(m: &[[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// Inverts a matrix whose determinant has been checked against [`MIN_DETERMINANT`].
fn invert(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = determinant(m);
    let cofactor =
        |r0: usize, r1: usize, c0: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    [
        [
            cofactor(1, 2, 1, 2) / det,
            -cofactor(0, 2, 1, 2) / det,
            cofactor(0, 1, 1, 2) / det,
        ],
        [
            -cofactor(1, 2, 0, 2) / det,
            cofactor(0, 2, 0, 2) / det,
            -cofactor(0, 1, 0, 2) / det,
        ],
        [
            cofactor(1, 2, 0, 1) / det,
            -cofactor(0, 2, 0, 1) / det,
            cofactor(0, 1, 0, 1) / det,
        ],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const S60: [[f64; 3]; 3] = [
        [0.8795, -0.2482, -0.0797],
        [-0.7804, 1.5403, 0.2573],
        [-0.1422, 0.1996, 0.7082],
    ];

    #[test]
    fn forward_and_inverse_compose_to_identity() {
        let forward = srgb_to_cam(&S60).unwrap();
        let product = multiply(&forward, &invert(&forward));
        for (i, row) in product.iter().enumerate() {
            for (j, value) in row.iter().enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((value - expected).abs() < 1e-12, "[{i}][{j}] = {value}");
            }
        }
    }

    #[test]
    fn white_maps_to_white_both_ways() {
        let forward = srgb_to_cam(&S60).unwrap();
        let inverse = cam_to_srgb(&S60).unwrap();
        for m in [forward, inverse] {
            for row in m {
                assert!((row.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn singular_matrices_are_rejected() {
        let singular = [[1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [0.0, 0.0, 1.0]];
        assert!(srgb_to_cam(&singular).unwrap_err().is_config());
    }
}
