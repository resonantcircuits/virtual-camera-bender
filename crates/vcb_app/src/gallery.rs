//! The preset gallery: every preset for the camera, rendered on the loaded image.
//!
//! An overlay over the central area, opened with G or the Gallery button beside the
//! preset picker. It shows a grid of the presets in the preset folder that name the
//! current camera, each as a thumbnail of the scene in front of the lens rendered
//! through it, with its name and description under it; a preset file that cannot be
//! read, or a preset that cannot be rendered, shows its error in its tile. A filter
//! box narrows the grid to the presets whose name, description or bend sections
//! contain every word typed. The arrow keys move the selection, Enter or a click
//! applies the preset through the preset picker's path and closes the gallery, and
//! Escape or G closes it.
//!
//! [`Gallery`] holds the tiles, the filter and the thumbnails, cached per preset
//! file, camera and scene generation, its count of scene changes: a new scene drops
//! the old scene's thumbnails, and a preset file that changed or left the folder
//! drops its own. While the gallery is open the app asks it for the next thumbnails
//! to render, in grid order and a few at a time, and the render worker renders them
//! behind every preview and full render (see [`crate::render`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::text::LayoutJob;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Event, FontId, Galley, Key, Layout, Margin,
    Painter, Pos2, Rect, Response, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit,
    TextStyle, Ui,
};
use vcb_core::{Camera, Preset, Scene};

use crate::presets::{self, Entry};
use crate::render::{GALLERY_LONG_EDGE, Thumbnail, ThumbnailReply};
use crate::theme;
use crate::viewport::{self, Texture};

/// Height of a tile's image in points.
///
/// A 4:3 thumbnail fills it 256 points wide, so three landscape tiles, or four
/// portrait ones, fit across the central area of the window at its opening size.
const IMAGE_HEIGHT: f32 = theme::GRID * 24.0;

/// Narrowest tile in points, so the longest shipped preset name fits on its line.
const MIN_TILE_WIDTH: f32 = theme::GRID * 25.0;

/// Space between tiles and around a tile's contents, in points.
const GAP: f32 = theme::GRID * 2.0;

/// Rows of description text a tile shows; a longer description ends in an ellipsis.
///
/// Most descriptions of up to 200 characters fit at a tile's width; the tooltip
/// holds the whole text.
const DESCRIPTION_ROWS: usize = 4;

/// Rows of an error message a tile's image box shows, as many as fit in it.
const ERROR_ROWS: usize = 12;

/// Thumbnails asked of the render worker at a time.
///
/// One rendering and one waiting, so the worker never idles between thumbnails, while
/// closing the gallery or narrowing the filter wastes at most two renders.
const IN_FLIGHT: usize = 2;

/// What the user did in the gallery during one pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Nothing that closes the gallery.
    Stay,
    /// Close the gallery.
    Close,
    /// Apply this preset and close the gallery.
    Apply(Entry),
}

/// A thumbnail in the cache.
#[derive(Debug)]
enum Thumb {
    /// The render, and how long it took.
    Ready(Texture, Duration),
    /// Why the preset could not be rendered.
    Failed(String),
}

/// What a thumbnail is cached for: a preset file, a camera and a scene.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheKey {
    path: PathBuf,
    camera: String,
    scene: u64,
}

/// One preset file of the gallery.
#[derive(Debug, Clone)]
struct Tile {
    entry: Entry,
    /// The preset, or why the file cannot be read.
    preset: Result<Preset, String>,
    /// The lower-case text the filter searches: name, description and bend sections.
    search: String,
}

impl Tile {
    /// Reads the preset file of `entry`.
    fn read(entry: &Entry) -> Self {
        let preset = presets::load_preset(&entry.path).map_err(|error| format!("{error:#}"));
        let mut search = entry.name.to_lowercase();
        if let Ok(preset) = &preset {
            if let Some(description) = &preset.description {
                search.push('\n');
                search.push_str(&description.to_lowercase());
            }
            for section in preset.bend.sections() {
                search.push('\n');
                search.push_str(section);
            }
        }
        Self {
            entry: entry.clone(),
            preset,
            search,
        }
    }

