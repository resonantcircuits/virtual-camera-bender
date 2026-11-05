//! Video sources: frames read on demand, a small cache, and the play-through stream.
//!
//! A video stays a file: [`Clip`] holds its path and stream parameters, and frames are
//! decoded when they are needed.
//!
//! - A [`Seeker`] reads frame `n` at the source size. It keeps one `ffmpeg` decoder
//!   open and reads forward from where it is; a jump backwards, or more than
//!   [`SKIP_AHEAD`] frames forwards, restarts the decoder at the frame's time. The
//!   app runs it on a [`Worker`] thread ([`decoder`]), which drains its queue to the
//!   newest request, so scrubbing costs one decode per settled position.
//! - A [`FrameCache`] keeps the most recently used decoded frames, so stepping back
//!   and forth over a few frames decodes each once.
//! - A [`PlayStream`] decodes consecutive frames from a start frame on its own thread,
//!   at a reduced size scaled by ffmpeg, a few frames ahead of the renderer. Play
//!   renders every frame it delivers, in order, so the camera's circuit state evolves
//!   as it would while recording.
//!
//! Frame `n` sits at `n / fps` seconds from the start of the stream. A decoder restarted
//! at frame `n` seeks to half a frame before that time, so the first frame ffmpeg
//! delivers is frame `n` whatever the rounding of the timestamps.

use std::collections::VecDeque;
use std::fmt::{self, Debug, Formatter};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::thread;

use anyhow::{Context as _, Result};
use eframe::egui::ColorImage;
use vcb_core::Scene;
use vcb_io::video::{ReadOptions, VideoInfo, VideoReader};

use crate::render::{PREVIEW_LONG_EDGE, Worker};
use crate::viewport;

/// Largest forward jump, in frames, that a [`Seeker`] decodes through instead of seeking.
///
/// Restarting ffmpeg and seeking costs about as much as decoding a few dozen frames of
/// a typical clip, so short jumps, such as stepping, read on.
pub const SKIP_AHEAD: u64 = 24;

/// Frames a [`PlayStream`] decodes ahead of the renderer.
///
/// Enough to hide the decoder's jitter; each waiting frame costs a preview-sized scene.
pub const PLAY_AHEAD: usize = 4;

/// Memory the full-size frame cache may use, in bytes.
///
/// A 1080p frame takes about 33 MB as a linear-light scene and its texture image, so
/// this holds about a dozen; 4K frames are held fewer at a time.
pub const CACHE_BUDGET_BYTES: usize = 384 << 20;

/// Fewest frames the cache holds, whatever their size.
const MIN_CACHED: usize = 2;

/// Most frames the cache holds, whatever their size.
const MAX_CACHED: usize = 16;

/// A video file and its stream parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// The file.
    pub path: PathBuf,
    /// Size, frame rate and length as probed.
    pub info: VideoInfo,
    /// Number of frames, as recorded or estimated from the duration; at least 1.
    pub frames: u64,
}

impl Clip {
    /// Describes the clip at `path` with the probed `info`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, info: VideoInfo) -> Self {
        let frames = info
            .frames
            .or_else(|| info.duration_s.map(|s| (s * info.fps).round() as u64))
            .unwrap_or(1)
            .max(1);
        Self {
            path: path.into(),
            info,
            frames,
        }
    }

    /// The time of frame `index` from the start of the stream, in seconds.
    #[must_use]
    pub fn time_s(&self, index: u64) -> f64 {
        index as f64 / self.info.fps
    }

    /// The length of the clip in seconds.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        self.info
            .duration_s
            .unwrap_or_else(|| self.time_s(self.frames))
    }

    /// The last frame index.
    #[must_use]
    pub fn last(&self) -> u64 {
        self.frames - 1
    }
}

/// Formats a time in seconds as `m:ss.cc`, or `h:mm:ss.cc` from an hour on.
#[must_use]
pub fn timecode(seconds: f64) -> String {
    let centis = (seconds.max(0.0) * 100.0).round() as u64;
    let (hours, minutes) = (centis / 360_000, centis / 6000 % 60);
    let (secs, frac) = (centis / 100 % 60, centis % 100);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}.{frac:02}")
    } else {
        format!("{minutes}:{secs:02}.{frac:02}")
    }
}

/// The start time that makes ffmpeg deliver frame `index` first; `None` for frame 0.
#[must_use]
pub fn seek_time(index: u64, fps: f64) -> Option<f64> {
    (index > 0).then(|| (index as f64 - 0.5) / fps)
}

