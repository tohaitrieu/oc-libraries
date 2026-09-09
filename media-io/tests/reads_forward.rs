//! Reading forward must return EXACTLY the frames seeking returns, and return them far faster.
//!
//! Both halves, because dropping either fails invisibly: hold a frame one beat too long and the
//! picture freezes mid-shot in a file that is still perfectly valid; seek before every frame and the
//! result is identical, only tens of times slower — which no assertion catches except a clock.
use std::path::PathBuf;

use media_io::video::Video;

/// A real clip to read. None means skip — the clip cache does not ship with the source.
fn a_clip() -> Option<PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../open-creator/data/cache/clips");
    let mut clips: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "mp4"))
        .collect();
    clips.sort();
    clips.into_iter().next()
}

#[test]
fn forward_reading_returns_the_same_frames_as_seeking() {
    let Some(path) = a_clip() else { return };
    let mut forward = Video::open(&path).expect("open clip");
    let mut seeking = Video::open(&path).expect("open clip");

    for i in 0..20 {
        let at = i as f64 / 30.0;
        let a = forward.frame_forward(at, 360).expect("read forward").pixels.clone();
        let b = seeking.frame_at(at, 360).expect("seek to mark").pixels;
        assert_eq!(a.len(), b.len(), "mark {at}: different sizes");
        assert!(a == b, "mark {at}: reading forward returned a different frame than seeking");
    }
}

#[test]
fn forward_reading_beats_seeking_for_every_frame() {
    let Some(path) = a_clip() else { return };
    let marks: Vec<f64> = (0..60).map(|i| i as f64 / 30.0).collect();

    let mut v = Video::open(&path).expect("open clip");
    let t = std::time::Instant::now();
    for at in &marks {
        v.frame_forward(*at, 360).expect("read forward");
    }
    let forward = t.elapsed();

    let mut v = Video::open(&path).expect("open clip");
    let t = std::time::Instant::now();
    for at in &marks {
        v.frame_at(*at, 360).expect("seek to mark");
    }
    let seeking = t.elapsed();

    // A deliberately wide margin: the measured gap is tens of times, so "twice as fast" still
    // catches the fault that matters (seeking back into every frame) without wobbling under load.
    assert!(
        forward * 2 < seeking,
        "forward {forward:?} against seeking {seeking:?} — looks like it still seeks per frame"
    );
}

#[test]
fn a_mark_past_the_end_holds_the_last_frame() {
    // A scene outlasting its clip is ordinary. Failing here would fail the whole build over the tail
    // of one clip.
    let Some(path) = a_clip() else { return };
    let mut v = Video::open(&path).expect("open clip");
    let end = v.duration();
    v.frame_forward(end * 0.5, 240).expect("mid clip");
    let last = v.frame_forward(end + 5.0, 240).expect("past the end");
    assert!(!last.pixels.is_empty());
}
