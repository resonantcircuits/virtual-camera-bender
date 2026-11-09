//! Golden-image regression tests: every shipped preset against a stored frame hash.
//!
//! Each test renders the same synthetic scene, 160 by 120 sRGB pixels generated here (a
//! colour gradient with a bright disc and a dark rectangle, so every bend has edges to
//! act on), through one preset in still mode at match-input resolution with the preset's own
//! seed, as frame 0 from power-on. It hashes the frame's RGB bytes with 64-bit FNV-1a and
//! compares the hash, 16 hexadecimal digits, with `tests/golden/<preset>.hash`. Beside
//! each hash lies `tests/golden/<preset>.png`, the golden frame for human review; the
//! PNGs are not compared. On a mismatch the new render is saved under Cargo's
//! integration-test temporary directory and named in the failure message.
//!
//! Any change to a rendered byte fails these tests. After an intended change of the
//! output (code, camera file or preset), regenerate the goldens with
//!
//! ```text
//! VCB_UPDATE_GOLDEN=1 cargo test -p vcb_core --test golden -- --nocapture
//! ```
//!
//! With `VCB_UPDATE_GOLDEN=1` the tests rewrite the hashes and PNGs instead of
//! asserting and print every file they wrote. Review the PNGs before committing them.
//!
//! A render is deterministic for a given scene, camera, preset and seed, whatever the
//! thread count. The hashes hold for the platform that wrote them: a math library that
//! rounds `exp`, `ln` or `powf` differently in the last place can change a byte.

#![cfg(feature = "serde")]

use std::ops::Range;
use std::path::{Path, PathBuf};

use vcb_core::preset::{Mode, Resolution};
use vcb_core::{Camera, Frame, Preset, Scene, Simulator};

/// Width of the synthetic scene and, at match-input resolution, of the still frame.
const WIDTH: u32 = 160;

/// Height of the synthetic scene and of the still frame.
const HEIGHT: u32 = 120;

/// Centre of the bright disc in pixels.
const DISC_CENTRE: (i64, i64) = (112, 40);

/// Radius of the bright disc in pixels.
const DISC_RADIUS: i64 = 24;

/// Columns of the dark rectangle.
const RECT_COLS: Range<u32> = 16..72;

/// Rows of the dark rectangle.
const RECT_ROWS: Range<u32> = 72..108;

/// Environment variable that switches the tests from asserting to rewriting the goldens.
const UPDATE: &str = "VCB_UPDATE_GOLDEN";

/// Offset basis of 64-bit FNV-1a (Fowler, Noll and Vo).
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// Prime of 64-bit FNV-1a.
const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn repo_file(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn camera() -> Camera {
    toml::from_str(&repo_file("cameras/a520.toml")).unwrap()
}

fn preset(name: &str) -> Preset {
    toml::from_str(&repo_file(&format!("presets/{name}.toml"))).unwrap()
}

/// Returns `from + (to - from) * i / n` rounded down, in integer arithmetic.
fn ramp(from: i64, to: i64, i: u32, n: u32) -> u8 {
    (from + (to - from) * i64::from(i) / i64::from(n)) as u8
}

/// The synthetic scene in 8-bit sRGB.
///
/// A gradient runs from orange-red on the left to blue on the right and brightens its
/// green downwards; a near-white disc, clipped as an overexposed light would be, sits
/// in the upper right and a near-black rectangle in the lower left. Integer arithmetic
/// keeps the bytes identical on every platform.
fn scene() -> Scene {
    let mut rgb = Vec::with_capacity((WIDTH * HEIGHT * 3) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (dx, dy) = (i64::from(x) - DISC_CENTRE.0, i64::from(y) - DISC_CENTRE.1);
            let pixel = if dx * dx + dy * dy <= DISC_RADIUS * DISC_RADIUS {
                [255, 252, 244]
            } else if RECT_COLS.contains(&x) && RECT_ROWS.contains(&y) {
                [10, 9, 12]
            } else {
                [
                    ramp(220, 40, x, WIDTH - 1),
                    ramp(50, 200, y, HEIGHT - 1),
                    ramp(60, 210, x, WIDTH - 1),
                ]
            };
            rgb.extend(pixel);
        }
    }
    Scene::from_srgb8(WIDTH, HEIGHT, &rgb)
}

/// Returns the 64-bit FNV-1a hash of `bytes`.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    })
}

/// Renders the scene through preset `name` in still mode at match-input resolution.
fn render(name: &str) -> Frame {
    let mut preset = preset(name);
    preset.readout.mode = Mode::Still;
    preset.readout.resolution = Resolution::MatchInput;
    let mut sim = Simulator::builder((camera(), preset))
        .input_size(WIDTH, HEIGHT)
        .build()
        .unwrap();
    sim.frame(&scene(), 0).clone()
}

