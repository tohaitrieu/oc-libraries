//! Đọc khung hình và đỉnh âm thanh từ tệp media.
//!
//! **Một bản cài đặt cho mọi nền tảng** — đó là cả lý do gói này tồn tại. Viết riêng cho từng hệ điều
//! hành nghe có vẻ "đúng chuẩn nền tảng", nhưng nó nhân số mã phải nuôi lên ba lần, và hai trong ba
//! bản đó không ai chạy hằng ngày nên chúng hỏng lặng lẽ.
//!
//! Gói này KHÔNG biết gì về Open Creator. Nó nhận một đường dẫn và một mốc thời gian, trả về pixel.
//! Ai muốn dùng cứ dùng.

pub mod audio;
pub mod video;

/// Ảnh RGBA8, hàng liền nhau.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
