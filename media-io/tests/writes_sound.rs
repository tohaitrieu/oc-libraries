//! A video with sound is judged by reopening it and finding the sound.
//!
//! The failure this guards against is silence: an encoder returns success on every call, the file
//! plays, the picture is right, and nobody notices there is no audio track until someone watches the
//! whole thing. So every check here reopens the file and asks the container what it holds.
use std::path::PathBuf;

use media_io::{writer, Rgba};

fn spec(with_sound: bool) -> writer::Spec {
    writer::Spec {
        width: 320,
        height: 240,
        fps: 25,
        video_bitrate: 1_000_000,
        audio: with_sound.then(|| writer::AudioSpec { sample_rate: 48_000, channels: 2, bitrate: 128_000 }),
    }
}

fn frame() -> Rgba {
    Rgba { width: 320, height: 240, pixels: [30u8, 90, 200, 255].iter().cycle().take(320 * 240 * 4).copied().collect() }
}

/// One second of a 440 Hz tone, interleaved stereo — something a decoder can find again.
fn tone(seconds: f64) -> Vec<f32> {
    let n = (48_000.0 * seconds) as usize;
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let v = ((i as f64 / 48_000.0) * 440.0 * std::f64::consts::TAU).sin() as f32 * 0.25;
        out.push(v);
        out.push(v);
    }
    out
}

fn out(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("media-io-sound-{name}.mp4"))
}

/// `Ok(writer)`, or `None` when this machine has no encoder — a real answer, not a failure.
fn open_or_skip(path: &PathBuf, spec: &writer::Spec) -> Option<writer::Writer> {
    match writer::open(path, spec) {
        Ok(w) => Some(w),
        Err(writer::Error::NoEncoder(why)) => {
            eprintln!("skipped: no encoder on this machine ({why})");
            None
        }
        Err(e) => panic!("opening the writer failed: {e}"),
    }
}

#[test]
fn a_file_written_with_sound_has_an_audio_track() {
    let path = out("basic");
    let s = spec(true);
    let Some(mut w) = open_or_skip(&path, &s) else { return };
    for _ in 0..50 {
        w.push(&frame()).expect("push a frame");
    }
    w.push_audio(&tone(2.0)).expect("push samples");
    let bytes = w.finish().expect("close the file");
    assert!(bytes > 1000, "{bytes} bytes — too small to be a video");

    let mut v = media_io::video::Video::open(&path).expect("reopen the file just written");
    assert_eq!((v.width(), v.height()), (320, 240));
    v.frame_at(0.5, 0).expect("a frame from the middle");

    let peaks = media_io::audio::peaks(&path, 10).expect("read the sound back");
    let loudest = peaks.values.iter().cloned().fold(0.0f32, f32::max);
    assert!(loudest > 0.05, "the track is silent: loudest peak {loudest}");
    assert!(peaks.duration > 1.5, "the sound is short: {} seconds", peaks.duration);
}

#[test]
fn samples_that_do_not_fill_a_frame_still_reach_the_file() {
    // AAC takes whole frames only, so a remainder waits inside the writer. Dropping it cuts the last
    // fraction of a second — which is exactly where the last word of a sentence sits.
    let path = out("remainder");
    let s = spec(true);
    let Some(mut w) = open_or_skip(&path, &s) else { return };
    for _ in 0..25 {
        w.push(&frame()).expect("push a frame");
    }
    // 1000 samples per channel: far less than one AAC frame.
    w.push_audio(&tone(1.0)).expect("push samples");
    w.push_audio(&vec![0.2f32; 2000]).expect("push a remainder");
    w.finish().expect("close the file");

    let peaks = media_io::audio::peaks(&path, 4).expect("read the sound back");
    let loudest = peaks.values.iter().cloned().fold(0.0f32, f32::max);
    assert!(loudest > 0.05, "the track is silent: loudest peak {loudest}");
}

#[test]
fn asking_a_silent_writer_for_sound_says_so() {
    // The distinction matters: a caller that thinks it delivered sound has no reason to go looking
    // for the sound that is missing.
    let path = out("silent");
    let s = spec(false);
    let Some(mut w) = open_or_skip(&path, &s) else { return };
    w.push(&frame()).expect("push a frame");
    assert!(w.push_audio(&[0.0; 128]).is_err());
}
