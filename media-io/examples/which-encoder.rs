//! Which video encoder this machine can actually open, and whether it writes a file.
//!
//! A build log says which encoders were compiled in; it cannot say which ones OPEN. On Android those
//! differ: `h264_mediacodec` is in the build and still refuses when a bitstream filter it needs was
//! stripped out. The answer has to come from the device.
//!
//! Deliberately tiny: no GPU, no template, no fonts. Push the binary and the libraries to a phone,
//! run it, read one line. Anything larger turns a yes/no question into another debugging session.
//!
//!   cargo build --release --target aarch64-linux-android --example which-encoder
//!   adb push <binary> /data/local/tmp/ && adb shell LD_LIBRARY_PATH=/data/local/tmp \
//!     /data/local/tmp/which-encoder out.mp4 1080 1920
fn main() {
    let path = std::path::PathBuf::from(
        std::env::args().nth(1).unwrap_or_else(|| "/data/local/tmp/which-encoder.mp4".into()),
    );
    // Size matters to a hardware encoder: a phone's MediaCodec advertises h264 and still refuses
    // sizes outside the profile it implements, so the probe has to be asked at the size that will
    // actually be used.
    let width: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(320);
    let height: u32 = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(240);
    let spec = media_io::writer::Spec {
        width,
        height,
        fps: 25,
        video_bitrate: 1_000_000,
        audio: None,
    };
    let mut writer = match media_io::writer::open(&path, &spec) {
        Ok(w) => {
            println!("opened an encoder");
            w
        }
        Err(e) => {
            println!("no encoder: {e}");
            std::process::exit(1);
        }
    };
    let frame = media_io::Rgba {
        width,
        height,
        pixels: [40u8, 90, 200, 255].iter().cycle().take((width * height * 4) as usize).copied().collect(),
    };
    for _ in 0..25 {
        if let Err(e) = writer.push(&frame) {
            println!("pushing a frame failed: {e}");
            std::process::exit(1);
        }
    }
    match writer.finish() {
        Ok(bytes) => println!("wrote {bytes} bytes to {}", path.display()),
        Err(e) => {
            println!("closing the file failed: {e}");
            std::process::exit(1);
        }
    }
}
