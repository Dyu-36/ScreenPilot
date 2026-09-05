# Screenshot

Đúng ông chủ. Với **CachyOS + KDE Plasma + Wayland**, tôi khuyên **không dùng `xcap` trên Linux**. KDE đã có đường chụp native qua **Spectacle/KWin**, và Spectacle hỗ trợ chính thức chế độ background: chụp rồi thoát **không hiện GUI**, `--nonotify` còn tắt luôn notification. Trên Arch hiện manpage của Spectacle 6.7.4 vẫn có đầy đủ `--background`, `--fullscreen`, `--nonotify`, `--output`. ([Arch Linux Man Pages][1])

Nếu tool của ông chủ vẫn cần **Windows + KDE Wayland**, tôi sẽ làm hybrid như sau:

* **Windows:** Rust `xcap` trực tiếp.
* **CachyOS KDE Wayland:** Rust gọi Spectacle native của KDE.
* Không mở terminal.
* Không popup GUI.
* Không notification.
* Lưu PNG theo đường dẫn chỉ định.

### `Cargo.toml`

```toml
[package]
name = "screen_capture"
version = "1.0.0"
edition = "2024"

# Chỉ Windows mới cần xcap.
# Linux KDE Wayland không kéo xcap/X11 dependencies.
[target.'cfg(target_os = "windows")'.dependencies]
xcap = "0.9.8"

[profile.release]
opt-level = 3
lto = true
strip = true
panic = "abort"
```

### `src/main.rs`

```rust
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() {
    if let Err(err) = run() {
        let _ = write_error_log(&err.to_string());
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(default_output_path);

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    capture_screen(&output)?;

    Ok(())
}

// ==========================================================
// WINDOWS
// ==========================================================

#[cfg(target_os = "windows")]
fn capture_screen(output: &Path) -> Result<(), Box<dyn Error>> {
    use xcap::Monitor;

    let monitors = Monitor::all()?;

    if monitors.is_empty() {
        return Err("No monitor found".into());
    }

    // Primary monitor.
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .unwrap_or(&monitors[0]);

    let image = monitor.capture_image()?;

    image.save(output)?;

    Ok(())
}

// ==========================================================
// LINUX / KDE PLASMA / WAYLAND
// ==========================================================

#[cfg(target_os = "linux")]
fn capture_screen(output: &Path) -> Result<(), Box<dyn Error>> {
    use std::process::{Command, Stdio};

    let output = output
        .to_str()
        .ok_or("Invalid output path")?;

    /*
        KDE Plasma / Wayland:

        spectacle
            --background  = không mở GUI
            --nonotify    = không hiện notification
            --fullscreen  = chụp toàn desktop
            --output      = file đích
    */

    let status = Command::new("spectacle")
        .arg("--background")
        .arg("--nonotify")
        .arg("--fullscreen")
        .arg("--output")
        .arg(output)

        // Headless hoàn toàn về stdout/stderr
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())

        .status()?;

    if !status.success() {
        return Err(
            format!(
                "Spectacle failed with exit code: {:?}",
                status.code()
            )
            .into(),
        );
    }

    if !Path::new(output).exists() {
        return Err("Screenshot file was not created".into());
    }

    Ok(())
}

// ==========================================================
// UNSUPPORTED OS
// ==========================================================

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn capture_screen(_output: &Path) -> Result<(), Box<dyn Error>> {
    Err("Unsupported operating system".into())
}

// ==========================================================
// OUTPUT
// ==========================================================

fn default_output_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    executable_directory()
        .join(format!("screenshot-{timestamp}.png"))
}

fn executable_directory() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn write_error_log(message: &str) -> std::io::Result<()> {
    let log_path = executable_directory().join("screenshot-error.log");

    fs::write(log_path, message)
}
```

### Trên CachyOS của ông chủ

Kiểm tra:

```bash
echo $XDG_SESSION_TYPE
```

Kết quả đúng là:

```text
wayland
```

Kiểm tra Spectacle:

```bash
spectacle --version
```

Nếu chưa có:

```bash
sudo pacman -S spectacle
```

KDE xác nhận Spectacle có chế độ command-line background dành cho việc script screenshot mà **không hiển thị GUI hoặc yêu cầu tương tác người dùng**. ([KDE Documentation][2])

Có thể test thẳng:

```bash
spectacle \
    --background \
    --nonotify \
    --fullscreen \
    --output /tmp/test.png
```

Lệnh này tương đương:

```bash
spectacle -b -n -f -o /tmp/test.png
```

Các option đó được manpage Arch hiện tại ghi nhận chính thức. ([Arch Linux Man Pages][1])

Sau khi chạy:

```bash
ls -lh /tmp/test.png
```

### Build

Trên CachyOS:

```bash
cargo build --release
```

Binary:

```text
target/release/screen_capture
```

Chạy:

```bash
./target/release/screen_capture
```

Nó sẽ tạo:

```text
screenshot-178862....png
```

Hoặc:

```bash
./target/release/screen_capture /tmp/current-screen.png
```

**Không có Konsole nào xuất hiện.** Việc một chương trình Rust gọi `Command::new("spectacle")` không đồng nghĩa với mở terminal; Spectacle chạy background trực tiếp.

### Nếu ông chủ dùng nhiều màn hình

Hiện tại:

```rust
.arg("--fullscreen")
```

nghĩa là **toàn bộ desktop, gồm tất cả monitor**. KDE định nghĩa Full Screen là toàn desktop/all monitors. ([KDE Documentation][3])

Nếu muốn chỉ monitor hiện tại, đổi thành:

```rust
.arg("--current")
```

tức:

```bash
spectacle -b -n -m -o /tmp/test.png
```

Tôi ưu tiên kiến trúc này cho **CachyOS KDE Plasma Wayland + Windows** vì Linux dùng luôn pipeline screenshot của KWin/Spectacle thay vì cố đọc framebuffer theo cách X11. Nó cũng giúp binary Linux không phải link một đống thư viện X11/xcap.

[1]: https://man.archlinux.org/man/spectacle.1.en?utm_source=chatgpt.com "spectacle(1)  Arch manual pages"
[2]: https://docs.kde.org/stable_kf6/en/spectacle/spectacle/starting.html?utm_source=chatgpt.com "Chapter2.Starting Spectacle"
[3]: https://docs.kde.org/stable_kf6/en/spectacle/spectacle/using.html?utm_source=chatgpt.com "Chapter3.Using Spectacle"
