//! Demosaicing: the two missing colours of every Bayer site.
//!
//! Both methods read the white-balanced mosaic mirrored at its borders with period-two
//! symmetry, so a mirrored site keeps its colour, and clip every interpolated value at
//! the saturation level. Balancing first makes a grey scene a flat mosaic, which the
//! gradient correction relies on.
//!
//! - Bilinear: the mean of the nearest sites of the wanted colour.
//! - Malvar, He and Cutler, "High-quality linear interpolation for demosaicing of
//!   Bayer-patterned color images" (ICASSP 2004): bilinear plus a correction by the
//!   Laplacian of the site's own colour, in 5 by 5 kernels with weights in eighths.
//!   The correction follows luminance edges, so it cuts the zipper and false colour
//!   bilinear interpolation leaves along them, at the price of small overshoots.

use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;

use crate::Scene;
use crate::plane::Plane;
use crate::scene::Channel;
use crate::sensor::Cfa;

/// Offsets of the rows and columns of a 5 by 5 window from its centre.
const OFFSETS: [i64; 5] = [-2, -1, 0, 1, 2];

/// Mirrors index `i` into `[0, n)` with period-two symmetry, preserving the Bayer phase.
fn reflect(i: i64, n: u32) -> usize {
    let n = i64::from(n);
    if n <= 1 {
        return 0;
    }
    let period = 2 * (n - 1);
    let m = i.rem_euclid(period);
    (if m >= n { period - m } else { m }) as usize
}

/// The 5 by 5 neighbourhood of one site, read with mirrored borders.
struct Window<'a> {
    rows: [&'a [f32]; 5],
    cols: [usize; 5],
}

impl Window<'_> {
    /// Returns the site `dy` rows and `dx` columns away from the centre.
    fn at(&self, dy: i32, dx: i32) -> f32 {
        self.rows[(dy + 2) as usize][self.cols[(dx + 2) as usize]]
    }

    /// Sum of the four sites at distance `d` along the rows and columns.
    fn cross(&self, d: i32) -> f32 {
        self.at(0, -d) + self.at(0, d) + self.at(-d, 0) + self.at(d, 0)
    }

    /// Sum of the two sites at distance `d` in the same row.
    fn across(&self, d: i32) -> f32 {
        self.at(0, -d) + self.at(0, d)
    }

    /// Sum of the two sites at distance `d` in the same column.
    fn vertical(&self, d: i32) -> f32 {
        self.at(-d, 0) + self.at(d, 0)
    }

    /// Sum of the four diagonal neighbours.
    fn diagonal(&self) -> f32 {
        self.at(-1, -1) + self.at(-1, 1) + self.at(1, -1) + self.at(1, 1)
    }
}

/// Runs `site(window, channel, red_row)` for every site, writing gained and clipped RGB.
///
/// `red_row` tells a green site whether its row holds red (true) or blue sites.
fn run(
    raw: &Plane<f32>,
    cfa: Cfa,
    saturation: f32,
    out: &mut Scene,
    site: impl Fn(&Window<'_>, Channel, bool) -> [f32; 3] + Sync,
) {
    let (width, height) = (raw.width(), raw.height());
    let [red, green, blue] = out.planes_mut();
    let row_len = width as usize;
    red.data_mut()
        .par_chunks_mut(row_len)
        .zip(green.data_mut().par_chunks_mut(row_len))
        .zip(blue.data_mut().par_chunks_mut(row_len))
        .enumerate()
        .for_each(|(y, ((red_row, green_row), blue_row))| {
            let y = y as u32;
            let rows = OFFSETS.map(|d| raw.row(reflect(i64::from(y) + d, height) as u32));
            for x in 0..width {
                let window = Window {
                    rows,
                    cols: OFFSETS.map(|d| reflect(i64::from(x) + d, width)),
                };
                let channel = cfa.channel(x, y);
                let red_in_row = cfa.channel(x ^ 1, y) == Channel::Red;
                let rgb = site(&window, channel, red_in_row);
                let c = x as usize;
                red_row[c] = rgb[0].min(saturation);
                green_row[c] = rgb[1].min(saturation);
                blue_row[c] = rgb[2].min(saturation);
            }
        });
}

/// Interpolates every channel bilinearly, clipping at `saturation`.
pub(super) fn bilinear(raw: &Plane<f32>, cfa: Cfa, saturation: f32, out: &mut Scene) {
    run(raw, cfa, saturation, out, |w, channel, red_row| {
        let here = w.at(0, 0);
        let cross = 0.25 * w.cross(1);
        let diagonal = 0.25 * w.diagonal();
        let across = 0.5 * w.across(1);
        let vertical = 0.5 * w.vertical(1);
        match channel {
            Channel::Red => [here, cross, diagonal],
            Channel::Blue => [diagonal, cross, here],
            Channel::Green if red_row => [across, here, vertical],
            Channel::Green => [vertical, here, across],
        }
    });
}

/// Interpolates every channel with Malvar-He-Cutler kernels, clipping at `saturation`.
pub(super) fn malvar(raw: &Plane<f32>, cfa: Cfa, saturation: f32, out: &mut Scene) {
    run(raw, cfa, saturation, out, |w, channel, red_row| {
        let here = w.at(0, 0);
        // Green at a red or blue site.
        let green = || (4.0 * here + 2.0 * w.cross(1) - w.cross(2)) * 0.125;
        // Red at a blue site, or blue at a red site.
        let opposite = || (6.0 * here + 2.0 * w.diagonal() - 1.5 * w.cross(2)) * 0.125;
        // At a green site: the colour of the row neighbours, and of the column neighbours.
        let row_colour = || {
            (5.0 * here + 4.0 * w.across(1) - w.diagonal() - w.across(2) + 0.5 * w.vertical(2))
                * 0.125
        };
        let column_colour = || {
            (5.0 * here + 4.0 * w.vertical(1) - w.diagonal() - w.vertical(2) + 0.5 * w.across(2))
                * 0.125
        };
        match channel {
            Channel::Red => [here, green(), opposite()],
            Channel::Blue => [opposite(), green(), here],
            Channel::Green if red_row => [row_colour(), here, column_colour()],
            Channel::Green => [column_colour(), here, row_colour()],
        }
    });
}
