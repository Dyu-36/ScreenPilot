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
    // Dù được gọi từ tack-ui.exe, startup luôn trỏ vào binary headless.
    let exe = agent_executable()?;
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

fn agent_executable() -> Result<PathBuf> {
    let mut exe = env::current_exe().context("không xác định được thư mục Tack Agent")?;
    exe.set_file_name("tack-agent.exe");
    Ok(exe)
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
