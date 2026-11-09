//! The central image view: fit, zoom about the pointer, pan, 1:1, and the original.
//!
//! The view shows a picture of a fixed *content size*, the size of the full render,
//! whatever the texture on screen: a preview has fewer pixels but covers the same
//! rectangle, so switching between preview and full render never moves the image.
//! [`View`] holds the zoom and pan as plain numbers and does the geometry; [`show`]
//! draws the picture and turns scrolling, pinching and dragging into zoom and pan.
//! Zoom is in points per content pixel; "1:1" maps one content pixel to one physical
//! screen pixel.

use std::fmt::{self, Debug, Formatter};

use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Pos2, Rect, Response, Sense, TextureFilter,
    TextureHandle, TextureId, TextureOptions, TextureWrapMode, Ui, Vec2,
};

use crate::theme;

/// Smallest zoom relative to the fitted zoom.
const MIN_ZOOM_OF_FIT: f32 = 0.25;

/// Largest zoom in points per content pixel, enough to inspect single pixels.
const MAX_ZOOM: f32 = 32.0;

/// Zoom factor per point of scrolling.
const SCROLL_ZOOM_RATE: f32 = 0.0025;

/// Largest texture edge the app uploads; larger images are shown subsampled.
///
/// The `max_texture_dimension_2d` that wgpu guarantees on every backend.
pub const MAX_TEXTURE_SIDE: u32 = 8192;

/// Zoom and pan of the image view.
///
/// With no explicit zoom the view fits the content into the viewport and centres it.
/// Otherwise `center` is the content point, in content pixels, at the viewport's
/// centre.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct View {
    zoom: Option<f32>,
    center: Vec2,
}

impl View {
    /// Returns to fitting the content into the viewport.
    pub fn fit(&mut self) {
        self.zoom = None;
    }

    /// Whether the view fits the content.
    #[must_use]
    pub fn is_fit(&self) -> bool {
        self.zoom.is_none()
    }

    /// The zoom in points per content pixel for a viewport and a content size.
    #[must_use]
    pub fn zoom(&self, view: Rect, content: Vec2) -> f32 {
        self.zoom.unwrap_or_else(|| fit_zoom(view.size(), content))
    }

    /// The screen rectangle the content covers.
    #[must_use]
    pub fn image_rect(&self, view: Rect, content: Vec2) -> Rect {
        let zoom = self.zoom(view, content);
        let center = if self.is_fit() {
            content / 2.0
        } else {
            self.center
        };
        Rect::from_min_size(view.center() - center * zoom, content * zoom)
    }

    /// Multiplies the zoom by `factor`, keeping the content point under `pointer` still.
    pub fn zoom_at(&mut self, view: Rect, content: Vec2, pointer: Pos2, factor: f32) {
        let zoom = self.zoom(view, content);
        let min = (fit_zoom(view.size(), content) * MIN_ZOOM_OF_FIT).min(MAX_ZOOM);
        let new_zoom = (zoom * factor).clamp(min, MAX_ZOOM);
        let offset = pointer - view.center();
        let under_pointer = self.content_center(content) + offset / zoom;
        self.zoom = Some(new_zoom);
        self.center = clamp_center(under_pointer - offset / new_zoom, content);
    }

    /// Moves the content by `delta` points on screen.
    pub fn pan(&mut self, view: Rect, content: Vec2, delta: Vec2) {
        let zoom = self.zoom(view, content);
        self.center = clamp_center(self.content_center(content) - delta / zoom, content);
        self.zoom = Some(zoom);
    }

    /// Shows one content pixel per physical screen pixel, keeping the centre.
    pub fn actual_pixels(&mut self, content: Vec2, pixels_per_point: f32) {
        self.center = self.content_center(content);
        self.zoom = Some(1.0 / pixels_per_point);
    }

    /// The content point at the viewport's centre.
    fn content_center(&self, content: Vec2) -> Vec2 {
        if self.is_fit() {
            content / 2.0
        } else {
            self.center
        }
    }
}

/// The zoom that fits `content` into `view`, both in their own units.
///
/// Returns 1 for an empty content size.
#[must_use]
pub fn fit_zoom(view: Vec2, content: Vec2) -> f32 {
    if content.x <= 0.0 || content.y <= 0.0 {
        return 1.0;
    }
    (view.x / content.x).min(view.y / content.y)
}

/// Keeps the view's centre on the content.
fn clamp_center(center: Vec2, content: Vec2) -> Vec2 {
    egui::vec2(
        center.x.clamp(0.0, content.x),
        center.y.clamp(0.0, content.y),
    )
}

/// The largest rectangle of aspect `width / height` centred in `outer`.
#[must_use]
pub fn contain(outer: Rect, width: f32, height: f32) -> Rect {
    if width <= 0.0 || height <= 0.0 {
        return outer;
    }
    let scale = (outer.width() / width).min(outer.height() / height);
    Rect::from_center_size(outer.center(), egui::vec2(width, height) * scale)
}

