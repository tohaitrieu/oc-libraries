// Sinh lớp gọi hàm C từ chính header của MLT.
//
// Không gõ tay chữ ký hàm: MLT có hàng trăm hàm, và một chữ ký gõ sai không báo lỗi lúc dựng — nó
// đổ ở lúc chạy, ở chỗ khác, dưới dạng bộ nhớ hỏng. Máy đọc header thì không gõ sai.
//
// Tìm thư viện bằng `pkg-config` chứ không cắm đường dẫn cứng: Homebrew trên máy Apple Silicon nằm
// ở `/opt/homebrew`, trên Intel ở `/usr/local`, còn Linux thì mỗi bản một chỗ.
fn main() {
    let mlt = pkg_config::Config::new()
        .atleast_version("7.0")
        .probe("mlt-framework-7")
        .expect("không thấy MLT — cài `mlt` rồi đặt PKG_CONFIG_PATH cho đúng");

    // Đường tới MODULE của MLT, chôn vào lúc dựng.
    //
    // `mlt_factory_init(NULL)` chỉ tìm module ở đường mặc định lúc MLT được biên dịch — với bản
    // Homebrew thì đường đó không phải chỗ nó thật sự nằm, và hàm trả con trỏ RỖNG chứ không báo gì.
    // Bên gọi không có cách nào đoán ra, nên hỏi `pkg-config` ngay đây.
    let moduledir = std::process::Command::new("pkg-config")
        .args(["--variable=moduledir", "mlt-framework-7"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    println!("cargo:rustc-env=MLT_MODULE_DIR={moduledir}");

    let includes: Vec<String> = mlt.include_paths.iter()
        .map(|p| format!("-I{}", p.display()))
        .collect();

    let bindings = bindgen::Builder::default()
        .header_contents("mlt.h", "#include <framework/mlt.h>")
        .clang_args(&includes)
        // Chỉ lấy phần MÔ HÌNH. MLT còn cả bộ dựng và bộ phát; mình đã có bộ dựng GPU nhanh hơn,
        // nên kéo cả vào là mang theo thứ không dùng và một mặt tiếp xúc rộng hơn cần thiết.
        .allowlist_function("mlt_(factory|profile|producer|playlist|tractor|properties|multitrack)_.*")
        .allowlist_type("mlt_.*")
        .allowlist_var("MLT_.*|mlt_.*")
        .derive_debug(true)
        .generate()
        .expect("sinh lớp gọi hàm hỏng");

    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings.write_to_file(out.join("mlt.rs")).expect("ghi lớp gọi hàm hỏng");
}
