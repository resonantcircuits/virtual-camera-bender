//! The preset gallery: the overlay opens and closes, the filter narrows it, the
//! thumbnails arrive in grid order behind the renders and stay cached until the scene
//! or a preset file changes, and a tile applies its preset. Also the preset's
//! `description` through the app's writer.
//!
//! The window tests run the app's real `logic` and `ui` on an `egui::Context` without
//! a renderer, as `tests/inspector.rs` does; the painted text shapes tell what is on
//! screen and where.

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use eframe::App as _;
use eframe::egui::{
    self, DroppedFile, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape,
    ViewportCommand,
};
use vcb_app::app::{App, Setup};
use vcb_app::gallery::Gallery;
use vcb_app::presets::{self, Entry};
use vcb_app::render::{
    self, GALLERY_LONG_EDGE, Job, Lane, PREVIEW_LONG_EDGE, Renderer, Thumbnail, Worker,
};
use vcb_core::preset::Resolution;
use vcb_core::{Camera, Frame, Preset, Scene, Simulator, Timing};

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// The scratch folder of this test binary, with a subfolder `name` emptied for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("vcb_app_gallery")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("old scratch folder removed");
    }
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

/// A small gradient photo on disk; `tint` tells photos apart.
fn photo(dir: &Path, tint: u8) -> PathBuf {
    let path = dir.join(format!("gradient-{tint}.png"));
    let (width, height) = (160, 120);
    let rgb = (0..width * height)
        .flat_map(|i| {
            let x = (i % width * 255 / width) as u8;
            [x, tint, 255 - x]
        })
        .collect();
    vcb_io::write_frame(
        &path,
        &Frame::from_rgb(width, height, rgb),
        vcb_io::Format::Png,
    )
    .expect("the photo is written");
    path
}

fn shipped(name: &str) -> String {
    std::fs::read_to_string(root().join("presets").join(format!("{name}.toml")))
        .expect("a shipped preset")
}

/// A preset folder of five files: two shipped presets, one with a description, one
/// for another camera and one that does not parse.
fn small_folder(name: &str) -> PathBuf {
    let dir = scratch(name).join("presets");
    std::fs::create_dir_all(&dir).expect("preset folder");
    std::fs::write(dir.join("a520-hpf.toml"), shipped("a520-hpf")).expect("written");
    std::fs::write(dir.join("a520-brownout.toml"), shipped("a520-brownout")).expect("written");
    let mut glow = Preset::new("a520");
    glow.seed = 1;
    glow.readout.resolution = Resolution::MatchInput;
    glow.description = Some("The amplifier's glow in a warm corner.".to_owned());
    std::fs::write(
        dir.join("a520-warm.toml"),
        presets::to_toml(&glow).expect("serialises"),
    )
    .expect("written");
    std::fs::write(dir.join("zx81-plain.toml"), "camera = \"zx81\"\n").expect("written");
    std::fs::write(
        dir.join("a520-broken.toml"),
        "camera = \"a520\"\nseed = -1\n",
    )
    .expect("written");
    dir
}

/// A file dropped on the window.
#[derive(Debug)]
struct Dropped(PathBuf);

impl DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|error| error.to_string())
    }
}

/// Text painted in one pass: the text, where it starts, and whether it is visible.
#[derive(Debug, Clone)]
struct Painted {
    text: String,
    pos: Pos2,
    visible: bool,
}

/// The app and the context it draws into, as the window would hold them.
struct Window {
    ctx: egui::Context,
    app: App,
    frame: eframe::Frame,
    time: f64,
    titles: Vec<String>,
}

impl Window {
    fn new(photo: &Path, presets_dir: PathBuf) -> Self {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let app = App::with_setup(
            &cc,
            Setup {
                path: Some(photo.to_path_buf()),
                camera_dir: root().join("cameras"),
                presets_dir,
                screenshot: None,
            },
        );
        Self {
            ctx,
            app,
            frame: eframe::Frame::_new_kittest(),
            time: 0.0,
            titles: Vec::new(),
        }
    }

    fn gallery(&self) -> &Gallery {
        self.app.gallery()
    }