    /// Whether the tile belongs in the gallery of `camera`; an unreadable file always does.
    fn is_for(&self, camera: &str) -> bool {
        match &self.preset {
            Ok(preset) => preset.camera == camera,
            Err(_) => true,
        }
    }

    /// Whether the tile's text holds every word of `words`.
    fn matches(&self, words: &[String]) -> bool {
        words.iter().all(|word| self.search.contains(word.as_str()))
    }

    /// The cache key of the tile's thumbnail on scene `scene`, if the preset was read.
    fn key(&self, scene: u64) -> Option<CacheKey> {
        let preset = self.preset.as_ref().ok()?;
        Some(CacheKey {
            path: self.entry.path.clone(),
            camera: preset.camera.clone(),
            scene,
        })
    }
}

/// The gallery's state: the tiles, the filter, the selection and the thumbnails.
#[derive(Debug, Default)]
pub struct Gallery {
    open: bool,
    filter: String,
    /// The selected tile, as an index into `tiles`.
    selected: Option<usize>,
    tiles: Vec<Tile>,
    /// The camera whose presets are shown.
    camera: String,
    /// Count of scene changes; thumbnails of earlier scenes are dropped.
    scene: u64,
    thumbnails: Vec<(CacheKey, Thumb)>,
    /// Thumbnails asked for and not yet answered, with the ticket of each request.
    in_flight: Vec<(CacheKey, u64)>,
    /// Thumbnails asked for since the app started; the last one is the newest ticket.
    requests: u64,
    /// Tiles per row in the last layout, for the up and down keys.
    columns: usize,
    /// Whether to scroll the selected tile into view in the next pass.
    reveal: bool,
}

impl Gallery {
    /// Whether the gallery is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the gallery on the presets of `list` for `camera`, selecting `current`.
    ///
    /// The files are read again, so thumbnails of presets that changed are dropped;
    /// the filter starts empty.
    pub fn open(&mut self, list: &[Entry], camera: &str, current: &str) {
        self.refresh(list);
        camera.clone_into(&mut self.camera);
        self.filter.clear();
        self.selected = self
            .tiles
            .iter()
            .position(|tile| tile.entry.name == current && tile.is_for(camera));
        self.ensure_selection();
        self.open = true;
        self.reveal = true;
    }

    /// Closes the gallery and gives up the keyboard; the thumbnails stay cached.
    ///
    /// No new thumbnail is asked for while it is closed; those already asked for are
    /// still taken in.
    pub fn close(&mut self, ctx: &egui::Context) {
        self.open = false;
        ctx.memory_mut(|memory| memory.surrender_focus(filter_id()));
    }

    /// Takes in a new preset list, dropping the thumbnails of files that changed or left.
    pub fn refresh(&mut self, list: &[Entry]) {
        let tiles: Vec<Tile> = list.iter().map(Tile::read).collect();
        let old = &self.tiles;
        let unchanged = |path: &Path| match (find(old, path), find(&tiles, path)) {
            (Some(before), Some(after)) => {
                before.entry == after.entry && before.preset == after.preset
            }
            _ => false,
        };
        self.thumbnails.retain(|(key, _)| unchanged(&key.path));
        self.in_flight.retain(|(key, _)| unchanged(&key.path));
        let selected = self
            .selected
            .and_then(|index| self.tiles.get(index))
            .map(|tile| tile.entry.path.clone());
        self.tiles = tiles;
        self.selected =
            selected.and_then(|path| self.tiles.iter().position(|tile| tile.entry.path == path));
        self.ensure_selection();
    }

