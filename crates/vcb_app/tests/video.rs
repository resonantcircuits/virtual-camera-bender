//! The frame cache, timecodes, preview sizes, and frames read by seeking and by play.
//!
//! The decoding tests generate their clip with ffmpeg and are skipped, with a note on
//! stderr, when ffmpeg is not installed.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use vcb_app::video::{self, Clip, FrameCache, Next, PlayStream, Seeker};
use vcb_core::Scene;
use vcb_io::video::{ReadOptions, VideoReader, probe_video};

#[test]
fn the_frame_cache_evicts_the_least_recently_used_frame() {
    let mut cache = FrameCache::new(3);
    for index in 0..3 {
        assert_eq!(cache.insert(index, index * 10), None, "room for three");
    }
    assert_eq!(
        cache.get(0),
        Some(&0),
        "a hit makes frame 0 the most recent"
    );
    assert_eq!(
        cache.insert(3, 30),
        Some((1, 10)),
        "frame 1 is the least recent"
    );
    assert_eq!(cache.indices(), vec![2, 0, 3]);
    assert!(cache.contains(0) && !cache.contains(1));
    assert_eq!(cache.get(1), None);

    assert_eq!(
        cache.insert(2, 21),
        None,
        "replacing a frame evicts nothing"
    );
    assert_eq!(cache.indices(), vec![0, 3, 2]);
    assert_eq!(cache.get(2), Some(&21));
    assert_eq!(cache.insert(4, 40), Some((0, 0)));
    assert_eq!(cache.len(), cache.capacity());
    cache.clear();
    assert!(cache.is_empty());
}

#[test]
fn a_cache_of_capacity_zero_still_holds_one_frame() {
    let mut cache = FrameCache::new(0);
    assert_eq!(cache.capacity(), 1);
    assert_eq!(cache.insert(5, ()), None);
    assert_eq!(cache.insert(6, ()), Some((5, ())));
}

#[test]
fn the_cache_holds_fewer_frames_the_larger_they_are() {
    let small = video::cache_capacity(320, 240);
    let hd = video::cache_capacity(1920, 1080);
    let eight_k = video::cache_capacity(7680, 4320);
    assert!(small >= hd && hd >= eight_k, "{small} {hd} {eight_k}");
    assert!(hd >= 4, "a few 1080p frames fit: {hd}");
    assert_eq!(eight_k, 2, "even the largest frames keep two");
    assert!(small <= 16);
}

#[test]
fn timecodes_read_minutes_seconds_and_hundredths() {
    assert_eq!(video::timecode(0.0), "0:00.00");
    assert_eq!(video::timecode(4.8), "0:04.80");
    assert_eq!(video::timecode(61.25), "1:01.25");
    assert_eq!(video::timecode(3725.5), "1:02:05.50");
}

#[test]
fn a_seek_starts_half_a_frame_early() {
    assert_eq!(video::seek_time(0, 25.0), None);
    let start = video::seek_time(10, 25.0).expect("a later frame seeks");
    assert!((start - 0.38).abs() < 1e-12, "{start}");
}

#[test]
fn previews_decode_large_enough_for_720_px_after_the_crop() {
    assert_eq!(
        video::preview_dimension([1920, 1080], [4, 3]),
        960,
        "a 16:9 frame cropped to 4:3 needs 960 px across for a 720 px crop"
    );
    assert_eq!(video::preview_dimension([1920, 1080], [16, 9]), 720);
    assert_eq!(
        video::preview_dimension([640, 480], [4, 3]),
        640,
        "small sources are not enlarged"
    );
}

#[test]
fn a_clip_counts_its_frames_and_times() {
    let Some(path) = common::clip() else { return };
    let clip = Clip::new(path, probe_video(path).expect("probe"));
    assert_eq!(clip.frames, 20);
    assert_eq!(clip.last(), 19);
    assert!((clip.time_s(5) - 0.5).abs() < 1e-12);
    assert!((clip.duration_s() - 2.0).abs() < 0.05);
}

/// Every frame of the clip, read straight through.
fn sequential(path: &std::path::Path) -> Vec<Scene> {
    let mut reader = VideoReader::open(path, ReadOptions::default()).expect("reader");
    std::iter::from_fn(|| reader.next_frame().expect("decodes")).collect()
}

#[test]
fn seeking_lands_on_the_frame_asked_for() {
    let Some(path) = common::clip() else { return };
    let frames = sequential(path);
    assert_eq!(frames.len(), 20);
    let mut seeker = Seeker::new(path, 10.0, None);
    for index in [13, 2, 3, 4, 19, 0, 7] {
        let scene = seeker
            .frame(index)
            .expect("decodes")
            .expect("within the clip");
        assert_eq!(
            scene.to_srgb8(),
            frames[index as usize].to_srgb8(),
            "frame {index} by seeking is frame {index} in sequence"
        );
    }
    assert!(
        seeker.frame(25).expect("no error past the end").is_none(),
        "there is no frame 25"
    );
    let again = seeker.frame(1).expect("decodes").expect("within the clip");
    assert_eq!(
        again.to_srgb8(),
        frames[1].to_srgb8(),
        "the seeker recovers from the end"
    );
}

#[test]
fn the_decoder_thread_answers_with_the_frame_and_its_picture() {
    let Some(path) = common::clip() else { return };
    let clip = Clip::new(path, probe_video(path).expect("probe"));
    let decoder = video::decoder(&clip, || {}).expect("thread starts");
    assert!(decoder.submit(6));
    let deadline = Instant::now() + Duration::from_secs(20);
    let decoded = loop {
        if let Some(decoded) = decoder.poll() {
            break decoded;
        }
        assert!(Instant::now() < deadline, "the decoder answers");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(decoded.index, 6);
    let frame = decoded.frame.expect("frame 6 exists");
    assert_eq!((frame.scene.width(), frame.scene.height()), (320, 240));
    assert_eq!(frame.original.size, [320, 240]);
}

#[test]
fn play_streams_every_frame_in_order_at_the_preview_size() {
    let Some(path) = common::clip() else { return };
    let clip = Clip::new(path, probe_video(path).expect("probe"));
    let frames = sequential(path);
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let stream = PlayStream::start(&clip, 12, Some(160), move || {
        counter.fetch_add(1, Ordering::Relaxed);
    })
    .expect("thread starts");
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut indices = Vec::new();
    loop {
        assert!(Instant::now() < deadline, "play delivers: {indices:?}");
        match stream.next() {
            Next::Frame(index, frame) => {
                assert_eq!((frame.scene.width(), frame.scene.height()), (160, 120));
                indices.push(index);
            }
            Next::Waiting => std::thread::sleep(Duration::from_millis(2)),
            Next::Ended => break,
            Next::Failed(message) => panic!("play failed: {message}"),
        }
    }
    assert_eq!(
        indices,
        (12..20).collect::<Vec<u64>>(),
        "no frame is skipped"
    );
    assert!(
        wakes.load(Ordering::Relaxed) >= indices.len(),
        "the UI is woken per frame"
    );

    let full = PlayStream::start(&clip, 12, None, || {}).expect("thread starts");
    let first = loop {
        match full.next() {
            Next::Frame(index, frame) => break (index, frame),
            Next::Waiting => std::thread::sleep(Duration::from_millis(2)),
            other => panic!("expected a frame, got {other:?}"),
        }
    };
    assert_eq!(first.0, 12);
    assert_eq!(
        first.1.scene.to_srgb8(),
        frames[12].to_srgb8(),
        "play starts on the frame asked for"
    );
}