fn save_png(frame: &Frame, path: &Path) {
    image::save_buffer(
        path,
        frame.rgb(),
        frame.width(),
        frame.height(),
        image::ExtendedColorType::Rgb8,
    )
    .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Compares the render of preset `name` with its golden hash, or rewrites the golden.
fn check(name: &str) {
    let frame = render(name);
    assert_eq!(
        (frame.width(), frame.height()),
        (WIDTH, HEIGHT),
        "{name}: a match-input still has the input's size"
    );
    let hash = format!("{:016x}", fnv1a(frame.rgb()));
    let dir = golden_dir();
    let hash_path = dir.join(format!("{name}.hash"));
    let png_path = dir.join(format!("{name}.png"));

    if std::env::var_os(UPDATE).is_some_and(|value| value == "1") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&hash_path, format!("{hash}\n"))
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", hash_path.display()));
        save_png(&frame, &png_path);
        println!(
            "{name}: wrote {} ({hash}) and {}",
            hash_path.display(),
            png_path.display()
        );
        return;
    }

    let golden = std::fs::read_to_string(&hash_path).unwrap_or_else(|error| {
        panic!(
            "{name}: cannot read {}: {error}; run with {UPDATE}=1 to create it",
            hash_path.display()
        )
    });
    let golden = golden.trim();
    if golden != hash {
        let actual = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("golden-{name}.png"));
        save_png(&frame, &actual);
        panic!(
            "{name}: the frame hashes to {hash} but the golden hash is {golden}. \
             The new render is {}, the golden frame {}. If the change is intended, \
             rerun with {UPDATE}=1 and review the PNGs.",
            actual.display(),
            png_path.display()
        );
    }
}

#[test]
fn the_clean_camera_renders_its_golden_frame() {
    check("a520-clean");
}

#[test]
fn the_hpf_bend_renders_its_golden_frame() {
    check("a520-hpf");
}

#[test]
fn the_div_bend_renders_its_golden_frame() {
    check("a520-div");
}

#[test]
fn the_inv_bend_renders_its_golden_frame() {
    check("a520-inv");
}

#[test]
fn the_full_spectrum_optics_render_their_golden_frame() {
    check("a520-full-spectrum");
}

#[test]
fn the_chlorophyll_optics_render_their_golden_frame() {
    check("a520-chlorophyll");
}

#[test]
fn the_full_spectrum_hpf_bend_renders_its_golden_frame() {
    check("a520-full-spectrum-hpf");
}

#[test]
fn the_brownout_rail_renders_its_golden_frame() {
    check("a520-brownout");
}

#[test]
fn the_sieve_rail_renders_its_golden_frame() {
    check("a520-sieve");
}

#[test]
fn the_dying_battery_renders_its_golden_frame() {
    check("a520-dying-battery");
}

#[test]
fn the_loose_sync_clock_renders_its_golden_frame() {
    check("a520-loose-sync");
}

#[test]
fn the_reclock_roll_clock_renders_its_golden_frame() {
    check("a520-reclock-roll");
}

#[test]
fn the_s9000_tear_memory_renders_its_golden_frame() {
    check("a520-s9000-tear");
}

#[test]
fn the_mirror_bank_memory_renders_its_golden_frame() {
    check("a520-mirror-bank");
}

#[test]
fn the_heat_gun_sensor_renders_its_golden_frame() {
    check("a520-heat-gun");
}

#[test]
fn the_substrate_sensor_renders_its_golden_frame() {
    check("a520-substrate");
}

#[test]
fn the_reset_drag_renders_its_golden_frame() {
    check("a520-reset-drag");
}

#[test]
fn the_hot_reclock_renders_its_golden_frame() {
    check("a520-hot-reclock");
}

#[test]
fn the_stuck_bit_memory_renders_its_golden_frame() {
    check("a520-stuck-bit");
}

#[test]
fn the_hot_board_renders_its_golden_frame() {
    check("a520-hot-board");
}

/// The preset's seed, 17, gives a first frame with a flipped green pixel gain, so the
/// golden shows the bend; a seed whose first frame takes no flip that shows would
/// render the clean camera's bytes, since clean writes restore its exact values.
#[test]
fn the_serial_noise_renders_its_golden_frame() {
    check("a520-serial-noise");
}

#[test]
fn the_light_leak_renders_its_golden_frame() {
    check("a520-light-leak");
}

#[test]
fn the_open_body_renders_its_golden_frame() {
    check("a520-open-body");
}

/// The preset's seed, 167, closes the contact during the first frame's scan on the
/// golden scene as on the sample, so the golden shows the decoder out of step.
#[test]
fn the_card_rot_renders_its_golden_frame() {
    check("a520-card-rot");
}

#[test]
fn the_strip_repeat_renders_its_golden_frame() {
    check("a520-strip-repeat");
}