    /// Shows the presets of `camera` from now on.
    pub fn set_camera(&mut self, camera: &str) {
        if self.camera != camera {
            camera.clone_into(&mut self.camera);
            self.ensure_selection();
        }
    }

    /// Records that the scene in front of the lens changed; its thumbnails are dropped.
    pub fn scene_changed(&mut self) {
        self.scene += 1;
        let scene = self.scene;
        self.thumbnails.retain(|(key, _)| key.scene == scene);
        self.in_flight.retain(|(key, _)| key.scene == scene);
    }

    /// The scene generation the thumbnails are rendered for.
    #[must_use]
    pub fn scene_generation(&self) -> u64 {
        self.scene
    }

    /// The presets shown, in grid order: those for the camera that the filter matches.
    #[must_use]
    pub fn shown(&self) -> Vec<&Entry> {
        self.shown_tiles()
            .into_iter()
            .map(|index| &self.tiles[index].entry)
            .collect()
    }

    /// The selected preset, if any is shown.
    #[must_use]
    pub fn selected(&self) -> Option<&Entry> {
        self.selected.map(|index| &self.tiles[index].entry)
    }

    /// Whether the tile of `entry` is complete for the current scene.
    ///
    /// A tile is complete once its thumbnail, or the error of its render, has come
    /// back; the tile of a file that cannot be read is complete with its error.
    #[must_use]
    pub fn is_ready(&self, entry: &Entry) -> bool {
        self.tiles
            .iter()
            .find(|tile| tile.entry == *entry)
            .is_some_and(|tile| {
                tile.key(self.scene)
                    .is_none_or(|key| self.thumbnail(&key).is_some())
            })
    }

    /// How many thumbnails have been asked of the renderer since the app started.
    #[must_use]
    pub fn requests(&self) -> u64 {
        self.requests
    }

