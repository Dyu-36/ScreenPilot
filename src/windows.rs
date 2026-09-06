use std::{env, path::Path};

use anyhow::{Context, Result};

use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
    RegKey,
};

use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND,
};

pub fn install_autostart() -> Result<()> {
    let exe = env::current_exe().context("không xác định được đường dẫn tack-agent.exe")?;
    let command = format!("\"{}\" --background", exe.display());

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .context("không mở được Windows Run registry")?;
    key.set_value("TackAgent", &command)
        .context("không đăng ký được Tack Agent chạy cùng Windows")?;
    Ok(())
}

pub fn remove_autostart() -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        KEY_SET_VALUE,
    ) {
        Ok(key) => key,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).context("không mở được Windows Run registry"),
    };

    match key.delete_value("TackAgent") {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).context("không xóa được Tack Agent khỏi Windows startup"),
    }
}

pub fn show_info(message: &str) {
    message_box(
        message,
        MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
    );
}

pub fn show_error(message: &str) {
    message_box(message, MB_OK | MB_ICONERROR | MB_SETFOREGROUND);
}

fn message_box(message: &str, flags: u32) {
    let message_w = wide(message);
    let title_w = wide("Tack Agent");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message_w.as_ptr(),
            title_w.as_ptr(),
            flags,
        );
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn capture_screen(output: &Path) -> Result<()> {
    use xcap::Monitor;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).context("không tạo được thư mục lưu screenshot")?;
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
        .context("không chụp được màn hình Windows")?;
    image.save(output).context("không lưu được screenshot PNG")?;
    Ok(())
}