    /// Runs one logic and ui pass with `events` and `dropped` files; returns what it painted.
    fn pass_with(&mut self, events: Vec<Event>, dropped: Vec<PathBuf>) -> Vec<Painted> {
        self.time += 0.1;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            time: Some(self.time),
            events,
            dropped_files: dropped
                .into_iter()
                .map(|path| Arc::new(Dropped(path)) as egui::DroppedFileHandle)
                .collect(),
            ..RawInput::default()
        };
        let (app, frame) = (&mut self.app, &mut self.frame);
        let mut output = self.ctx.run_ui(input, |ui| {
            app.logic(ui.ctx(), frame);
            app.ui(ui, frame);
        });
        output.textures_delta.clear();
        for command in output
            .viewport_output
            .values()
            .flat_map(|viewport| &viewport.commands)
        {
            if let ViewportCommand::Title(title) = command {
                self.titles.push(title.clone());
            }
        }
        let mut painted = Vec::new();
        for clipped in &output.shapes {
            collect(&clipped.shape, clipped.clip_rect, &mut painted);
        }
        painted
    }

    fn pass(&mut self, events: Vec<Event>) -> Vec<Painted> {
        self.pass_with(events, Vec::new())
    }

    fn press(&mut self, key: Key) -> Vec<Painted> {
        self.pass(vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        self.pass(Vec::new())
    }

    /// Clicks the visible text `text` that lies highest on screen.
    fn click(&mut self, text: &str) -> Vec<Painted> {
        let painted = self.pass(Vec::new());
        let target = painted
            .iter()
            .filter(|shape| shape.visible && shape.text == text)
            .min_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
            .unwrap_or_else(|| panic!("\"{text}\" is on screen: {:?}", on_screen(&painted)));
        let pos = target.pos + egui::vec2(4.0, 4.0);
        let button = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.pass(vec![Event::PointerMoved(pos), button(true)]);
        self.pass(vec![button(false)]);
        self.pass(Vec::new())
    }

    /// Runs passes until `done` holds, failing with `what` after a few minutes.
    fn until(&mut self, what: &str, done: impl Fn(&App) -> bool) -> Vec<Painted> {
        let deadline = Instant::now() + Duration::from_mins(4);
        loop {
            let painted = self.pass(Vec::new());
            if done(&self.app) {
                return painted;
            }
            assert!(Instant::now() < deadline, "timed out waiting until {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Runs passes until every tile shown has its thumbnail, checking that they come
    /// in grid order: at every pass, the tiles with a thumbnail are the first ones.
    fn fill(&mut self) -> Vec<Painted> {
        let deadline = Instant::now() + Duration::from_mins(4);
        loop {
            let painted = self.pass(Vec::new());
            let shown: Vec<Entry> = self.gallery().shown().into_iter().cloned().collect();
            let arrived: Vec<bool> = shown
                .iter()
                .map(|entry| self.gallery().is_ready(entry))
                .collect();
            let filled = arrived.iter().take_while(|arrived| **arrived).count();
            assert!(
                arrived[filled..].iter().all(|arrived| !arrived),
                "thumbnails arrive in grid order: {:?}",
                shown
                    .iter()
                    .zip(&arrived)
                    .map(|(entry, arrived)| format!("{} {arrived}", entry.name))
                    .collect::<Vec<_>>()
            );
            if filled == shown.len() {
                return painted;
            }
            assert!(
                Instant::now() < deadline,
                "the thumbnails never all arrived"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn shown(&self) -> Vec<String> {
        self.gallery()
            .shown()
            .into_iter()
            .map(|entry| entry.name.clone())
            .collect()
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // Hand the uploaded textures to a pass, as a renderer would, before the context goes.
        let mut output = self.ctx.run_ui(RawInput::default(), |_| {});
        output.textures_delta.clear();
    }
}

/// Collects the text shapes in `shape`, nested ones included.
fn collect(shape: &Shape, clip: Rect, out: &mut Vec<Painted>) {
    match shape {
        Shape::Text(text) => {
            let rect = text.galley.rect.translate(text.pos.to_vec2());
            out.push(Painted {
                text: text.galley.text().to_owned(),
                pos: rect.min,
                visible: clip.contains(rect.center()),
            });
        }
        Shape::Vec(shapes) => {
            for shape in shapes {
                collect(shape, clip, out);
            }
        }
        _ => {}
    }
}

fn shows(painted: &[Painted], text: &str) -> bool {
    painted
        .iter()
        .any(|shape| shape.visible && shape.text == text)
}

fn on_screen(painted: &[Painted]) -> Vec<String> {
    painted
        .iter()
        .filter(|shape| shape.visible)
        .map(|shape| format!("{} @ {:.0},{:.0}", shape.text, shape.pos.x, shape.pos.y))
        .collect()
}

#[test]
fn the_gallery_opens_and_closes_by_key_and_button() {
    let dir = scratch("open-close");
    let mut window = Window::new(&photo(&dir, 120), small_folder("open-close-presets"));
    window.pass(Vec::new());
    assert!(!window.gallery().is_open());

    let painted = window.press(Key::G);
    assert!(window.gallery().is_open(), "G opens the gallery");
    assert!(
        shows(&painted, "Filter by name, description or bend"),
        "the filter box shows: {:?}",
        on_screen(&painted)
    );
    window.press(Key::Escape);
    assert!(!window.gallery().is_open(), "Escape closes it");

    window.press(Key::G);
    window.press(Key::G);
    assert!(!window.gallery().is_open(), "G closes it again");

    window.click("Gallery");
    assert!(window.gallery().is_open(), "the Gallery button opens it");
    window.click("Close");
    assert!(!window.gallery().is_open(), "the Close button closes it");
    window.click("Gallery");
    window.click("Gallery");
    assert!(
        !window.gallery().is_open(),
        "the Gallery button closes it too"
    );
}

#[test]
fn the_gallery_lists_the_cameras_presets_and_the_filter_narrows_them() {
    let dir = scratch("filter");
    let mut window = Window::new(&photo(&dir, 120), small_folder("filter-presets"));
    window.press(Key::G);
    assert_eq!(
        window.shown(),
        ["a520-broken", "a520-brownout", "a520-hpf", "a520-warm"],
        "every preset for the A520 and the file that cannot be read, not the ZX81's"
    );
    let painted = window.pass(Vec::new());
    assert!(
        painted
            .iter()
            .any(|shape| shape.visible && shape.text.starts_with("invalid preset")),
        "the broken file's tile shows its error: {:?}",
        on_screen(&painted)
    );
    assert!(
        shows(&painted, "The amplifier's glow in a warm corner."),
        "a description shows under its name"
    );

    window.click("Filter by name, description or bend");
    let type_text = |window: &mut Window, text: &str| {
        window.pass(vec![Event::Text(text.to_owned())]);
        window.pass(Vec::new());
    };
    type_text(&mut window, "hpf");
    assert_eq!(window.shown(), ["a520-hpf"], "the filter matches names");
    for _ in 0..3 {
        window.press(Key::Backspace);
    }
    type_text(&mut window, "rail");
    assert_eq!(
        window.shown(),
        ["a520-brownout"],
        "the filter matches the bend sections a preset uses"
    );
    for _ in 0..4 {
        window.press(Key::Backspace);
    }
    type_text(&mut window, "Warm GLOW");
    assert_eq!(
        window.shown(),
        ["a520-warm"],
        "the filter matches descriptions, every word, in any case"
    );
    // A key press as the keyboard sends it: the key, then the text it types.
    window.pass(vec![
        Event::Key {
            key: Key::G,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
        Event::Text("g".to_owned()),
    ]);
    window.pass(Vec::new());
    assert!(
        window.gallery().is_open(),
        "G types into the filter instead of closing"
    );
    assert!(window.shown().is_empty(), "no preset holds \"glowg\"");
}

#[test]
fn thumbnails_arrive_for_every_shipped_preset_in_grid_order_and_stay_cached() {
    let dir = scratch("shipped");
    let mut window = Window::new(&photo(&dir, 120), root().join("presets"));
    window.press(Key::G);
    let shown = window.shown();
    let files = presets::discover(&root().join("presets")).expect("the shipped presets");
    assert_eq!(
        shown.len(),
        files.len(),
        "every shipped preset is an A520's"
    );
    let painted = window.pass(Vec::new());
    assert!(
        shows(&painted, "rendering…"),
        "tiles wait with a placeholder"
    );
    let started = Instant::now();
    let painted = window.fill();
    let elapsed = started.elapsed();
    eprintln!(
        "{} thumbnails in {:.1} s",
        shown.len(),
        elapsed.as_secs_f64()
    );
    assert!(
        !shows(&painted, "rendering…"),
        "no placeholder is left: {:?}",
        on_screen(&painted)
    );
    let requests = window.gallery().requests();
    assert_eq!(
        requests,
        shown.len() as u64,
        "one request per preset, none repeated"
    );

    window.press(Key::Escape);
    window.press(Key::G);
    for _ in 0..10 {
        window.pass(Vec::new());
    }
    assert!(window.gallery().is_open());
    assert_eq!(
        window.gallery().requests(),
        requests,
        "a second opening renders nothing again"
    );
    assert!(
        shown
            .iter()
            .zip(window.gallery().shown())
            .all(|(name, entry)| *name == entry.name && window.gallery().is_ready(entry)),
        "every thumbnail is still there"
    );
}

#[test]
fn applying_a_tile_sets_the_preset_and_closes_the_gallery() {
    let dir = scratch("apply");
    let mut window = Window::new(&photo(&dir, 120), small_folder("apply-presets"));
    window.press(Key::G);
    assert_eq!(
        window.gallery().selected().map(|entry| entry.name.as_str()),
        Some("a520-broken"),
        "an unsaved preset selects the first tile"
    );
    window.press(Key::ArrowRight);
    window.press(Key::ArrowRight);
    assert_eq!(
        window.gallery().selected().map(|entry| entry.name.as_str()),
        Some("a520-hpf"),
        "the arrows move the selection"
    );
    window.press(Key::Enter);
    assert!(!window.gallery().is_open(), "Enter applies and closes");
    assert!(
        window
            .titles
            .last()
            .is_some_and(|title| title.contains("a520-hpf")),
        "the preset is applied: {:?}",
        window.titles
    );

    window.press(Key::G);
    assert_eq!(
        window.gallery().selected().map(|entry| entry.name.as_str()),
        Some("a520-hpf"),
        "the gallery opens on the current preset"
    );
    window.click("a520-warm");
    assert!(!window.gallery().is_open(), "a click applies and closes");
    assert!(
        window
            .titles
            .last()
            .is_some_and(|title| title.contains("a520-warm")),
        "the clicked preset is applied: {:?}",
        window.titles
    );
}

#[test]
fn a_new_scene_drops_the_thumbnails_and_renders_them_again() {
    let dir = scratch("scene");
    let mut window = Window::new(&photo(&dir, 120), small_folder("scene-presets"));
    window.press(Key::G);
    window.fill();
    let first = window.gallery().requests();
    let generation = window.gallery().scene_generation();
    let shown = window.gallery().shown().len() as u64;
    assert_eq!(
        first,
        shown - 1,
        "every readable preset has a request; the broken file has none"
    );

    window.pass_with(Vec::new(), vec![photo(&dir, 40)]);
    window.until("the dropped photo is open", |app| {
        app.gallery().scene_generation() > generation
    });
    assert!(
        window
            .gallery()
            .shown()
            .iter()
            .filter(|entry| entry.name != "a520-broken")
            .all(|entry| !window.gallery().is_ready(entry)),
        "the old scene's thumbnails are gone"
    );
    window.fill();
    assert_eq!(
        window.gallery().requests(),
        2 * first,
        "every thumbnail is rendered again for the new scene"
    );
}

/// A gradient scene of `width` by `height`.
fn gradient(width: u32, height: u32) -> Arc<Scene> {
    let rgb: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            let x = (i % width * 255 / width) as u8;
            [x, 128, 255 - x]
        })
        .collect();
    Arc::new(Scene::from_srgb8(width, height, &rgb))
}

fn camera() -> Arc<Camera> {
    Arc::new(presets::load_camera(&root().join("cameras"), "a520").expect("camera"))
}

/// The thumbnail request for the shipped preset `name`.
fn thumbnail(name: &str, scene: &Arc<Scene>) -> Thumbnail {
    let path = root().join("presets").join(format!("{name}.toml"));
    Thumbnail {
        ticket: 1,
        entry: Entry {
            name: name.to_owned(),
            path: path.clone(),
        },
        preset: presets::load_preset(&path).expect("shipped"),
        camera: camera(),
        scene: Arc::clone(scene),
        scene_generation: 1,
    }
}

#[test]
fn a_thumbnail_is_frame_zero_of_a_fresh_camera_and_leaves_the_running_one_alone() {
    let scene = gradient(640, 480);
    let job = thumbnail("a520-hpf", &scene);
    let preview = Job {
        generation: 1,
        scene: Arc::clone(&scene),
        camera: Arc::clone(&job.camera),
        preset: job.preset.clone(),
        preview: true,
        power_cycle: 0,
    };
    let mut renderer = Renderer::default();
    let before = renderer
        .render(&preview)
        .outcome
        .expect("the preview renders");
    assert_eq!(before.index, 0);

    let reply = renderer.thumbnail(&job);
    assert_eq!(
        (reply.ticket, reply.path.as_path(), reply.scene_generation),
        (1, job.entry.path.as_path(), 1),
        "the reply names its request"
    );
    let output = reply.outcome.expect("the thumbnail renders");
    assert_eq!(
        (output.frame.width(), output.frame.height()),
        (GALLERY_LONG_EDGE, 224),
        "a 4:3 thumbnail, 300 pixels on the long edge; the sensor reads 224 of the 225 rows, an even count"
    );
    assert_eq!(output.index, 0);

    let full =
        Timing::derive(&job.camera, &job.preset, scene.width(), scene.height()).expect("timing");
    let aspect = f64::from(full.output_width) / f64::from(full.output_height);
    let reduced = render::reduce(&scene, aspect, GALLERY_LONG_EDGE);
    let mut fresh =
        Simulator::builder(((*job.camera).clone(), render::preview_preset(&job.preset)))
            .input_size(reduced.width(), reduced.height())
            .build()
            .expect("builds");
    assert_eq!(
        output.frame.rgb(),
        fresh.frame(&reduced, 0).rgb(),
        "the thumbnail is what a fresh camera renders first at match-input size"
    );

    let after = renderer.render(&preview).outcome.expect("renders");
    assert_eq!(
        (after.index, after.frame.width()),
        (1, PREVIEW_LONG_EDGE.min(640)),
        "the preview camera ran on from where it was"
    );
}

#[test]
fn a_preview_submitted_while_thumbnails_wait_runs_first() {
    let (started, started_rx) = mpsc::channel::<u64>();
    let (release, gate) = mpsc::channel::<()>();
    let worker = Worker::spawn_lanes(
        "fake-render",
        |&(_, lane): &(u64, Lane)| lane,
        move |(id, _): (u64, Lane)| {
            started.send(id).expect("the test listens");
            if id == 1 || id == 10 {
                gate.recv().expect("the test releases the job");
            }
            id
        },
        || {},
    )
    .expect("thread starts");
    let wait_for = |id: u64| {
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(5)),
            Ok(id),
            "job {id} starts"
        );
    };

    assert!(worker.submit((1, Lane::Newest)));
    wait_for(1);
    for id in [10, 11, 12] {
        assert!(worker.submit((id, Lane::Background)));
    }
    assert!(worker.submit((2, Lane::Newest)));
    assert!(worker.submit((3, Lane::Newest)));
    release.send(()).expect("the worker waits");
    wait_for(3);
    wait_for(10);
    assert!(
        worker.submit((4, Lane::Newest)),
        "a preview arrives while a thumbnail renders"
    );
    release.send(()).expect("the worker waits");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut replies = Vec::new();
    while replies.len() < 6 {
        assert!(
            Instant::now() < deadline,
            "worker answered only {replies:?}"
        );
        match worker.poll() {
            Some(reply) => replies.push(reply),
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    assert_eq!(
        replies,
        [1, 3, 10, 4, 11, 12],
        "the newest render runs before every waiting thumbnail, which wait in order"
    );
}

/// Asks `gallery` for thumbnails and renders them until it wants none.
fn fill(gallery: &mut Gallery, ctx: &egui::Context, scene: &Arc<Scene>) {
    let camera = camera();
    let mut renderer = Renderer::default();
    while let Some(job) = gallery.next_request(&camera, scene) {
        let reply = renderer.thumbnail(&job);
        assert!(
            gallery.receive(ctx, reply),
            "a requested thumbnail is taken in"
        );
    }
}

#[test]
fn the_cache_keeps_thumbnails_per_file_and_scene() {
    let dir = small_folder("cache");
    let list = presets::discover(&dir).expect("the folder");
    let ctx = egui::Context::default();
    let scene = gradient(160, 120);
    let mut gallery = Gallery::default();
    gallery.open(&list, "a520", "untitled");
    fill(&mut gallery, &ctx, &scene);
    let entry = |name: &str| {
        list.iter()
            .find(|entry| entry.name == name)
            .expect("listed")
            .clone()
    };
    let readable = ["a520-brownout", "a520-hpf", "a520-warm"];
    assert!(readable.iter().all(|name| gallery.is_ready(&entry(name))));
    let requests = gallery.requests();

    // A stale reply, from before the file changed, is dropped.
    let mut renderer = Renderer::default();
    let camera = camera();
    let mut warm = presets::load_preset(&entry("a520-warm").path).expect("readable");
    warm.seed = 9;
    std::fs::write(
        &entry("a520-warm").path,
        presets::to_toml(&warm).expect("serialises"),
    )
    .expect("written");
    gallery.refresh(&list);
    assert!(
        !gallery.is_ready(&entry("a520-warm")),
        "the changed file's thumbnail is dropped"
    );
    assert!(
        gallery.is_ready(&entry("a520-hpf")) && gallery.is_ready(&entry("a520-brownout")),
        "the others stay"
    );
    let job = gallery
        .next_request(&camera, &scene)
        .expect("the changed file is rendered again");
    assert_eq!(job.entry, entry("a520-warm"));
    assert_eq!(job.preset.seed, 9, "from the file as it is now");
    let mut stale = renderer.thumbnail(&job);
    stale.ticket -= 1;
    assert!(
        !gallery.receive(&ctx, stale),
        "a reply to an earlier request is dropped"
    );
    assert!(gallery.receive(&ctx, renderer.thumbnail(&job)));
    assert_eq!(gallery.requests(), requests + 1);

    gallery.scene_changed();
    assert!(
        readable.iter().all(|name| !gallery.is_ready(&entry(name))),
        "a new scene drops every thumbnail"
    );
    let old = renderer.thumbnail(&job);
    let job = gallery
        .next_request(&camera, &scene)
        .expect("the new scene is rendered");
    assert_eq!(
        job.scene_generation,
        old.scene_generation + 1,
        "the request names the new scene"
    );
    assert!(
        !gallery.receive(&ctx, old),
        "a thumbnail of the old scene is dropped"
    );

    let mut output = ctx.run_ui(RawInput::default(), |_| {});
    output.textures_delta.clear();
}

#[test]
fn the_description_round_trips_through_the_app_writer_and_is_checked() {
    let dir = scratch("description");
    let mut preset = presets::load_preset(&root().join("presets/a520-hpf.toml")).expect("shipped");
    assert!(
        preset.description.is_some(),
        "every shipped preset has a description"
    );
    preset.description = None;
    let text = presets::to_toml(&preset).expect("serialises");
    assert!(!text.contains("description"), "none is not written: {text}");

    preset.description = Some("High-pass contours from the bus bend's capacitor.".to_owned());
    let path = dir.join("described.toml");
    presets::save_preset(&path, &preset).expect("written");
    let text = std::fs::read_to_string(&path).expect("readable");
    assert!(
        text.starts_with("camera = \"a520\"\ndescription = \"High-pass contours"),
        "the description follows the camera at the top: {text}"
    );
    assert_eq!(presets::load_preset(&path).expect("read back"), preset);

    for (bad, why) in [
        ("two\nlines", "one line"),
        (&"a long paragraph ".repeat(20), "at most"),
    ] {
        let mut broken = preset.clone();
        broken.description = Some(bad.to_owned());
        let text = presets::to_toml(&broken).expect("serialises");
        let error = presets::from_toml(&text).expect_err("rejected");
        assert!(
            format!("{error:#}").contains("description") && format!("{error:#}").contains(why),
            "{error:#}"
        );
    }
}