    /// Whether thumbnails are being rendered.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        !self.in_flight.is_empty()
    }

    /// The next thumbnail to render on `scene` through `camera`, in grid order.
    ///
    /// Returns `None` while the gallery is closed, while enough thumbnails are being
    /// rendered, or when every shown tile has one or cannot have one.
    pub fn next_request(&mut self, camera: &Arc<Camera>, scene: &Arc<Scene>) -> Option<Thumbnail> {
        if !self.open || self.in_flight.len() >= IN_FLIGHT {
            return None;
        }
        let (tile, key) = self.shown_tiles().into_iter().find_map(|index| {
            let tile = &self.tiles[index];
            let key = tile.key(self.scene)?;
            let wanted = self.thumbnail(&key).is_none()
                && !self.in_flight.iter().any(|(flying, _)| *flying == key);
            wanted.then_some((tile, key))
        })?;
        let preset = tile.preset.as_ref().ok()?.clone();
        let entry = tile.entry.clone();
        self.requests += 1;
        self.in_flight.push((key, self.requests));
        Some(Thumbnail {
            ticket: self.requests,
            entry,
            preset,
            camera: Arc::clone(camera),
            scene: Arc::clone(scene),
            scene_generation: self.scene,
        })
    }

    /// Takes in a thumbnail from the renderer; returns whether it was still wanted.
    ///
    /// A thumbnail of an earlier scene, or of a preset file that has changed since it
    /// was asked for, is dropped.
    pub fn receive(&mut self, ctx: &egui::Context, reply: ThumbnailReply) -> bool {
        let key = CacheKey {
            path: reply.path,
            camera: reply.camera,
            scene: reply.scene_generation,
        };
        let Some(position) = self
            .in_flight
            .iter()
            .position(|(flying, ticket)| *flying == key && *ticket == reply.ticket)
        else {
            return false;
        };
        self.in_flight.remove(position);
        let thumb = match reply.outcome {
            Ok(output) => Thumb::Ready(
                Texture::upload(ctx, "thumbnail", output.image),
                output.elapsed,
            ),
            Err(message) => Thumb::Failed(message),
        };
        self.thumbnails.push((key, thumb));
        true
    }

    /// Takes the gallery's keys from this pass's input: arrows, Enter and Escape.
    ///
    /// Acts only while no control has the keyboard, or the filter box has it; then
    /// the up and down arrows, Enter and Escape still reach the gallery, and the left
    /// and right arrows move the filter's cursor.
    pub fn keys(&mut self, ctx: &egui::Context) -> Pick {
        if !self.open {
            return Pick::Stay;
        }
        let in_filter = ctx.memory(|memory| memory.has_focus(filter_id()));
        let free =
            !ctx.egui_wants_keyboard_input() && ctx.memory(|memory| memory.focused().is_none());
        if !(in_filter || free) {
            return Pick::Stay;
        }
        let columns = self.columns.max(1).cast_signed();
        let mut moves = Vec::new();
        let mut pick = Pick::Stay;
        let mut apply = false;
        ctx.input_mut(|input| {
            input.events.retain(|event| {
                let Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                else {
                    return true;
                };
                if modifiers.command || modifiers.ctrl || modifiers.alt || modifiers.mac_cmd {
                    return true;
                }
                match key {
                    Key::Escape => pick = Pick::Close,
                    Key::Enter => apply = true,
                    Key::ArrowUp => moves.push(-columns),
                    Key::ArrowDown => moves.push(columns),
                    Key::ArrowLeft if free => moves.push(-1),
                    Key::ArrowRight if free => moves.push(1),
                    _ => return true,
                }
                false
            });
        });
        for delta in moves {
            self.step(delta);
        }
        if pick == Pick::Close {
            return pick;
        }
        match self.selected() {
            Some(entry) if apply => Pick::Apply(entry.clone()),
            _ => Pick::Stay,
        }
    }

    /// Draws the gallery over `ui`'s whole area.
    ///
    /// `aspect` is the width over the height of the full render on screen, which the
    /// tiles' images take; `None` says that no image is loaded.
    pub fn show(&mut self, ui: &mut Ui, aspect: Option<f32>) -> Pick {
        let rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, theme::CANVAS);
        let mut pick = Pick::Stay;
        egui::Frame::new()
            .inner_margin(Margin::same(GAP as i8))
            .show(ui, |ui| {
                ui.set_min_size(rect.size() - egui::vec2(GAP, GAP) * 2.0);
                if self.header(ui, aspect.is_some()) {
                    pick = Pick::Close;
                }
                ui.add_space(theme::GRID);
                if let Some(entry) = self.grid(ui, aspect) {
                    pick = Pick::Apply(entry);
                }
            });
        pick
    }

    /// The title, the filter box, the count and the close button; returns whether to close.
    fn header(&mut self, ui: &mut Ui, scene: bool) -> bool {
        let mut close = false;
        let total = self
            .tiles
            .iter()
            .filter(|tile| tile.is_for(&self.camera))
            .count();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Gallery").heading().color(theme::TEXT));
            ui.add_space(theme::GRID);
            let filter = ui.add(
                TextEdit::singleline(&mut self.filter)
                    .id(filter_id())
                    .hint_text("Filter by name, description or bend")
                    .desired_width(theme::GRID * 32.0),
            );
            if filter.changed() {
                self.ensure_selection();
                self.reveal = true;
            }
            let shown = self.shown_tiles().len();
            ui.label(
                RichText::new(format!(
                    "{shown} of {total} presets for the {}",
                    self.camera
                ))
                .color(theme::TEXT_WEAK),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = ui
                    .button("Close")
                    .on_hover_text("Close the gallery (Esc or G)")
                    .clicked();
            });
        });
        let hint = if scene {
            format!(
                "Each tile is the first frame from power-on, {GALLERY_LONG_EDGE} pixels on \
                 the long edge. Click or Enter applies the preset; the arrows move."
            )
        } else {
            "Open a photo or a video to see every preset on it.".to_owned()
        };
        ui.label(RichText::new(hint).color(theme::TEXT_WEAK));
        close
    }

    /// The grid of tiles in a scroll area; returns the preset clicked, if any.
    ///
    /// Each row holds as many tiles as fit at their narrowest, widened to fill it.
    fn grid(&mut self, ui: &mut Ui, aspect: Option<f32>) -> Option<Entry> {
        let shown = self.shown_tiles();
        if shown.is_empty() {
            ui.label(RichText::new("No preset matches the filter.").color(theme::TEXT_WEAK));
            return None;
        }
        let scene = aspect.is_some();
        let image_width = IMAGE_HEIGHT * aspect.unwrap_or(4.0 / 3.0).clamp(0.5, 2.0);
        let narrowest = (image_width + GAP).max(MIN_TILE_WIDTH);
        let mut clicked = None;
        let reveal = std::mem::take(&mut self.reveal);
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available = ui.available_width();
                let columns = ((available + GAP) / (narrowest + GAP)).floor().max(1.0) as usize;
                let width = (available - GAP * (columns - 1) as f32) / columns as f32;
                self.columns = columns;
                ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
                for row in shown.chunks(columns) {
                    ui.horizontal(|ui| {
                        for &index in row {
                            let selected = self.selected == Some(index);
                            let response = self.tile(ui, index, width, selected, scene);
                            if selected && reveal {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                self.selected = Some(index);
                                clicked = Some(self.tiles[index].entry.clone());
                            }
                        }
                    });
                }
            });
        clicked
    }

    /// Draws one tile `width` points wide: the thumbnail or its placeholder, the name
    /// and the description.
    fn tile(&self, ui: &mut Ui, index: usize, width: f32, selected: bool, scene: bool) -> Response {
        let tile = &self.tiles[index];
        let name_font = TextStyle::Body.resolve(ui.style());
        let small_font = TextStyle::Small.resolve(ui.style());
        let image_size = egui::vec2(width - GAP, IMAGE_HEIGHT);
        let height = GAP * 0.5
            + image_size.y
            + theme::GRID
            + ui.text_style_height(&TextStyle::Body)
            + theme::GRID * 0.5
            + ui.text_style_height(&TextStyle::Small) * DESCRIPTION_ROWS as f32
            + GAP * 0.5;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());
        let painter = ui.painter_at(rect);
        let radius = CornerRadius::same(4);
        let fill = if selected {
            theme::HOUSING
        } else {
            theme::PANEL
        };
        painter.rect_filled(rect, radius, fill);
        let image = Rect::from_min_size(rect.min + egui::vec2(GAP, GAP) * 0.5, image_size);
        let thumb = tile.key(self.scene).and_then(|key| self.thumbnail(&key));
        picture(&painter, image, &tile.preset, thumb, &small_font, scene);
        let name_at = egui::pos2(image.left(), image.bottom() + theme::GRID);
        let name_colour = if selected { theme::ACCENT } else { theme::TEXT };
        painter.text(
            name_at,
            Align2::LEFT_TOP,
            &tile.entry.name,
            name_font,
            name_colour,
        );
        let description = tile
            .preset
            .as_ref()
            .ok()
            .and_then(|preset| preset.description.as_deref());
        if let Some(description) = description {
            let galley = wrapped(
                &painter,
                description,
                &small_font,
                theme::TEXT_WEAK,
                image.width(),
                DESCRIPTION_ROWS,
            );
            let at = name_at
                + egui::vec2(
                    0.0,
                    ui.text_style_height(&TextStyle::Body) + theme::GRID * 0.5,
                );
            painter.galley(at, galley, theme::TEXT_WEAK);
        }
        let stroke = if selected {
            Stroke::new(2.0, theme::ACCENT)
        } else if response.hovered() {
            Stroke::new(1.0, theme::LINE)
        } else {
            Stroke::NONE
        };
        painter.rect_stroke(rect, radius, stroke, StrokeKind::Inside);
        let rendered = match thumb {
            Some(Thumb::Ready(_, elapsed)) => {
                format!(" (rendered in {:.0} ms)", elapsed.as_secs_f64() * 1e3)
            }
            _ => String::new(),
        };
        let hover = match description {
            Some(description) => format!("Apply {}{rendered}\n{description}", tile.entry.name),
            None => format!("Apply {}{rendered}", tile.entry.name),
        };
        response.on_hover_text(hover)
    }

    /// The indices of the tiles shown, in grid order.
    fn shown_tiles(&self) -> Vec<usize> {
        let words: Vec<String> = self
            .filter
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.tiles
            .iter()
            .enumerate()
            .filter(|(_, tile)| tile.is_for(&self.camera) && tile.matches(&words))
            .map(|(index, _)| index)
            .collect()
    }

    /// The cached thumbnail for `key`.
    fn thumbnail(&self, key: &CacheKey) -> Option<&Thumb> {
        self.thumbnails
            .iter()
            .find(|(cached, _)| cached == key)
            .map(|(_, thumb)| thumb)
    }

    /// Keeps the selection on a shown tile: the first one if the selected is hidden.
    fn ensure_selection(&mut self) {
        let shown = self.shown_tiles();
        if !self.selected.is_some_and(|index| shown.contains(&index)) {
            self.selected = shown.first().copied();
        }
    }

    /// Moves the selection `delta` tiles along the grid, stopping at its ends.
    fn step(&mut self, delta: isize) {
        let shown = self.shown_tiles();
        let Some(last) = shown.len().checked_sub(1) else {
            return;
        };
        let at = self
            .selected
            .and_then(|index| shown.iter().position(|&shown| shown == index))
            .unwrap_or(0);
        let to = at.saturating_add_signed(delta).min(last);
        self.selected = Some(shown[to]);
        self.reveal = true;
    }
}

