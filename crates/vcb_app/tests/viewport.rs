//! Geometry of the image view: fit, zoom about the pointer, pan, 1:1 and crops.

use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};
use vcb_app::viewport::{self, MAX_TEXTURE_SIDE, View};

fn view_rect() -> Rect {
    Rect::from_min_size(pos2(100.0, 50.0), vec2(1000.0, 800.0))
}

fn close(a: Pos2, b: Pos2) -> bool {
    (a - b).length() < 1e-3
}

/// The content point drawn at screen position `screen`.
fn content_at(view: &View, rect: Rect, content: Vec2, screen: Pos2) -> Vec2 {
    let image = view.image_rect(rect, content);
    (screen - image.min) / view.zoom(rect, content)
}

#[test]
fn fit_centres_the_content_and_touches_the_nearer_edges() {
    let content = vec2(2000.0, 1000.0);
    let view = View::default();
    assert!(view.is_fit());
    let image = view.image_rect(view_rect(), content);
    assert!((image.width() - 1000.0).abs() < 1e-3, "{image:?}");
    assert!((image.height() - 500.0).abs() < 1e-3, "{image:?}");
    assert!(close(image.center(), view_rect().center()));
    assert!((viewport::fit_zoom(vec2(1000.0, 800.0), content) - 0.5).abs() < 1e-6);
}

#[test]
fn zooming_keeps_the_point_under_the_pointer_still() {
    let content = vec2(2272.0, 1704.0);
    let rect = view_rect();
    let mut view = View::default();
    let pointer = pos2(300.0, 200.0);
    let before = content_at(&view, rect, content, pointer);
    view.zoom_at(rect, content, pointer, 3.0);
    let after = content_at(&view, rect, content, pointer);
    assert!(
        (before - after).length() < 1e-2,
        "{before:?} moved to {after:?}"
    );
    let fit = viewport::fit_zoom(rect.size(), content);
    assert!((view.zoom(rect, content) / fit - 3.0).abs() < 1e-4);
    assert!(!view.is_fit());
}

#[test]
fn panning_moves_the_image_with_the_pointer() {
    let content = vec2(2272.0, 1704.0);
    let rect = view_rect();
    let mut view = View::default();
    view.zoom_at(rect, content, rect.center(), 4.0);
    let before = view.image_rect(rect, content);
    view.pan(rect, content, vec2(40.0, -25.0));
    let after = view.image_rect(rect, content);
    assert!(
        close(after.min, before.min + vec2(40.0, -25.0)),
        "{before:?} -> {after:?}"
    );
}

#[test]
fn panning_stops_at_the_content_edge() {
    let content = vec2(800.0, 600.0);
    let rect = view_rect();
    let mut view = View::default();
    view.pan(rect, content, vec2(1e6, 1e6));
    let image = view.image_rect(rect, content);
    assert!(
        image.contains(rect.center()) || close(image.min, rect.center()),
        "the viewport centre stays on the image: {image:?}"
    );
}

#[test]
fn one_to_one_maps_a_content_pixel_to_a_physical_pixel() {
    let content = vec2(2272.0, 1704.0);
    let rect = view_rect();
    let mut view = View::default();
    view.actual_pixels(content, 2.0);
    let image = view.image_rect(rect, content);
    assert!((image.width() - 1136.0).abs() < 1e-3, "{image:?}");
    assert!(close(image.center(), rect.center()), "1:1 keeps the centre");
    view.fit();
    assert!(view.is_fit());
}

#[test]
fn the_original_is_cropped_like_the_sensor_crops_it() {
    let uv = viewport::crop_uv(vec2(3000.0, 2000.0), 4.0 / 3.0);
    assert!((uv.width() - (4.0 / 3.0) / 1.5).abs() < 1e-6, "{uv:?}");
    assert!((uv.height() - 1.0).abs() < 1e-6);
    assert!(close(uv.center(), pos2(0.5, 0.5)));
    let tall = viewport::crop_uv(vec2(1000.0, 1000.0), 2.0);
    assert!((tall.height() - 0.5).abs() < 1e-6, "{tall:?}");
}

#[test]
fn a_frame_of_another_aspect_is_letterboxed_inside_the_content() {
    let outer = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0));
    let inner = viewport::contain(outer, 720.0, 480.0);
    assert!((inner.width() - 400.0).abs() < 1e-3);
    assert!((inner.height() - 400.0 / 1.5).abs() < 1e-3);
    assert!(close(inner.center(), outer.center()));
}

#[test]
fn oversized_images_are_subsampled_for_upload() {
    let width = MAX_TEXTURE_SIDE * 2 + 2;
    let rgb = vec![7; width as usize * 4 * 3];
    let image = viewport::texture_image(width, 4, &rgb);
    assert!(
        image.size[0] <= MAX_TEXTURE_SIDE as usize,
        "{:?}",
        image.size
    );
    assert_eq!(image.size[1], 1);
    let small = viewport::texture_image(4, 2, &[9; 4 * 2 * 3]);
    assert_eq!(small.size, [4, 2]);
}
