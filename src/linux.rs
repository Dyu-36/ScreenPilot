use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

fn autostart_path() -> Result<PathBuf> {
    let config_root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| anyhow::anyhow!("không xác định được thư mục cấu hình người dùng"))?;
    Ok(config_root.join("autostart").join("tack-agent.desktop"))
}

fn app_path() -> PathBuf {
    // Khi chạy trong AppImage, biến APPIMAGE trỏ đúng file .AppImage gốc.
    if let Some(appimage) = env::var_os("APPIMAGE") {
        return PathBuf::from(appimage);
    }
    env::current_exe().unwrap_or_else(|_| PathBuf::from("tack-agent"))
}

pub fn install_autostart() -> Result<()> {
    let path = autostart_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("không tạo được thư mục autostart của Linux")?;
    }

    let exec = app_path().display();
    let content = format!(
        "[Desktop Entry]\nType=Application\nName=Tack Agent\nComment=Agent điều khiển từ Zalo\nExec=\"{exec}\" --background\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    );
    fs::write(&path, content).context("không ghi được file autostart của Linux")?;
    Ok(())
}

pub fn remove_autostart() -> Result<()> {
    let path = autostart_path()?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).context("không gỡ được Tack Agent khỏi Linux autostart"),
    }
}

pub fn show_info(message: &str) {
    println!("{message}");
}

pub fn show_error(message: &str) {
    eprintln!("{message}");
}

pub fn capture_screen(output: &Path) -> Result<()> {
    use xcap::Monitor;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).context("không tạo được thư mục lưu screenshot")?;
    }

    let monitors = Monitor::all().context("không đọc được danh sách màn hình")?;
    if monitors.is_empty() {
        return Err(anyhow::anyhow!("không tìm thấy màn hình"));
    }

    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .unwrap_or(&monitors[0]);

    let image = monitor
        .capture_image()
        .context("không chụp được màn hình Linux (chỉ hỗ trợ X11)")?;
    image.save(output).context("không lưu được screenshot PNG")?;
    Ok(())
}
