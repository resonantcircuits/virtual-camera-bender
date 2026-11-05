//! Writes the app icon as a macOS icon set: `cargo run -p vcb_app --example icon -- <dir>`.
//!
//! `<dir>` should end in `.iconset`; `iconutil -c icns <dir>` turns it into the
//! `.icns` file of the app bundle (`scripts/bundle-macos.sh` does both).

use std::path::PathBuf;
use std::process::ExitCode;

/// The image sizes of a macOS icon set: the point size and the scale.
const SIZES: [(u32, u32); 10] = [
    (16, 1),
    (16, 2),
    (32, 1),
    (32, 2),
    (128, 1),
    (128, 2),
    (256, 1),
    (256, 2),
    (512, 1),
    (512, 2),
];

fn main() -> ExitCode {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: icon <dir.iconset>");
        return ExitCode::FAILURE;
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("error: cannot create {}: {error}", dir.display());
        return ExitCode::FAILURE;
    }
    for (points, scale) in SIZES {
        let pixels = points * scale;
        let name = if scale == 1 {
            format!("icon_{points}x{points}.png")
        } else {
            format!("icon_{points}x{points}@{scale}x.png")
        };
        let path = dir.join(name);
        let Some(image) = image::RgbaImage::from_raw(pixels, pixels, vcb_app::icon::rgba(pixels))
        else {
            eprintln!("error: the icon of {pixels} pixels has the wrong size");
            return ExitCode::FAILURE;
        };
        if let Err(error) = image.save(&path) {
            eprintln!("error: cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    }
    println!("{}", dir.display());
    ExitCode::SUCCESS
}
