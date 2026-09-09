//! Đỉnh âm thanh cho sóng trên dải thời gian.
//!
//! Tính sẵn theo KHUNG HÌNH, không theo cột pixel: cột pixel đổi mỗi lần phóng to thu nhỏ, nên tính
//! theo cột là tính lại từ đầu sau mỗi cú cuộn. Khung hình thì cố định — tính một lần, vẽ mãi.
//!
//! Lấy **đỉnh** (trị tuyệt đối lớn nhất trong khoảng), không lấy trung bình bình phương: sóng vẽ ra
//! để người ta thấy chỗ nào to chỗ nào im, mà trung bình làm phẳng mất đúng những chỗ đó.
//!
//! Thuần Rust, không thư viện hệ thống — chạy được cả nơi không nạp được FFmpeg.
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Đỉnh của một tệp tiếng: mỗi phần tử là đỉnh của một khoảng đều nhau.
pub struct Peaks {
    pub points_per_second: u32,
    /// Đỉnh trong khoảng 0..=1.
    pub values: Vec<f32>,
    pub duration: f64,
}

#[derive(Debug)]
pub enum Error {
    Open(String),
    Decode(String),
    NoAudioStream,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(e) => write!(f, "không mở được tệp tiếng: {e}"),
            Self::Decode(e) => write!(f, "giải mã tiếng hỏng: {e}"),
            Self::NoAudioStream => write!(f, "tệp không có luồng tiếng"),
        }
    }
}

impl std::error::Error for Error {}

/// Đọc cả tệp và rút đỉnh. `points_per_second` là độ mịn của sóng.
pub fn peaks(path: &Path, points_per_second: u32) -> Result<Peaks, Error> {
    let file = std::fs::File::open(path).map_err(|e| Error::Open(e.to_string()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| Error::Open(e.to_string()))?;
    let mut format = probed.format;
    // The first track a decoder can be BUILT for, not the default one.
    //
    // `default_track` on an mp4 that carries video hands back the video track, and the failure that
    // follows says "unsupported codec" — which reads like the file being wrong rather than the wrong
    // track being picked. A voice-only file hides this completely, because there the default track
    // IS the audio.
    let track = format
        .tracks()
        .iter()
        .find(|t| symphonia::default::get_codecs().get_codec(t.codec_params.codec).is_some())
        .ok_or(Error::NoAudioStream)?;
    let track_id = track.id;
    let rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| Error::Decode(e.to_string()))?;

    let per_point = (rate / points_per_second.max(1)).max(1) as usize;
    let mut values: Vec<f32> = Vec::new();
    let mut peak = 0.0_f32;
    let mut seen = 0_usize;
    let mut total = 0_usize;
    let mut buf: Option<SampleBuffer<f32>> = None;

    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else { continue };
        // Đọc số kênh TRƯỚC khi mượn `decoded` để chép — chép xong thì nó đi mất.
        let channels = decoded.spec().channels.count().max(1);
        let sb = buf.get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, *decoded.spec()));
        sb.copy_interleaved_ref(decoded);
        // Gộp kênh bằng cách lấy đỉnh của cả khung mẫu: sóng trên dải là một vệt, không phải hai.
        for frame in sb.samples().chunks(channels) {
            let m = frame.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
            peak = peak.max(m);
            seen += 1;
            total += 1;
            if seen >= per_point {
                values.push(peak.min(1.0));
                peak = 0.0;
                seen = 0;
            }
        }
    }
    if seen > 0 {
        values.push(peak.min(1.0));
    }
    Ok(Peaks { points_per_second, values, duration: total as f64 / rate as f64 })
}

/// A voice track read forward, a chunk at a time.
///
/// Streaming and not "decode the whole file": five minutes of 48 kHz stereo is 288 MB as `f32`, and
/// on a phone that is not memory anyone has spare. The render loop asks for one frame's worth of
/// sound at a time, so nothing bigger than that is ever held.
///
/// Samples come back INTERLEAVED at the file's own rate and channel count — the same shape the
/// writer takes, so nothing in between has to know how they are laid out.
pub struct Samples {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    rate: u32,
    channels: usize,
    /// Decoded but not yet handed out. A packet holds more than one request usually asks for.
    held: std::collections::VecDeque<f32>,
    done: bool,
}

impl Samples {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let file = std::fs::File::open(path).map_err(|e| Error::Open(e.to_string()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| Error::Open(e.to_string()))?;
        let format = probed.format;
        // The first track a decoder exists for, for the reason `peaks` gives: on a file that carries
        // video, the default track is the video one.
        let track = format
            .tracks()
            .iter()
            .find(|t| symphonia::default::get_codecs().get_codec(t.codec_params.codec).is_some())
            .ok_or(Error::NoAudioStream)?;
        let track_id = track.id;
        let rate = track.codec_params.sample_rate.unwrap_or(48_000);
        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1).max(1);
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| Error::Decode(e.to_string()))?;
        Ok(Self { format, decoder, track_id, rate, channels, held: Default::default(), done: false })
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The next `frames` samples PER CHANNEL, interleaved.
    ///
    /// Past the end it returns silence rather than nothing: a scene table can outlast its voice
    /// track by a fraction of a second, and ending the video there would cut the picture short.
    pub fn take(&mut self, frames: usize) -> Vec<f32> {
        let want = frames * self.channels;
        while self.held.len() < want && !self.done {
            match self.format.next_packet() {
                Ok(packet) => {
                    if packet.track_id() != self.track_id {
                        continue;
                    }
                    let Ok(decoded) = self.decoder.decode(&packet) else { continue };
                    let mut sb = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                    sb.copy_interleaved_ref(decoded);
                    self.held.extend(sb.samples().iter().copied());
                }
                Err(_) => self.done = true,
            }
        }
        let mut out: Vec<f32> = self.held.drain(..want.min(self.held.len())).collect();
        out.resize(want, 0.0);
        out
    }
}
