# Tack Agent

`tack-agent.exe` là agent Windows chạy ẩn, ghép cặp một lần với Zalo rồi cho phép tài khoản Zalo đã pair yêu cầu chụp màn hình từ xa.

## Flow người dùng

1. Double-click `tack-agent.exe`.
2. Cửa sổ pair hiện lệnh dạng `/pair 123456`.
3. Mã pair có hiệu lực trong **60 giây**.
4. Gửi đúng lệnh đó cho Zalo Bot.
5. Tack Agent lưu `chat_id`, đăng ký chạy cùng Windows và **đóng tiến trình pair ngay**.
6. Từ lần đăng nhập Windows kế tiếp, agent chạy với `--background`: không terminal, không popup.
7. Chỉ Zalo đã pair mới được xử lý lệnh.

Agent dùng Zalo Bot `getUpdates` long-polling trực tiếp từ máy Windows. Không cần webhook, port mở, tunnel hay public inbound URL.

## Lệnh Zalo

| Lệnh | Tác dụng |
| --- | --- |
| `/screenshot` | Chụp màn hình chính và gửi ảnh về Zalo |
| `/screen`, `/cap` | Alias của `/screenshot` |
| `/status`, `/ping` | Kiểm tra agent đang online |
| `/help`, `/start` | Xem danh sách lệnh |
| `/unpair` | Hủy pair, gỡ startup và mở mã pair mới trên PC trong 60 giây |

Tin nhắn/lệnh từ `chat_id` khác bị bỏ qua.

## Gửi screenshot

Windows screenshot được chụp bằng `xcap` và lưu tạm trong `%TEMP%\tack-agent`.

Để gửi ảnh qua Zalo Bot, agent thử upload file tạm theo thứ tự:

1. Litterbox (TTL 1 giờ)
2. Tmpfiles
3. 0x0.st
4. File.io

Sau đó URL được truyền cho Zalo `sendPhoto`. File PNG cục bộ được xóa ngay sau lần gửi.

Điều này **không yêu cầu máy Windows tự public một URL/server**, nhưng ảnh có đi qua một dịch vụ lưu file tạm trước khi Zalo tải ảnh. Nếu yêu cầu bảo mật không cho phép dịch vụ trung gian, cần thay transport media bằng một cơ chế khác được Zalo hỗ trợ.

## Bot Token

Tack Agent tìm token theo thứ tự:

1. Biến môi trường runtime `TACK_ZALO_BOT_TOKEN`.
2. Token được nhúng lúc build bằng cùng tên biến môi trường.
3. `%APPDATA%\tack-agent\config.json`.
4. Token Gotack đã lưu tại `%APPDATA%\gotack\zalo.json`.

Có thể lưu token riêng cho Tack Agent:

```powershell
.\tack-agent.exe --set-token "BOT_TOKEN"
```

Hoặc xóa token cục bộ:

```powershell
.\tack-agent.exe --clear-token
```

## Windows startup

Sau khi pair thành công, agent ghi value `TackAgent` vào:

```text
HKCU\Software\Microsoft\Windows\CurrentVersion\Run
```

Command startup trỏ tới chính file hiện tại:

```text
"C:\...\tack-agent.exe" --background
```

Vì vậy nên đặt `tack-agent.exe` ở vị trí cố định trước khi pair. Nếu di chuyển file sau đó, chạy lại `--install-startup` hoặc `--pair`.

## File trạng thái

```text
%APPDATA%\tack-agent\paired.json
%APPDATA%\tack-agent\config.json
%APPDATA%\tack-agent\agent.log
```

`agent.log` tự xoay đơn giản khi vượt khoảng 2 MB và không cố ý ghi Bot Token.

## Tùy chọn quản trị cục bộ

```text
--pair                  Ghép cặp lại
--unpair                Xóa pair và gỡ Windows startup
--set-token <token>     Lưu Bot Token Zalo riêng
--clear-token            Xóa token riêng
--install-startup        Đăng ký chạy cùng Windows
--remove-startup         Gỡ khỏi startup
--capture <path.png>     Chụp thử screenshot ra file
--background             Chạy nền; Windows startup dùng mode này
--help                   Hiện trợ giúp
```

## Single instance

Agent dùng named Windows mutex `Local\TackAgent.SingleInstance.v1`. Nếu agent nền đang chạy mà người dùng mở thêm một bản, tiến trình thứ hai sẽ thoát thay vì tạo hai long-poll consumer.

## Build

Yêu cầu Rust stable trên Windows:

```powershell
cargo test
cargo build --release
```

Output:

```text
target\release\tack-agent.exe
```

GitHub Actions PR CI cũng build trên `windows-latest`, chạy tests, verify file `.exe` và upload artifact `tack-agent-windows`.

## Source layout

```text
src/main.rs      lifecycle + pair + command dispatch
src/state.rs     pair state + token + logs
src/windows.rs   mutex + registry startup + MessageBox + screenshot
src/zalo.rs      Zalo Bot API + long polling
src/media.rs     temporary screenshot upload providers
```
