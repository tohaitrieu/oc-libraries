//! Dòng thời gian nhiều làn, và những thao tác sửa nó.
//!
//! Hình dạng lấy đúng của MLT vì đó là hình dạng mọi bộ dựng phim dùng:
//!
//!   · **tractor** — cả dòng thời gian.
//!   · **playlist** — một LÀN. Trong một làn, kẹp nối nhau theo thứ tự, chỗ trống là "blank" thật sự
//!     chứ không phải khoảng cách suy ra. Nhờ vậy "kẹp thứ ba bắt đầu ở đâu" là một phép cộng, không
//!     phải một vòng tìm.
//!   · **producer** — một kẹp, có mốc vào và mốc ra trong tệp nguồn.
//!
//! Một làn KHÔNG chứa hai kẹp chồng nhau — đó là luật của mô hình, không phải hạn chế. Muốn chồng
//! thì phải sang làn khác, và đó chính là hành vi Tô yêu cầu: kéo chồng lên nhau thì sinh làn mới
//! cùng loại chứ không chèn bừa vào trước hay sau.
use std::ffi::CString;
use std::sync::Once;

use crate::sys;

static INIT: Once = Once::new();
static mut INIT_OK: bool = false;

/// Khởi tạo MLT đúng MỘT lần cho cả tiến trình.
///
/// `mlt_factory_init` không an toàn khi hai luồng cùng gọi — và bộ test chạy song song, nên lỗi này
/// hiện ra dưới dạng "test đầu qua, các test sau đổ", tức là trông như lỗi của mô hình chứ không
/// phải của khởi tạo. Mất một lượt đo mới thấy.
fn init_once() -> Result<(), EditError> {
    INIT.call_once(|| {
        // Đường module chôn lúc dựng; rỗng thì để MLT tự tìm theo mặc định của bản nó.
        let dir = env!("MLT_MODULE_DIR");
        let c = (!dir.is_empty()).then(|| CString::new(dir).unwrap());
        let ptr = c.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());
        unsafe { INIT_OK = !sys::mlt_factory_init(ptr).is_null() };
    });
    if unsafe { INIT_OK } {
        Ok(())
    } else {
        Err(EditError::Init(format!("không nạp được module của MLT ở `{}`", env!("MLT_MODULE_DIR"))))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Overlay,
    Audio,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: String,
    /// Mốc bắt đầu trên DÒNG THỜI GIAN, khung hình.
    pub start: i32,
    /// Số khung kẹp chiếm.
    pub length: i32,
}

#[derive(Debug)]
pub enum EditError {
    Init(String),
    NoSuchTrack(usize),
    Refused(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Init(e) => write!(f, "không khởi tạo được mô hình: {e}"),
            Self::NoSuchTrack(i) => write!(f, "không có làn số {i}"),
            Self::Refused(e) => write!(f, "thao tác bị từ chối: {e}"),
        }
    }
}

impl std::error::Error for EditError {}

pub struct Track {
    inner: sys::mlt_playlist,
    pub kind: TrackKind,
}

pub struct Timeline {
    profile: sys::mlt_profile,
    tractor: sys::mlt_tractor,
    tracks: Vec<Track>,
}

impl Timeline {
    /// Dòng thời gian rỗng ở nhịp cho trước.
    pub fn new(fps: f64, width: i32, height: i32) -> Result<Self, EditError> {
        init_once()?;
        let profile = unsafe { sys::mlt_profile_init(std::ptr::null()) };
        if profile.is_null() {
            return Err(EditError::Init("không dựng được hồ sơ khổ hình".into()));
        }
        unsafe {
            (*profile).width = width;
            (*profile).height = height;
            // MLT giữ nhịp bằng PHÂN SỐ chứ không bằng số thực: 30000/1001 là nhịp thật của NTSC, mà
            // 29.97 làm tròn thì sau vài nghìn khung là lệch hẳn một khung.
            (*profile).frame_rate_num = (fps * 1000.0).round() as i32;
            (*profile).frame_rate_den = 1000;
        }
        let tractor = unsafe { sys::mlt_tractor_new() };
        if tractor.is_null() {
            return Err(EditError::Init("không dựng được dòng thời gian".into()));
        }
        Ok(Self { profile, tractor, tracks: Vec::new() })
    }

    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// Thêm một làn. Trả về chỉ số của nó.
    pub fn add_track(&mut self, kind: TrackKind) -> usize {
        // `mlt_playlist_new(profile)` chứ KHÔNG phải `mlt_playlist_init()`.
        //
        // Bản không hồ sơ dựng được, thêm kẹp được, chỉ đổ khi cần chừa khoảng trống — vì khoảng
        // trống là một producer thật và producer cần biết nhịp với khổ hình. Lỗi hiện ra ở chỗ khác
        // hẳn nơi gây ra nó, và ở đây là nó giết cả tiến trình.
        let pl = unsafe { sys::mlt_playlist_new(self.profile) };
        let multitrack = unsafe { sys::mlt_tractor_multitrack(self.tractor) };
        unsafe {
            sys::mlt_multitrack_connect(multitrack, sys::mlt_playlist_producer(pl), self.tracks.len() as i32);
        }
        self.tracks.push(Track { inner: pl, kind });
        self.tracks.len() - 1
    }

