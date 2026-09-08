// Đo trên tệp thật: rút hình hai đầu kẹp, và rút đỉnh sóng cả tệp tiếng.
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let video = args.next().expect("dùng: measure <video> [tiếng]");
    let audio = args.next();

    let t = Instant::now();
    let mut v = media_io::video::Video::open(std::path::Path::new(&video)).expect("mở video");
    println!("mở       {:>8.1} ms · {}×{}", t.elapsed().as_secs_f64() * 1000.0, v.width(), v.height());

    // Đúng thứ dải thời gian cần: khung đầu và khung cuối của mỗi kẹp, thu về chiều cao làn.
    for (name, at) in [("khung đầu", 0.0), ("khung giữa", 5.0), ("khung cuối", 10.0)] {
        let t = Instant::now();
        match v.frame_at(at, 26) {
            Ok(f) => println!("{name:9}{:>8.1} ms · {}×{}", t.elapsed().as_secs_f64() * 1000.0, f.width, f.height),
            Err(e) => println!("{name:9}     hỏng · {e}"),
        }
    }

    if let Some(a) = audio {
        let t = Instant::now();
        match media_io::audio::peaks(std::path::Path::new(&a), 30) {
            Ok(p) => println!("đỉnh sóng{:>8.1} ms · {} điểm · {:.1} s", t.elapsed().as_secs_f64() * 1000.0, p.values.len(), p.duration),
            Err(e) => println!("đỉnh sóng     hỏng · {e}"),
        }
    }
}