/// Paints a tile's image box: the thumbnail, the error, or a placeholder until it comes.
fn picture(
    painter: &Painter,
    image: Rect,
    preset: &Result<Preset, String>,
    thumb: Option<&Thumb>,
    font: &FontId,
    scene: bool,
) {
    painter.rect_filled(image, CornerRadius::ZERO, theme::SLOT);
    match (preset, thumb) {
        (Err(message), _) | (Ok(_), Some(Thumb::Failed(message))) => {
            let galley = wrapped(
                painter,
                message,
                font,
                theme::ERROR,
                image.width() - GAP,
                ERROR_ROWS,
            );
            painter.galley(image.min + egui::vec2(GAP, GAP) * 0.5, galley, theme::ERROR);
        }
        (Ok(_), Some(Thumb::Ready(texture, _))) => {
            let size = texture.size();
            painter.image(
                texture.id(),
                viewport::contain(image, size.x, size.y),
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        (Ok(_), None) => {
            painter.text(
                image.center(),
                Align2::CENTER_CENTER,
                if scene { "rendering…" } else { "no image" },
                font.clone(),
                theme::TEXT_WEAK,
            );
        }
    }
}

/// The tile of the preset file at `path`.
fn find<'a>(tiles: &'a [Tile], path: &Path) -> Option<&'a Tile> {
    tiles.iter().find(|tile| tile.entry.path == path)
}

/// The id of the filter box, which the keys ask about.
fn filter_id() -> egui::Id {
    egui::Id::unique("gallery-filter")
}

/// `text` laid out at `width`, cut to `rows` rows with an ellipsis.
fn wrapped(
    painter: &Painter,
    text: &str,
    font: &FontId,
    colour: Color32,
    width: f32,
    rows: usize,
) -> Arc<Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), font.clone(), colour, width);
    job.wrap.max_rows = rows;
    job.wrap.overflow_character = Some('…');
    painter.layout_job(job)
}