    /// Đặt một kẹp vào làn ở mốc cho trước, chừa khoảng trống nếu cần.
    ///
    /// Không dồn kẹp phía sau: chừa chỗ là hành vi của một bản dựng có nhiều làn, còn dồn là hành vi
    /// của làn xương sống. Hai thứ đó khác nhau và trộn chúng là lý do người dùng mất cảnh mà không
    /// hiểu vì sao.
    pub fn place(&mut self, track: usize, resource: &str, start: i32, length: i32) -> Result<(), EditError> {
        let t = self.tracks.get(track).ok_or(EditError::NoSuchTrack(track))?;
        let res = CString::new(resource).map_err(|e| EditError::Refused(e.to_string()))?;
        let producer = unsafe { sys::mlt_factory_producer(self.profile, std::ptr::null(), res.as_ptr() as *mut _) };
        if producer.is_null() {
            return Err(EditError::Refused(format!("không mở được tư liệu {resource}")));
        }
        // Độ dài của làn = thời lượng phát của producer bọc nó. MLT không có hàm riêng cho playlist
        // vì playlist CHÍNH LÀ một producer — đó là lý do một làn cắm vào dòng thời gian được y như
        // một kẹp, và cũng là lý do lồng chuỗi cảnh vào nhau không cần khái niệm mới nào.
        let end = unsafe { sys::mlt_producer_get_playtime(sys::mlt_playlist_producer(t.inner)) };
        if start > end {
            unsafe { sys::mlt_playlist_blank(t.inner, start - end - 1) };
        }
        unsafe { sys::mlt_playlist_append_io(t.inner, producer, 0, length - 1) };
        Ok(())
    }

    /// Làn đầu tiên CÙNG LOẠI còn chỗ trống cho quãng này; không có thì mở làn mới.
    ///
    /// Đây là luật Tô yêu cầu, viết một chỗ: kéo chồng lên nhau thì xuống làn dưới cùng loại, không
    /// phải chèn vào trước hay sau một cách vô nghĩa.
    pub fn lane_for(&mut self, kind: TrackKind, start: i32, length: i32) -> usize {
        for (i, t) in self.tracks.iter().enumerate() {
            if t.kind != kind {
                continue;
            }
            if !self.overlaps(i, start, length) {
                return i;
            }
        }
        self.add_track(kind)
    }

    fn overlaps(&self, track: usize, start: i32, length: i32) -> bool {
        let Some(t) = self.tracks.get(track) else { return false };
        let end = start + length;
        let count = unsafe { sys::mlt_playlist_count(t.inner) };
        for i in 0..count {
            let mut info = std::mem::MaybeUninit::<sys::mlt_playlist_clip_info>::zeroed();
            let ok = unsafe { sys::mlt_playlist_get_clip_info(t.inner, info.as_mut_ptr(), i) };
            if ok != 0 {
                continue;
            }
            let info = unsafe { info.assume_init() };
            // Khoảng trống không tính là bận: đó là chỗ đặt được. Hỏi MLT chứ đừng kiểm con trỏ
            // rỗng — khoảng trống của nó là một producer THẬT, nên con trỏ vẫn khác rỗng.
            if unsafe { sys::mlt_playlist_is_blank(t.inner, i) } != 0 {
                continue;
            }
            if info.start < end && start < info.start + info.frame_count {
                return true;
            }
        }
        false
    }

