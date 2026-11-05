//! `vcb sheet`: one image through several presets, tiled into a contact sheet.
//!
//! Tiles are laid out row-major, each scaled to fit its cell in linear light (area
//! averaging, so a downscaled tile keeps the mean brightness of its pixels) and
//! centred on a dark grey background. Cells are separated from each other and from the
//! sheet's edge by [`GAP_PX`] pixels.

use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use clap::Args;
use vcb_core::{Frame, Scene, Simulator};

use crate::{CameraDir, Overrides, load_camera, output_format};

/// Space between cells, and between the cells and the sheet's edge, in pixels.
const GAP_PX: u32 = 4;

/// Longest edge of a default cell in pixels.
///
/// Keeps a sheet of a dozen full-resolution renders within a few thousand pixels.
const DEFAULT_CELL_MAX_PX: u32 = 640;

/// sRGB value of the background and the gaps.
///
/// Dark enough not to be mistaken for image content, light enough to show where a
/// black frame ends.
const BACKGROUND: [u8; 3] = [32, 32, 32];

#[derive(Debug, Args)]
pub(crate) struct SheetArgs {
    /// Output image; the extension picks PNG or JPEG.
    output: PathBuf,
    /// Input image (JPEG or PNG) rendered through every preset.
    #[arg(long)]
    input: PathBuf,
    /// Preset files, one tile each in order; repeat the flag or list several paths.
    #[arg(long = "preset", required = true, num_args = 1..)]
    presets: Vec<PathBuf>,
    /// Images (JPEG or PNG) appended as tiles after the renders, such as references.
    #[arg(long = "image", num_args = 1..)]
    images: Vec<PathBuf>,
    /// Number of tile columns; defaults to the square root of the tile count, rounded up.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    cols: Option<u32>,
    #[arg(
        long,
        value_name = "WIDTHxHEIGHT",
        help = "Cell size in pixels as WIDTHxHEIGHT; defaults to the first render's size, at most 640 on the long edge"
    )]
    cell: Option<Cell>,
    #[command(flatten)]
    cameras: CameraDir,
    #[command(flatten)]
    overrides: Overrides,
}

/// The size of one cell of the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    width: u32,
    height: u32,
}

impl FromStr for Cell {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed = s
            .split_once(['x', 'X'])
            .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)));
        match parsed {
            Some((width, height)) if width > 0 && height > 0 => Ok(Self { width, height }),
            _ => Err(format!(
                "expected WIDTHxHEIGHT in pixels, such as 640x480, got \"{s}\""
            )),
        }
    }
}

