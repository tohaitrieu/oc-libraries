//! Rút một khung ở mốc thời gian bất kỳ.
//!
//! Nhảy tới nơi rồi giải mã, không đọc tuần tự từ đầu: hình xem trước cần khung ở CUỐI kẹp cũng
//! nhiều như khung ở đầu, mà đọc tuần tự tới giây 200 để lấy một khung là vô lý.
//!
//! Nhảy luôn rơi vào khung khoá gần nhất TRƯỚC mốc, nên sau khi nhảy phải giải mã tiếp tới khi chạm
//! mốc. Bỏ bước đó thì hình trả về lệch vài giây so với chỗ người dùng đang nhìn — sai một cách khó
//! thấy, vì nó vẫn là một hình hợp lệ.
use crate::Rgba;

pub struct Video {
    input: ffmpeg_next::format::context::Input,
    stream: usize,
    decoder: ffmpeg_next::decoder::Video,
    time_base: f64,
}

#[derive(Debug)]
pub enum Error {
    Open(String),
    NoVideoStream,
    Decode(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(e) => write!(f, "không mở được tệp: {e}"),
            Self::NoVideoStream => write!(f, "tệp không có luồng hình"),
            Self::Decode(e) => write!(f, "giải mã hỏng: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl Video {
    pub fn open(path: &std::path::Path) -> Result<Self, Error> {
        ffmpeg_next::init().map_err(|e| Error::Open(e.to_string()))?;
        let input = ffmpeg_next::format::input(&path).map_err(|e| Error::Open(e.to_string()))?;
        let stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or(Error::NoVideoStream)?;
        let index = stream.index();
        let time_base = f64::from(stream.time_base());
        let decoder = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
            .map_err(|e| Error::Decode(e.to_string()))?
            .decoder()
            .video()
            .map_err(|e| Error::Decode(e.to_string()))?;
        Ok(Self { input, stream: index, decoder, time_base })
    }

    pub fn width(&self) -> u32 {
        self.decoder.width()
    }

    pub fn height(&self) -> u32 {
        self.decoder.height()
    }

    /// Khung ở mốc `at` giây, thu về đúng chiều cao yêu cầu (giữ tỉ lệ). `height = 0` là giữ nguyên khổ.
    pub fn frame_at(&mut self, at: f64, height: u32) -> Result<Rgba, Error> {
        let ts = (at / self.time_base) as i64;
        // Nhảy tới TRƯỚC mốc rồi tiến dần: nhảy tới sau mốc là lấy nhầm cảnh kế tiếp.
        let _ = self.input.seek(ts, ..ts);
        self.decoder.flush();

        let (w, h) = (self.decoder.width(), self.decoder.height());
        let out_h = if height == 0 { h } else { height };
        let out_w = ((w as f64 / h.max(1) as f64) * out_h as f64).round().max(1.0) as u32;
        let mut scaler = ffmpeg_next::software::scaling::Context::get(
            self.decoder.format(), w, h,
            ffmpeg_next::format::Pixel::RGBA, out_w, out_h,
            ffmpeg_next::software::scaling::Flags::BILINEAR,
        ).map_err(|e| Error::Decode(e.to_string()))?;

        let mut decoded = ffmpeg_next::util::frame::video::Video::empty();
        let packets: Vec<_> = self.input.packets().collect();
        for (stream, packet) in packets {
            if stream.index() != self.stream {
                continue;
            }
            if self.decoder.send_packet(&packet).is_err() {
                continue;
            }
            while self.decoder.receive_frame(&mut decoded).is_ok() {
                let pts = decoded.pts().unwrap_or(0);
                // Khung đầu tiên CHẠM hoặc vượt mốc là khung cần. Dừng ngay — giải mã tiếp là tốn
                // công cho những khung không ai xem.
                if pts >= ts {
                    let mut rgba = ffmpeg_next::util::frame::video::Video::empty();
                    scaler.run(&decoded, &mut rgba).map_err(|e| Error::Decode(e.to_string()))?;
                    return Ok(pack(&rgba, out_w, out_h));
                }
            }
        }
        Err(Error::Decode(format!("không có khung nào ở mốc {at}s")))
    }
}

/// Bỏ phần đệm cuối mỗi hàng.
///
/// Bộ giải mã căn hàng theo bội số để chạy nhanh, nên `stride` thường LỚN HƠN `width × 4`. Chép
/// thẳng cả đệm là ảnh xô chéo dần từ trên xuống — một lỗi kinh điển và nhìn ra ngay, nhưng chỉ khi
/// khổ ảnh không tình cờ chia hết.
fn pack(frame: &ffmpeg_next::util::frame::video::Video, w: u32, h: u32) -> Rgba {
    let stride = frame.stride(0);
    let row = (w as usize) * 4;
    let data = frame.data(0);
    let mut pixels = Vec::with_capacity(row * h as usize);
    for y in 0..h as usize {
        let start = y * stride;
        pixels.extend_from_slice(&data[start..start + row]);
    }
    Rgba { width: w, height: h, pixels }
}
