//! Mô hình dòng thời gian và **ngữ nghĩa biên tập**, dựng trên MLT.
//!
//! Đây là phần đắt nhất của một bộ dựng phim, và là phần không nên viết lại. Đo trên Kdenlive
//! 08/09/2026: phần VẼ của họ 9 650 dòng QML, phần MÔ HÌNH 23 581 dòng C++ — chèn, dồn, cắt, nhóm,
//! hít điểm, chồng lấn, hoàn tác. Cả Kdenlive lẫn Shotcut đều dựng phần đó trên MLT (285 tệp trong
//! hai kho gọi `Mlt::`).
//!
//! MLT là LGPL-2.1: liên kết ĐỘNG, người dùng thay được thư viện, và **không phải mở mã của bên
//! dùng nó**. Nghĩa vụ dừng ở chỗ cho phép thay — không ai buộc phải làm cho bản người dùng thay vào
//! chạy được.
//!
//! Gói này tồn tại vì hệ sinh thái Rust **không có** lớp nối nào còn sống: crate tên `mlt` trên
//! crates.io là một dự án khác hẳn (bản đồ), còn `mlt-sys` dừng ở 2018 cho MLT đời cũ trong khi MLT
//! nay là 7.40 và bản 7 đã đổi API lớn. Nên đây là lớp nối, không phải bản viết lại.
//!
//! **Chỉ lấy phần mô hình.** MLT còn bộ dựng và bộ phát; chỗ này đã có bộ dựng GPU riêng nhanh hơn.
#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]

mod sys {
    include!(concat!(env!("OUT_DIR"), "/mlt.rs"));
}

pub mod timeline;

pub use timeline::{Clip, EditError, Timeline, Track, TrackKind};
