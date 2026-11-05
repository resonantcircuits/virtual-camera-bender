//! Orientation, decoding and encoding of image files.

use std::path::PathBuf;

use vcb_core::Frame;
use vcb_core::Scene;
use vcb_core::scene::Channel;
use vcb_io::{Format, Orientation, orient, probe_image, read_image, write_frame};

fn sample() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../samples/sample-01.jpg"
    ))
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vcb_io");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// A 3 by 2 scene whose red channel numbers the pixels 1 to 6 in row-major order.
fn numbered() -> Scene {
    let mut scene = Scene::new(3, 2);
    for (i, value) in scene
        .plane_mut(Channel::Red)
        .data_mut()
        .iter_mut()
        .enumerate()
    {
        *value = (i + 1) as f32;
    }
    scene
}

fn find(scene: &Scene, value: f32) -> (u32, u32) {
    let plane = scene.plane(Channel::Red);
    let index = plane
        .data()
        .iter()
        .position(|&v| (v - value).abs() < 0.5)
        .unwrap();
    (index as u32 % plane.width(), index as u32 / plane.width())
}

#[test]
fn every_orientation_moves_the_top_right_pixel_where_exif_says() {
    // Stored image:  1 2 3      pixel 3 is the top-right corner.
    //                4 5 6
    let expected = [
        (Orientation::Normal, (3, 2), (2, 0)),
        (Orientation::MirrorHorizontal, (3, 2), (0, 0)),
        (Orientation::Rotate180, (3, 2), (0, 1)),
        (Orientation::MirrorVertical, (3, 2), (2, 1)),
        (Orientation::Transpose, (2, 3), (0, 2)),
        (Orientation::Rotate90, (2, 3), (1, 2)),
        (Orientation::Transverse, (2, 3), (1, 0)),
        (Orientation::Rotate270, (2, 3), (0, 0)),
    ];
    let scene = numbered();
    for (orientation, size, position) in expected {
        let upright = orient(&scene, orientation);
        assert_eq!((upright.width(), upright.height()), size, "{orientation}");
        assert_eq!(find(&upright, 3.0), position, "{orientation}");
    }
}

#[test]
fn orientations_compose_like_the_dihedral_group() {
    let scene = numbered();
    let twice = |o: Orientation| orient(&orient(&scene, o), o);
    for involution in [
        Orientation::MirrorHorizontal,
        Orientation::Rotate180,
        Orientation::MirrorVertical,
        Orientation::Transpose,
        Orientation::Transverse,
    ] {
        assert_eq!(twice(involution), scene, "{involution} twice");
    }
    let quarter = orient(
        &orient(&scene, Orientation::Rotate90),
        Orientation::Rotate270,
    );
    assert_eq!(quarter, scene);
    let half = orient(
        &orient(&scene, Orientation::Rotate90),
        Orientation::Rotate90,
    );
    assert_eq!(half, orient(&scene, Orientation::Rotate180));
}

#[test]
fn exif_values_map_one_to_one() {
    for (i, orientation) in Orientation::ALL.into_iter().enumerate() {
        let value = i as u32 + 1;
        assert_eq!(Orientation::from_exif(value), Some(orientation));
        assert_eq!(orientation.exif(), value);
    }
    assert_eq!(Orientation::from_exif(0), None);
    assert_eq!(Orientation::from_exif(9), None);
}

#[test]
fn sample_photo_is_read_upright_in_portrait() {
    let scene = read_image(sample()).unwrap();
    assert!(scene.height() > scene.width(), "{scene:?}");
    let probe = probe_image(sample()).unwrap();
    assert!(probe.orientation.swaps_axes(), "{probe:?}");
    assert_eq!((probe.width, probe.height), (scene.width(), scene.height()));
}

#[test]
fn png_round_trip_preserves_every_byte() {
    let (width, height) = (7, 5);
    let rgb: Vec<u8> = (0..width * height * 3)
        .map(|i| (i * 37 % 256) as u8)
        .collect();
    let frame = Frame::from_rgb(width, height, rgb.clone());
    let path = scratch("round-trip.png");
    write_frame(&path, &frame, Format::Png).unwrap();
    let scene = read_image(&path).unwrap();
    assert_eq!(scene.to_srgb8(), rgb);
}

#[test]
fn jpeg_output_has_the_frame_size() {
    let frame = Frame::from_rgb(24, 16, vec![90; 24 * 16 * 3]);
    let path = scratch("frame.jpg");
    write_frame(&path, &frame, Format::Jpeg { quality: 90 }).unwrap();
    let probe = probe_image(&path).unwrap();
    assert_eq!((probe.width, probe.height), (24, 16));
    assert_eq!(probe.orientation, Orientation::Normal);
}

#[test]
fn missing_and_corrupt_files_report_their_cause() {
    let missing = read_image(scratch("does-not-exist.png")).unwrap_err();
    assert!(missing.is_io());
    assert!(missing.path().ends_with("does-not-exist.png"));

    let corrupt = scratch("corrupt.png");
    std::fs::write(&corrupt, b"not an image").unwrap();
    let error = read_image(&corrupt).unwrap_err();
    assert!(error.is_decode());
    assert!(std::error::Error::source(&error).is_some());
}