/// Texture coordinates of the centred crop of a `source` image with `aspect` (width over height).
#[must_use]
pub fn crop_uv(source: Vec2, aspect: f32) -> Rect {
    if source.x <= 0.0 || source.y <= 0.0 || aspect <= 0.0 {
        return Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
    }
    let source_aspect = source.x / source.y;
    let (u, v) = if source_aspect > aspect {
        (aspect / source_aspect, 1.0)
    } else {
        (1.0, source_aspect / aspect)
    };
    Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(u, v))
}

/// Converts interleaved RGB bytes to an egui image, subsampled to fit [`MAX_TEXTURE_SIDE`].
///
/// # Panics
///
/// Panics if `rgb` does not hold `width * height` pixels.
#[must_use]
pub fn texture_image(width: u32, height: u32, rgb: &[u8]) -> ColorImage {
    let (w, h) = (width as usize, height as usize);
    assert_eq!(
        rgb.len(),
        w * h * 3,
        "{width} x {height} RGB image has {} bytes",
        rgb.len()
    );
    let step = width.max(height).div_ceil(MAX_TEXTURE_SIDE).max(1) as usize;
    if step == 1 {
        return ColorImage::from_rgb([w, h], rgb);
    }
    let (out_w, out_h) = (w / step, h / step);
    let pixels = (0..out_h)
        .flat_map(|y| (0..out_w).map(move |x| ((y * step) * w + x * step) * 3))
        .map(|i| Color32::from_rgb(rgb[i], rgb[i + 1], rgb[i + 2]))
        .collect();
    ColorImage::new([out_w, out_h], pixels)
}

/// Sampling of the image textures: smooth when shrunk, sharp pixels when magnified.
const SAMPLING: TextureOptions = TextureOptions {
    magnification: TextureFilter::Nearest,
    minification: TextureFilter::Linear,
    wrap_mode: TextureWrapMode::ClampToEdge,
    mipmap_mode: None,
};

/// An image uploaded to the GPU; dropping it frees the texture.
#[derive(Clone)]
pub struct Texture(TextureHandle);

impl Texture {
    /// Uploads `image` as a new texture called `name`.
    #[must_use]
    pub fn upload(ctx: &egui::Context, name: &str, image: ColorImage) -> Self {
        Self(ctx.load_texture(name, image, SAMPLING))
    }

    /// Replaces the texture's image.
    pub fn set(&mut self, image: ColorImage) {
        self.0.set(image, SAMPLING);
    }

    /// The texture's size in texels.
    #[must_use]
    pub fn size(&self) -> Vec2 {
        self.0.size_vec2()
    }

    /// The texture's id, for painting it.
    pub(crate) fn id(&self) -> TextureId {
        self.0.id()
    }
}

impl Debug for Texture {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Texture")
            .field("id", &self.0.id())
            .field("size", &self.0.size())
            .finish()
    }
}

/// What the viewport shows.
#[derive(Debug, Clone, Copy)]
pub enum Picture<'a> {
    /// A render, drawn inside the content rectangle at its own aspect ratio.
    Render(&'a Texture),
    /// The source image, cropped to the content's aspect ratio as the camera crops it.
    Original(&'a Texture),
}

/// Draws `picture` with content size `content` and handles zoom and pan.
///
/// Scrolling or pinching zooms about the pointer, dragging pans, and a double click
/// returns to fit.
pub fn show(ui: &mut Ui, view: &mut View, content: Vec2, picture: Option<Picture<'_>>) -> Response {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, theme::CANVAS);

    if response.hovered() {
        let (scroll, pinch, pointer) = ui.input(|i| {
            (
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                i.pointer.hover_pos(),
            )
        });
        let factor = (scroll * SCROLL_ZOOM_RATE).exp() * pinch;
        if let Some(pointer) = pointer
            && (factor - 1.0).abs() > f32::EPSILON
        {
            view.zoom_at(rect, content, pointer, factor);
        }
    }
    if response.dragged() {
        view.pan(rect, content, response.drag_delta());
    }
    if response.double_clicked() {
        view.fit();
    }

    let area = view.image_rect(rect, content);
    let full_uv = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
    match picture {
        Some(Picture::Render(texture)) => {
            let size = texture.size();
            painter.image(
                texture.id(),
                contain(area, size.x, size.y),
                full_uv,
                Color32::WHITE,
            );
        }
        Some(Picture::Original(texture)) => {
            let uv = crop_uv(texture.size(), content.x / content.y.max(1.0));
            painter.image(texture.id(), area, uv, Color32::WHITE);
        }
        None => {}
    }
    response
}
