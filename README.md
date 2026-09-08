# oc-libraries

Phần **mở** của Open Creator. Hai việc, cố ý gộp vào một chỗ:

1. **Chứa những thứ bị giấy phép ràng buộc** — thư viện giải mã theo LGPL và tương tự. Gom về một
   kho công khai thì nghĩa vụ của những giấy phép đó được làm đúng **một lần, ở một chỗ**, thay vì
   rải vào kho sản phẩm rồi không ai theo dõi nổi.
2. **Chứa phần dùng lại được** — mã nối, mô hình dữ liệu, thuật toán không phải lợi thế cạnh tranh
   của ai. Ai cũng dùng được, sửa được, đóng góp được.

Kho sản phẩm phụ thuộc vào kho này. Chiều ngược lại thì không.

## Có gì trong đây

- **`media-io`** — đọc khung hình và đỉnh âm thanh. Video qua FFmpeg, âm thanh qua `symphonia` thuần
  Rust. Một bản cài đặt cho mọi nền tảng.
- **`timeline-model`** — mô hình dòng thời gian và **ngữ nghĩa biên tập**, dựng trên MLT. Đo 08/09
  trên Kdenlive: phần vẽ của họ 9 650 dòng, phần mô hình **23 581 dòng**. Đó là phần không nên viết
  lại, và cả Kdenlive lẫn Shotcut đều lấy nó từ MLT.

## Vì sao bản cài nằm ở đây

Tệp cài phát hành theo release của kho này. Băng thông miễn phí, không giới hạn, và **tệp sống độc
lập với tên miền**: máy chủ tắt, tên miền hết hạn, tài khoản đổi — bản cài vẫn còn, vẫn tải được.
Địa chỉ nguồn phát nằm trong tệp đã ký của app, nên nó phải là địa chỉ sống lâu hơn hạ tầng.

## Giấy phép

Mã trong kho này: **Apache-2.0** (xem `LICENSE`).

Thư viện vay ngoài giữ nguyên giấy phép của chúng, kèm mã nguồn và ghi công trong `NOTICE.md`. Bản
FFmpeg dùng ở đây là bản **LGPL** — dựng không kèm `--enable-gpl`, không có x264/x265, vì việc cần là
giải mã chứ không phải mã hoá.

Người dùng **thay được** những thư viện đó — đó là điều LGPL đòi và là điều đúng nên làm. Nhưng thay
một bản không tương thích thì phần mềm không chạy, và không ai có nghĩa vụ chạy theo bản đã thay.
