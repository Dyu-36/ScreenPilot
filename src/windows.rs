use std::{
    env,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
    RegKey,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND,
};

use ::windows::{
    core::PCWSTR,
    Win32::{
        Foundation::GENERIC_WRITE,
        Graphics::{
            Gdi::{
                BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
                HBITMAP, HDC, HGDIOBJ, HPALETTE, ReleaseDC, SelectObject, CAPTUREBLT, SRCCOPY,
            },
            Imaging::{
                IWICImagingFactory, CLSID_WICImagingFactory, GUID_ContainerFormatPng,
                GUID_VendorMicrosoft, WICBitmapEncoderNoCache, WICBitmapIgnoreAlpha,
            },
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
    },
};

pub fn install_autostart() -> Result<()> {
    // Luôn đăng ký binary headless. Hàm này có thể được gọi từ tack-ui.exe.
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

/// Chụp màn hình chính bằng GDI và encode PNG bằng Windows Imaging Component.
/// Không dùng xcap, không tạo thêm RGBA Vec trong Rust và WIC được cấu hình no-cache.
pub fn capture_screen(output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).context("không tạo được thư mục lưu screenshot")?;
    }

    let capture = GdiCapture::primary_screen()?;
    encode_png_with_wic(capture.bitmap, output)
}

struct GdiCapture {
    screen_dc: HDC,
    memory_dc: HDC,
    bitmap: HBITMAP,
    previous_object: HGDIOBJ,
}

impl GdiCapture {
    fn primary_screen() -> Result<Self> {
        unsafe {
            let width = GetSystemMetrics(SM_CXSCREEN);
            let height = GetSystemMetrics(SM_CYSCREEN);
            if width <= 0 || height <= 0 {
                return Err(anyhow!("Windows trả kích thước màn hình không hợp lệ"));
            }

            let screen_dc = GetDC(None);
            if screen_dc.is_invalid() {
                return Err(anyhow!("GetDC không lấy được desktop DC"));
            }

            let memory_dc = CreateCompatibleDC(Some(screen_dc));
            if memory_dc.is_invalid() {
                ReleaseDC(None, screen_dc);
                return Err(anyhow!("CreateCompatibleDC thất bại"));
            }

            let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
            if bitmap.is_invalid() {
                DeleteDC(memory_dc);
                ReleaseDC(None, screen_dc);
                return Err(anyhow!("CreateCompatibleBitmap thất bại"));
            }

            let previous_object = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
            if previous_object.is_invalid() {
                DeleteObject(HGDIOBJ(bitmap.0));
                DeleteDC(memory_dc);
                ReleaseDC(None, screen_dc);
                return Err(anyhow!("SelectObject bitmap thất bại"));
            }

            if let Err(err) = BitBlt(
                memory_dc,
                0,
                0,
                width,
                height,
                Some(screen_dc),
                0,
                0,
                SRCCOPY | CAPTUREBLT,
            ) {
                SelectObject(memory_dc, previous_object);
                DeleteObject(HGDIOBJ(bitmap.0));
                DeleteDC(memory_dc);
                ReleaseDC(None, screen_dc);
                return Err(err).context("BitBlt không chụp được màn hình");
            }

            Ok(Self {
                screen_dc,
                memory_dc,
                bitmap,
                previous_object,
            })
        }
    }
}

impl Drop for GdiCapture {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.memory_dc, self.previous_object);
            DeleteObject(HGDIOBJ(self.bitmap.0));
            DeleteDC(self.memory_dc);
            ReleaseDC(None, self.screen_dc);
        }
    }
}

fn encode_png_with_wic(bitmap: HBITMAP, output: &Path) -> Result<()> {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        hr.ok().context("không khởi tạo được COM cho WIC")?;
        let _com = ComGuard;

        let factory: IWICImagingFactory = CoCreateInstance(
            &CLSID_WICImagingFactory,
            None,
            CLSCTX_INPROC_SERVER,
        )
        .context("không tạo được WIC imaging factory")?;

        // WIC dùng trực tiếp HBITMAP của GDI, tránh thêm một full-screen pixel buffer.
        let source = factory
            .CreateBitmapFromHBITMAP(bitmap, HPALETTE::default(), WICBitmapIgnoreAlpha)
            .context("WIC không nhận được GDI bitmap")?;

        let stream = factory.CreateStream().context("không tạo được WIC stream")?;
        let output_wide: Vec<u16> = output
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        stream
            .InitializeFromFilename(PCWSTR(output_wide.as_ptr()), GENERIC_WRITE.0)
            .context("WIC không mở được file PNG đầu ra")?;

        let encoder = factory
            .CreateEncoder(&GUID_ContainerFormatPng, &GUID_VendorMicrosoft)
            .context("không tạo được WIC PNG encoder")?;
        encoder
            .Initialize(&stream, WICBitmapEncoderNoCache)
            .context("không khởi tạo được WIC PNG encoder")?;

        let mut frame = None;
        let mut properties = None;
        encoder
            .CreateNewFrame(&mut frame, &mut properties)
            .context("không tạo được WIC PNG frame")?;
        let frame = frame.ok_or_else(|| anyhow!("WIC không trả PNG frame"))?;

        frame
            .Initialize(properties.as_ref())
            .context("không khởi tạo được WIC PNG frame")?;
        frame
            .WriteSource(&source, std::ptr::null())
            .context("WIC không ghi được bitmap vào PNG")?;
        frame.Commit().context("không commit được WIC PNG frame")?;
        encoder.Commit().context("không commit được file PNG")?;
    }

    Ok(())
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}