/// Longest source edge to decode for previews of an output of shape `aspect`.
///
/// The preview crops the source to the output's aspect ratio and keeps
/// [`PREVIEW_LONG_EDGE`] pixels along the crop's long edge, so a source of another
/// shape must be decoded larger than that. Never above the source's own long edge.
#[must_use]
pub fn preview_dimension(source: [u32; 2], aspect: [u32; 2]) -> u32 {
    let [width, height] = source.map(|edge| f64::from(edge.max(1)));
    let ratio = f64::from(aspect[0].max(1)) / f64::from(aspect[1].max(1));
    let (crop_w, crop_h) = if width / height > ratio {
        (height * ratio, height)
    } else {
        (width, width / ratio)
    };
    let scale = (f64::from(PREVIEW_LONG_EDGE) / crop_w.max(crop_h)).min(1.0);
    let long = (width.max(height) * scale).ceil() as u32;
    long.clamp(2, source[0].max(source[1]).max(2))
}

/// The number of frames the full-size cache holds for frames of `width` by `height`.
#[must_use]
pub fn cache_capacity(width: u32, height: u32) -> usize {
    // A linear-light scene of three f32 planes and its RGBA texture image.
    let bytes = width as usize * height as usize * (3 * 4 + 4);
    (CACHE_BUDGET_BYTES / bytes.max(1)).clamp(MIN_CACHED, MAX_CACHED)
}

/// A decoded frame: the scene in front of the lens and its picture for "show original".
#[derive(Clone)]
pub struct VideoFrame {
    /// The frame as linear light.
    pub scene: Arc<Scene>,
    /// The frame as an egui image.
    pub original: ColorImage,
}

impl VideoFrame {
    /// Wraps a decoded scene and prepares its picture.
    #[must_use]
    pub fn new(scene: Scene) -> Self {
        let original = viewport::texture_image(scene.width(), scene.height(), &scene.to_srgb8());
        Self {
            scene: Arc::new(scene),
            original,
        }
    }
}

impl Debug for VideoFrame {
    /// Prints the shape only; the scene holds megabytes.
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("VideoFrame")
            .field("scene", &self.scene)
            .finish_non_exhaustive()
    }
}

/// The most recently used frames by index, up to a capacity.
///
/// [`get`](Self::get) and [`insert`](Self::insert) make a frame the most recent; an
/// insert into a full cache evicts the least recently used frame.
#[derive(Debug, Clone)]
pub struct FrameCache<T> {
    capacity: usize,
    /// Entries from the least to the most recently used.
    entries: VecDeque<(u64, T)>,
}

impl<T> FrameCache<T> {
    /// Creates an empty cache for `capacity` frames, at least one.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            entries: VecDeque::with_capacity(capacity),
        }
    }

    /// The most frames the cache holds.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The number of frames held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache holds no frame.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether frame `index` is held, without making it more recent.
    #[must_use]
    pub fn contains(&self, index: u64) -> bool {
        self.entries.iter().any(|(key, _)| *key == index)
    }

    /// The held frame indices from the least to the most recently used.
    #[must_use]
    pub fn indices(&self) -> Vec<u64> {
        self.entries.iter().map(|(key, _)| *key).collect()
    }

    /// Returns frame `index` and makes it the most recently used.
    pub fn get(&mut self, index: u64) -> Option<&T> {
        let position = self.entries.iter().position(|(key, _)| *key == index)?;
        let entry = self.entries.remove(position)?;
        self.entries.push_back(entry);
        self.entries.back().map(|(_, value)| value)
    }

    /// Stores frame `index` as the most recently used; returns the evicted frame, if any.
    ///
    /// Storing an index that is already held replaces its frame and evicts nothing.
    pub fn insert(&mut self, index: u64, value: T) -> Option<(u64, T)> {
        if let Some(position) = self.entries.iter().position(|(key, _)| *key == index) {
            self.entries.remove(position);
            self.entries.push_back((index, value));
            return None;
        }
        let evicted = if self.entries.len() >= self.capacity {
            self.entries.pop_front()
        } else {
            None
        };
        self.entries.push_back((index, value));
        evicted
    }

    /// Forgets every frame.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Reads frames of a video by index through one `ffmpeg` decoder, seeking when needed.
#[derive(Debug)]
pub struct Seeker {
    path: PathBuf,
    fps: f64,
    max_dimension: Option<u32>,
    reader: Option<VideoReader>,
    /// Index of the frame the open decoder delivers next.
    next: u64,
}

