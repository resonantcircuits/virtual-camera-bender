//! BT.601 full-range `Y'CbCr`, as JPEG (JFIF) and camera DSPs use it.
//!
//! Values are on the 8-bit scale: Y' in `[0, 255]`, Cb and Cr centred on zero in
//! `[-127.5, 127.5]` (JPEG stores them offset by 128). The coefficients are libjpeg's
//! (`jccolor.c`, `jdcolor.c`).

/// Converts gamma-encoded `[r, g, b]` to `[y, cb, cr]`.
pub(crate) fn from_rgb([r, g, b]: [f32; 3]) -> [f32; 3] {
    [
        0.299 * r + 0.587 * g + 0.114 * b,
        -0.168_735_9 * r - 0.331_264_1 * g + 0.5 * b,
        0.5 * r - 0.418_687_6 * g - 0.081_312_4 * b,
    ]
}

/// Converts `[y, cb, cr]` to gamma-encoded `[r, g, b]`, without clipping.
pub(crate) fn to_rgb([y, cb, cr]: [f32; 3]) -> [f32; 3] {
    [
        1.402f32.mul_add(cr, y),
        y - 0.344_136_3 * cb - 0.714_136_3 * cr,
        1.772f32.mul_add(cb, y),
    ]
}
