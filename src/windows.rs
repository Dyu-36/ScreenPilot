use std::{
    env,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{anyhow, Context, Result};

#[cfg(target_os = "windows")]
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
    RegKey,
};

#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE},
    System::Threading::CreateMutexW,
    UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
    },
};

#[cfg(target_os = "windows")]
pub struct InstanceGuard(HANDLE);

#[cfg(target_os = "windows")]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub fn acquire_single_instance() -> Result<Option<InstanceGuard>> {
    let name = wide("Local\\TackAgent.SingleInstance.v1");
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(anyhow!("không tạo được khóa single-instance của Tack Agent"));
    }

    let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already_running {
        unsafe {
            CloseHandle(handle);
        }
        return Ok(None);
    }

    Ok(Some(InstanceGuard(handle)))
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
pub fn show_pair_window(code: &str, dismissed: Arc<AtomicBool>) {
    let code = code.to_string();
    std::thread::spawn(move || {
        let message = format!(
            "Kết nối Tack Agent với Zalo\r\n\r\nTrên Zalo, nhắn cho bot:\r\n\r\n/pair {code}\r\n\r\nMã có hiệu lực trong 60 giây.\r\nSau khi ghép cặp, Tack Agent sẽ tự đăng ký chạy cùng Windows rồi đóng.\r\n\r\nNhấn OK để hủy."
        );
        message_box(
            "Tack Agent - Pair Zalo",
            &message,
            MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST,
        );
        dismissed.store(true, Ordering::SeqCst);
    });
}

#[cfg(target_os = "windows")]
pub fn show_info(message: &str) {
    message_box(
        "Tack Agent",
        message,
        MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
    );
}

#[cfg(target_os = "windows")]
pub fn show_error(message: &str) {
    message_box(
        "Tack Agent",
        message,
        MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
    );
}

#[cfg(target_os = "windows")]
fn message_box(title: &str, message: &str, flags: u32) {
    let message_w = wide(message);
    let title_w = wide(title);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message_w.as_ptr(),
            title_w.as_ptr(),
            flags,
        );
    }
}

#[cfg(target_os = "windows")]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
pub fn capture_screen(output: &Path) -> Result<()> {
    use xcap::Monitor;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).context("không tạo được thư mục lưu screenshot")?;
    }

    let monitors = Monitor::all().context("không đọc được danh sách màn hình")?;
    if monitors.is_empty() {
        return Err(anyhow!("không tìm thấy màn hình"));
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

#[cfg(not(target_os = "windows"))]
pub fn show_error(message: &str) {
    eprintln!("{message}");
}

#[cfg(not(target_os = "windows"))]
pub fn show_info(message: &str) {
    println!("{message}");
}
