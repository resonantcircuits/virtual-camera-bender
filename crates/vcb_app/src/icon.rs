//! The app icon, drawn in code: a lens on a dark tile, its glass crossed by a slipped line.
//!
//! The icon is computed per pixel from distances to its shapes, with one pixel of
//! anti-aliasing at every edge, so it is sharp at every size from 16 to 1024 pixels.
//! [`rgba`] returns the pixels; the window uses them through [`icon_data`], and
//! `examples/icon.rs` writes the PNG set from which `scripts/bundle-macos.sh` builds
//! the app bundle's `.icns` file. Colours come from [`theme`].

use eframe::egui::{Color32, IconData};

use crate::theme;

/// Margin around the tile as a fraction of the icon, after Apple's icon grid (100 of 1024).
const MARGIN: f32 = 100.0 / 1024.0;

/// Corner radius of the tile as a fraction of the icon (185 of 1024).
const CORNER: f32 = 185.0 / 1024.0;

/// Centre of the lens, as fractions of the icon.
const LENS: (f32, f32) = (0.5, 0.5);

/// Outer radius of the lens barrel as a fraction of the icon.
const BARREL: f32 = 0.27;

/// Radius of the glass as a fraction of the icon.
const GLASS: f32 = 0.215;

/// Radius and half-width of the accent ring on the glass, as fractions of the icon.
const RING: (f32, f32) = (0.15, 0.012);

/// Horizontal bands across the glass: centre height, half-height and shift, as fractions.
///
/// The shift moves the ring inside the band sideways, as a horizontal clock glitch
/// shifts a band of lines.
const BANDS: [(f32, f32, f32); 2] = [(0.455, 0.022, 0.045), (0.56, 0.012, -0.03)];

/// The icon's pixels: `size` by `size`, RGBA with straight alpha, rows top to bottom.
#[must_use]
pub fn rgba(size: u32) -> Vec<u8> {
    let n = size.max(1) as f32;
    let px = 1.0 / n;
    let mut out = Vec::with_capacity(size as usize * size as usize * 4);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = ((x as f32 + 0.5) / n, (y as f32 + 0.5) / n);
            out.extend_from_slice(&pixel(u, v, px));
        }
    }
    out
}

/// The icon as the window's icon, `size` by `size` pixels.
#[must_use]
pub fn icon_data(size: u32) -> IconData {
    IconData {
        rgba: rgba(size),
        width: size,
        height: size,
    }
}

/// The colour at icon coordinates `(u, v)`, with `px` the size of a pixel.
fn pixel(u: f32, v: f32, px: f32) -> [u8; 4] {
    let tile = coverage(rounded_square(u, v), px);
    if tile <= 0.0 {
        return [0; 4];
    }
    // The tile: a vertical gradient from a lighter to a darker grey.
    let top = Color32::from_gray(0x34);
    let bottom = Color32::from_gray(0x1A);
    let mut colour = mix(top, bottom, v);
    let (dx, dy) = (u - LENS.0, v - LENS.1);
    let r = dx.hypot(dy);
    colour = over(colour, theme::LINE, coverage(r - BARREL, px));
    colour = over(colour, theme::SLOT, coverage(r - GLASS, px));
    // A faint highlight in the upper left of the glass.
    let glint = (1.0 - ((dx + 0.07).hypot(dy + 0.07) / 0.09)).clamp(0.0, 1.0) * 0.18;
    colour = over(colour, Color32::WHITE, glint * coverage(r - GLASS, px));
    let shift = BANDS
        .iter()
        .find(|(centre, half, _)| (v - centre).abs() < *half)
        .map_or(0.0, |(_, _, shift)| *shift);
    let ring = (dx - shift).hypot(dy);
    let on_ring = coverage((ring - RING.0).abs() - RING.1, px) * coverage(r - GLASS, px);
    colour = over(colour, theme::ACCENT, on_ring);
    let [red, green, blue, _] = colour.to_array();
    [red, green, blue, (tile * 255.0).round() as u8]
}

/// Signed distance from `(u, v)` to the edge of the tile; negative inside.
fn rounded_square(u: f32, v: f32) -> f32 {
    let half = 0.5 - MARGIN;
    let qx = (u - 0.5).abs() - (half - CORNER);
    let qy = (v - 0.5).abs() - (half - CORNER);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - CORNER
}

/// Coverage of a pixel by the inside of a shape at signed distance `distance`.
fn coverage(distance: f32, px: f32) -> f32 {
    (0.5 - distance / px).clamp(0.0, 1.0)
}

/// Linear blend of two colours, `t = 0` giving `a`.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
    )
}

/// `top` painted over `base` with opacity `alpha`.
fn over(base: Color32, top: Color32, alpha: f32) -> Color32 {
    mix(base, Color32::from_rgb(top.r(), top.g(), top.b()), alpha)
}