impl Display for Cell {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// One image on the sheet and a description of where it came from.
struct Tile {
    label: String,
    frame: Frame,
}

/// Renders the tiles, composes the sheet and writes it.
pub(crate) fn run(args: &SheetArgs) -> Result<()> {
    let format = output_format(&args.output)?;
    let scene = vcb_io::read_image(&args.input)?;
    let mut tiles = Vec::with_capacity(args.presets.len() + args.images.len());
    for path in &args.presets {
        let preset = args.overrides.preset(path)?;
        let camera = load_camera(&args.cameras.camera_dir, &preset.camera)?;
        let mut simulator = Simulator::builder((camera, preset))
            .input_size(scene.width(), scene.height())
            .build()
            .with_context(|| format!("cannot build the simulator for {}", path.display()))?;
        let mode = simulator.timing().mode;
        let frame = simulator.frame(&scene, 0).clone();
        tiles.push(Tile {
            label: format!("preset {} ({mode})", path.display()),
            frame,
        });
    }
    for path in &args.images {
        let image = vcb_io::read_image(path)?;
        tiles.push(Tile {
            label: format!("image {}", path.display()),
            frame: Frame::from_rgb(image.width(), image.height(), image.to_srgb8()),
        });
    }
    let Some(first) = tiles.first() else {
        bail!("a sheet needs at least one preset");
    };
    let cell = args.cell.unwrap_or_else(|| default_cell(&first.frame));
    let count = u32::try_from(tiles.len()).context("too many tiles for one sheet")?;
    let cols = args.cols.unwrap_or_else(|| default_cols(count));
    let rows = count.div_ceil(cols);
    let frames: Vec<&Frame> = tiles.iter().map(|tile| &tile.frame).collect();
    let sheet = compose(&frames, cols, cell);
    vcb_io::write_frame(&args.output, &sheet, format)?;
    for (index, tile) in (0_u32..).zip(&tiles) {
        println!(
            "tile {index} (row {}, column {}): {} {}x{}",
            index / cols,
            index % cols,
            tile.label,
            tile.frame.width(),
            tile.frame.height()
        );
    }
    println!(
        "{} {}x{} {cols} columns x {rows} rows of {cell} cells",
        args.output.display(),
        sheet.width(),
        sheet.height()
    );
    Ok(())
}

/// The first tile's size, scaled down to [`DEFAULT_CELL_MAX_PX`] on the long edge.
fn default_cell(frame: &Frame) -> Cell {
    let (width, height) = (frame.width().max(1), frame.height().max(1));
    let long = width.max(height);
    if long <= DEFAULT_CELL_MAX_PX {
        return Cell { width, height };
    }
    let scale = f64::from(DEFAULT_CELL_MAX_PX) / f64::from(long);
    let fit = |edge: u32| ((f64::from(edge) * scale).round() as u32).max(1);
    Cell {
        width: fit(width),
        height: fit(height),
    }
}

/// The smallest column count whose square holds `count` tiles.
fn default_cols(count: u32) -> u32 {
    let root = count.isqrt();
    if root * root < count {
        root + 1
    } else {
        root.max(1)
    }
}

/// Lays `frames` out row-major in `cols` columns of `cell`-sized, letterboxed cells.
fn compose(frames: &[&Frame], cols: u32, cell: Cell) -> Frame {
    let count = frames.len() as u32;
    let rows = count.div_ceil(cols);
    let width = cols * cell.width + (cols + 1) * GAP_PX;
    let height = rows * cell.height + (rows + 1) * GAP_PX;
    let mut rgb = BACKGROUND.repeat(width as usize * height as usize);
    for (index, frame) in (0_u32..).zip(frames) {
        let (tile_w, tile_h) = fit(frame.width(), frame.height(), cell);
        let tile = resize(frame, tile_w, tile_h);
        let x0 = GAP_PX + (index % cols) * (cell.width + GAP_PX) + (cell.width - tile_w) / 2;
        let y0 = GAP_PX + (index / cols) * (cell.height + GAP_PX) + (cell.height - tile_h) / 2;
        let stride = tile_w as usize * 3;
        for (y, row) in (y0..).zip(tile.rgb().chunks_exact(stride)) {
            let start = (y as usize * width as usize + x0 as usize) * 3;
            rgb[start..start + stride].copy_from_slice(row);
        }
    }
    Frame::from_rgb(width, height, rgb)
}

/// The largest size with the frame's aspect ratio that fits in `cell`.
fn fit(width: u32, height: u32, cell: Cell) -> (u32, u32) {
    let scale =
        (f64::from(cell.width) / f64::from(width)).min(f64::from(cell.height) / f64::from(height));
    let edge = |value: u32, limit: u32| ((f64::from(value) * scale).round() as u32).clamp(1, limit);
    (edge(width, cell.width), edge(height, cell.height))
}

/// Resamples `frame` to `width` by `height` by area averaging in linear light.
fn resize(frame: &Frame, width: u32, height: u32) -> Frame {
    if (frame.width(), frame.height()) == (width, height) {
        return frame.clone();
    }
    let source = Scene::from_srgb8(frame.width(), frame.height(), frame.rgb());
    let mut scaled = Scene::new(width, height);
    let columns = footprints(frame.width(), width);
    let rows = footprints(frame.height(), height);
    let mut across = vec![0.0_f32; width as usize * frame.height() as usize];
    for (src, dst) in source.planes().into_iter().zip(scaled.planes_mut()) {
        for (row, out) in src.rows().zip(across.chunks_exact_mut(width as usize)) {
            for (value, taps) in out.iter_mut().zip(&columns) {
                *value = taps.apply(|i| row[i]);
            }
        }
        for (out, taps) in dst.rows_mut().zip(&rows) {
            for (x, value) in out.iter_mut().enumerate() {
                *value = taps.apply(|i| across[i * width as usize + x]);
            }
        }
    }
    Frame::from_rgb(width, height, scaled.to_srgb8())
}

/// The source samples one output sample covers, with their share of its area.
struct Footprint {
    first: usize,
    weights: Vec<f32>,
}

impl Footprint {
    fn apply(&self, sample: impl Fn(usize) -> f32) -> f32 {
        (self.first..)
            .zip(&self.weights)
            .map(|(i, weight)| sample(i) * weight)
            .sum()
    }
}

/// Footprints of `dst` equal output samples spread over `src` input samples.
fn footprints(src: u32, dst: u32) -> Vec<Footprint> {
    let scale = f64::from(src) / f64::from(dst);
    (0..dst)
        .map(|i| {
            let (lo, hi) = (f64::from(i) * scale, f64::from(i + 1) * scale);
            let first = lo.floor() as usize;
            let last = (hi.ceil() as usize).min(src as usize);
            let weights = (first..last)
                .map(|j| {
                    let overlap = hi.min(j as f64 + 1.0) - lo.max(j as f64);
                    (overlap.max(0.0) / scale) as f32
                })
                .collect();
            Footprint { first, weights }
        })
        .collect()
}
