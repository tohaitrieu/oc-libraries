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
    let track = format.default_track().ok_or(Error::NoAudioStream)?;
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