    /// Các kẹp trong một làn, theo thứ tự.
    pub fn clips(&self, track: usize) -> Vec<Clip> {
        let Some(t) = self.tracks.get(track) else { return Vec::new() };
        let count = unsafe { sys::mlt_playlist_count(t.inner) };
        let mut out = Vec::new();
        for i in 0..count {
            let mut info = std::mem::MaybeUninit::<sys::mlt_playlist_clip_info>::zeroed();
            if unsafe { sys::mlt_playlist_get_clip_info(t.inner, info.as_mut_ptr(), i) } != 0 {
                continue;
            }
            if unsafe { sys::mlt_playlist_is_blank(t.inner, i) } != 0 {
                continue;
            }
            let info = unsafe { info.assume_init() };
            out.push(Clip {
                id: format!("{track}:{i}"),
                start: info.start,
                length: info.frame_count,
            });
        }
        out
    }
}

impl Drop for Timeline {
    fn drop(&mut self) {
        // Thả theo đúng thứ tự ngược: làn trước, rồi dòng thời gian, rồi hồ sơ. MLT đếm tham chiếu,
        // nhưng thả hồ sơ trước khi thả thứ đang dùng nó là đọc bộ nhớ đã trả.
        unsafe {
            for t in &self.tracks {
                sys::mlt_playlist_close(t.inner);
            }
            if !self.tractor.is_null() {
                sys::mlt_tractor_close(self.tractor);
            }
            if !self.profile.is_null() {
                sys::mlt_profile_close(self.profile);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kẹp giả không cần tệp thật: `color:` là producer sẵn có của MLT, dùng để thử mô hình mà không
    /// phải kéo theo một tệp video vào bộ test.
    const FAKE: &str = "color:red";

    #[test]
    fn overlapping_clips_open_a_new_lane_of_the_same_kind() {
        let mut tl = Timeline::new(30.0, 1080, 1920).expect("dựng dòng thời gian");
        let a = tl.lane_for(TrackKind::Overlay, 0, 90);
        tl.place(a, FAKE, 0, 90).expect("đặt kẹp đầu");

        // Kẹp thứ hai CHỒNG lên kẹp đầu (bắt đầu ở khung 30, trong khi kẹp đầu chạy 0..90).
        let b = tl.lane_for(TrackKind::Overlay, 30, 90);
        assert_ne!(a, b, "kẹp chồng nhau phải sang làn khác, không chèn vào làn đang bận");
        tl.place(b, FAKE, 30, 90).expect("đặt kẹp hai");
        assert_eq!(tl.track_count(), 2);
    }

    #[test]
    fn clips_that_do_not_touch_share_one_lane() {
        let mut tl = Timeline::new(30.0, 1080, 1920).expect("dựng dòng thời gian");
        let a = tl.lane_for(TrackKind::Overlay, 0, 60);
        tl.place(a, FAKE, 0, 60).expect("đặt kẹp đầu");
        // Bắt đầu SAU khi kẹp đầu kết thúc: mở làn mới ở đây là dòng thời gian cao gấp mười lần cần thiết.
        let b = tl.lane_for(TrackKind::Overlay, 60, 60);
        assert_eq!(a, b, "kẹp không chồng nhau phải nằm chung một làn");
    }

    #[test]
    fn a_lane_only_takes_clips_of_its_own_kind() {
        let mut tl = Timeline::new(30.0, 1080, 1920).expect("dựng dòng thời gian");
        let text = tl.lane_for(TrackKind::Overlay, 0, 60);
        let audio = tl.lane_for(TrackKind::Audio, 0, 60);
        assert_ne!(text, audio, "dải là loại phương tiện — chữ và tiếng không dùng chung làn");
    }

    #[test]
    fn a_gap_before_the_first_clip_is_kept() {
        let mut tl = Timeline::new(30.0, 1080, 1920).expect("dựng dòng thời gian");
        let lane = tl.lane_for(TrackKind::Video, 0, 60);
        tl.place(lane, FAKE, 45, 60).expect("đặt kẹp lệch khỏi mốc 0");
        let clips = tl.clips(lane);
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].start, 45, "chừa chỗ chứ không dồn kẹp về mốc 0");
    }
}
