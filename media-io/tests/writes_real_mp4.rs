//! The writer is judged by the file it leaves behind, not by its return codes.
//!
//! Every check here reopens the mp4 with this crate's own reader. An encoder can return success on
//! every call and still write a file no player will open — wrong dimensions, a missing trailer, one
//! keyframe and nothing after it. Reading it back is the only claim worth making.
//!
//! No encoder on this machine is NOT a failure: `NoEncoder` is a real answer that sends the job to a
//! server. These tests skip on it rather than turning a supported situation into a red build.
use std::path::PathBuf;

use media_io::{writer, Rgba};

fn spec(width: u32, height: u32, fps: u32) -> writer::Spec {
    writer::Spec { width, height, fps, video_bitrate: 2_000_000, audio: None }
}

/// A frame filled with one colour, so a decoded pixel can be compared against what went in.
fn frame(width: u32, height: u32, rgba: [u8; 4]) -> Rgba {
    Rgba { width, height, pixels: rgba.iter().cycle().take((width * height * 4) as usize).copied().collect() }
}

fn out(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("media-io-{name}.mp4"))
}

/// `Ok(writer)`, or `None` when this machine has no encoder — see the module note.
fn open_or_skip(path: &PathBuf, spec: &writer::Spec) -> Option<writer::Writer> {
    match writer::open(path, spec) {
        Ok(w) => Some(w),
        Err(writer::Error::NoEncoder(why)) => {
            eprintln!("bỏ qua: máy này không có bộ mã hoá ({why})");
            None
        }
        Err(e) => panic!("mở bộ ghi hỏng: {e}"),
    }
}

#[test]
fn writes_a_file_the_reader_can_open() {
    let path = out("basic");
    let s = spec(320, 240, 25);
    let Some(mut w) = open_or_skip(&path, &s) else { return };
    for _ in 0..25 {
        w.push(&frame(320, 240, [200, 40, 40, 255])).expect("đẩy khung");
    }
    let bytes = w.finish().expect("đóng tệp");
    assert!(bytes > 0, "tệp rỗng");

    let mut v = media_io::video::Video::open(&path).expect("đọc lại tệp vừa ghi");
    assert_eq!((v.width(), v.height()), (320, 240));
    // One second at 25 fps. Encoders round duration, so compare loosely rather than pretend to a
    // precision the container does not carry.
    assert!((v.duration() - 1.0).abs() < 0.2, "độ dài {} giây", v.duration());
    v.frame_at(0.5, 240).expect("rút được một khung ở giữa");
}

#[test]
fn the_last_frames_are_not_lost() {
    // An encoder holds frames back; forgetting to flush at EOF simply ends the video early, and
    // nothing about that looks wrong — the file still opens and still plays.
    let path = out("flush");
    let s = spec(320, 240, 30);
    let Some(mut w) = open_or_skip(&path, &s) else { return };
    for _ in 0..90 {
        w.push(&frame(320, 240, [20, 120, 200, 255])).expect("đẩy khung");
    }
    w.finish().expect("đóng tệp");

    let v = media_io::video::Video::open(&path).expect("đọc lại");
    assert!(v.duration() > 2.5, "mất khung cuối: chỉ còn {} giây", v.duration());
}

#[test]
fn an_existing_file_is_replaced_not_refused() {
    let path = out("overwrite");
    std::fs::write(&path, b"old junk").expect("dựng tệp cũ");
    let s = spec(160, 120, 25);
    let Some(mut w) = open_or_skip(&path, &s) else { return };
    w.push(&frame(160, 120, [255, 255, 255, 255])).expect("đẩy khung");
    w.finish().expect("đóng tệp");
    media_io::video::Video::open(&path).expect("tệp cũ đã bị thay, không phải bị từ chối");
}

#[test]
fn odd_dimensions_are_refused_with_a_reason() {
    // h264 in 4:2:0 cannot express odd sizes. Failing here with the numbers beats a muxer error
    // that reads like a bug in the caller's arithmetic.
    let err = writer::open(&out("odd"), &spec(321, 240, 25)).err().expect("phải từ chối");
    assert!(matches!(err, writer::Error::Failed(ref m) if m.contains("321")), "{err}");
}
