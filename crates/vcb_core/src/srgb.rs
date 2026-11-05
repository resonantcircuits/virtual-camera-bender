//! The sRGB transfer function (IEC 61966-2-1) between encoded values and linear light.

/// Linear-light value below which the sRGB curve is a straight line.
const LINEAR_KNEE: f32 = 0.003_130_8;

/// Encoded value below which the sRGB curve is a straight line (`LINEAR_KNEE * 12.92`).
const ENCODED_KNEE: f32 = 0.040_45;

/// Converts an sRGB-encoded value in `[0, 1]` to linear light.
pub(crate) fn decode(encoded: f32) -> f32 {
    if encoded <= ENCODED_KNEE {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// Converts linear light to an sRGB-encoded value, clipping to `[0, 1]`.
pub(crate) fn encode(linear: f32) -> f32 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= LINEAR_KNEE {
        linear * 12.92
    } else {
        1.055f32.mul_add(linear.powf(1.0 / 2.4), -0.055)
    }
}

/// Converts linear light to an 8-bit sRGB code, rounding to nearest.
pub(crate) fn encode8(linear: f32) -> u8 {
    (encode(linear) * 255.0).round() as u8
}

/// Returns the linear-light value of every 8-bit sRGB code.
pub(crate) fn decode8_table() -> [f32; 256] {
    std::array::from_fn(|code| decode(code as f32 / 255.0))
}
