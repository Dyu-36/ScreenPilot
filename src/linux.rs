use std::{
    env,
    fs,
    path::PathBuf,
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

fn agent_path() -> PathBuf {
    // AppImage giữ launcher gốc; bản binary thường luôn trỏ sibling tack-agent.
    if let Some(appimage) = env::var_os("APPIMAGE") {
        return PathBuf::from(appimage);
    }

    let mut exe = env::current_exe().unwrap_or_else(|_| PathBuf::from("tack-agent"));
    exe.set_file_name("tack-agent");
    exe
}

pub fn install_autostart() -> Result<()> {
    let path = autostart_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("không tạo được thư mục autostart của Linux")?;
    }

    let exec = agent_path().display();
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
