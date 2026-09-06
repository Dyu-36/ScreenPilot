use std::{env, path::PathBuf};

use anyhow::{Context, Result};
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
    RegKey,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND,
};

pub fn install_autostart() -> Result<()> {
    let exe = manager_executable()?;
    let command = format!("\"{}\" --background", exe.display());

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .context("không mở được Windows Run registry")?;
    key.set_value("Manager", &command)
        .context("không đăng ký được Manager chạy cùng Windows")?;
    // Dọn tên startup cũ nếu còn tồn tại.
    let _ = key.delete_value("TackAgent");
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

    let _ = key.delete_value("TackAgent");
    match key.delete_value("Manager") {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).context("không xóa được Manager khỏi Windows startup"),
    }
}

fn manager_executable() -> Result<PathBuf> {
    let mut exe = env::current_exe().context("không xác định được thư mục Manager")?;
    exe.set_file_name("Manager.exe");
    Ok(exe)
}

pub fn show_info(message: &str) {
    message_box(message, MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND);
}

pub fn show_error(message: &str) {
    message_box(message, MB_OK | MB_ICONERROR | MB_SETFOREGROUND);
}

fn message_box(message: &str, flags: u32) {
    let message_w = wide(message);
    let title_w = wide("Manager");
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