impl Seeker {
    /// Prepares to read `path`, whose frames come at `fps`, scaled to `max_dimension` if given.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, fps: f64, max_dimension: Option<u32>) -> Self {
        Self {
            path: path.into(),
            fps,
            max_dimension,
            reader: None,
            next: 0,
        }
    }

    /// Returns frame `index`, or `None` if the video ends before it.
    ///
    /// # Errors
    ///
    /// Fails if ffmpeg cannot be started or fails while decoding.
    pub fn frame(&mut self, index: u64) -> Result<Option<Scene>> {
        let near = index >= self.next && index - self.next <= SKIP_AHEAD;
        if self.reader.is_none() || !near {
            self.open(index)?;
        }
        let Some(reader) = self.reader.as_mut() else {
            return Ok(None);
        };
        while self.next < index {
            if reader.next_frame()?.is_none() {
                self.reader = None;
                return Ok(None);
            }
            self.next += 1;
        }
        let scene = reader.next_frame()?;
        if scene.is_some() {
            self.next += 1;
        } else {
            self.reader = None;
        }
        Ok(scene)
    }

    /// Starts a decoder whose first frame is frame `index`.
    fn open(&mut self, index: u64) -> Result<()> {
        self.reader = None;
        let options = ReadOptions {
            start_s: seek_time(index, self.fps),
            duration_s: None,
            max_dimension: self.max_dimension,
        };
        let reader = VideoReader::open(&self.path, options)
            .with_context(|| format!("cannot read frame {index}"))?;
        self.reader = Some(reader);
        self.next = index;
        Ok(())
    }
}

/// A frame the decoder thread answers with.
#[derive(Debug, Clone)]
pub struct Decoded {
    /// The frame index asked for.
    pub index: u64,
    /// The frame, or why it could not be read.
    pub frame: Result<VideoFrame, String>,
}

/// Starts a thread that decodes frames of `clip` at the source size, newest request first.
///
/// # Errors
///
/// Returns the operating system's error if the thread cannot be started.
pub fn decoder(
    clip: &Clip,
    notify: impl Fn() + Send + 'static,
) -> io::Result<Worker<u64, Decoded>> {
    let mut seeker = Seeker::new(&clip.path, clip.info.fps, None);
    Worker::spawn(
        "vcb-decode",
        move |index: u64| {
            let frame = match seeker.frame(index) {
                Ok(Some(scene)) => Ok(VideoFrame::new(scene)),
                Ok(None) => Err(format!("the video ends before frame {index}")),
                Err(error) => Err(format!("{error:#}")),
            };
            Decoded { index, frame }
        },
        notify,
    )
}

/// What a [`PlayStream`] has ready.
#[derive(Debug)]
pub enum Next {
    /// The next frame and its index.
    Frame(u64, VideoFrame),
    /// The decoder has not delivered the next frame yet.
    Waiting,
    /// The video has ended.
    Ended,
    /// Decoding failed.
    Failed(String),
}

/// Consecutive frames of a video, decoded ahead on their own thread.
///
/// Dropping the stream stops its thread and its ffmpeg process.
#[derive(Debug)]
pub struct PlayStream {
    frames: Receiver<Result<(u64, VideoFrame), String>>,
}

impl PlayStream {
    /// Starts decoding `clip` from frame `from`, scaled so its long edge is `max_dimension`.
    ///
    /// `notify` is called whenever a frame is ready, from the decoding thread.
    ///
    /// # Errors
    ///
    /// Returns the operating system's error if the thread cannot be started.
    pub fn start(
        clip: &Clip,
        from: u64,
        max_dimension: Option<u32>,
        notify: impl Fn() + Send + 'static,
    ) -> io::Result<Self> {
        let (sender, frames) = mpsc::sync_channel(PLAY_AHEAD);
        let path = clip.path.clone();
        let fps = clip.info.fps;
        thread::Builder::new()
            .name("vcb-play".to_owned())
            .spawn(move || stream(&path, fps, from, max_dimension, &sender, &notify))?;
        Ok(Self { frames })
    }

    /// Returns the next frame if it has been decoded.
    #[must_use]
    pub fn next(&self) -> Next {
        match self.frames.try_recv() {
            Ok(Ok((index, frame))) => Next::Frame(index, frame),
            Ok(Err(message)) => Next::Failed(message),
            Err(TryRecvError::Empty) => Next::Waiting,
            Err(TryRecvError::Disconnected) => Next::Ended,
        }
    }
}

/// The play thread: decodes frames in order until the video ends or the stream is dropped.
fn stream(
    path: &Path,
    fps: f64,
    from: u64,
    max_dimension: Option<u32>,
    sender: &SyncSender<Result<(u64, VideoFrame), String>>,
    notify: &(impl Fn() + Send),
) {
    let options = ReadOptions {
        start_s: seek_time(from, fps),
        duration_s: None,
        max_dimension,
    };
    let mut reader = match VideoReader::open(path, options) {
        Ok(reader) => reader,
        Err(error) => {
            // The stream may already be gone; then nobody waits for the error.
            let _ = sender.send(Err(error.to_string()));
            notify();
            return;
        }
    };
    let mut index = from;
    loop {
        let item = match reader.next_frame() {
            Ok(Some(scene)) => Ok((index, VideoFrame::new(scene))),
            Ok(None) => break,
            Err(error) => Err(error.to_string()),
        };
        let failed = item.is_err();
        if sender.send(item).is_err() {
            break;
        }
        notify();
        if failed {
            break;
        }
        index += 1;
    }
    notify();
}
