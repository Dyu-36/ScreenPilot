use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
};

use anyhow::{anyhow, Context, Result};
use ::windows::{
    core::PCWSTR,
    Win32::{
        Foundation::GENERIC_WRITE,
        Graphics::{
            Gdi::{
                BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
                HBITMAP, HGDIOBJ, HPALETTE, ReleaseDC, SelectObject, CAPTUREBLT, SRCCOPY,
            },
            Imaging::{
                IWICImagingFactory, CLSID_WICImagingFactory, GUID_ContainerFormatPng,
                WICBitmapEncoderNoCache, WICBitmapIgnoreAlpha,
            },
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
    },
};

/// Native Windows capture: GDI -> HBITMAP -> WIC PNG.
/// Không có xcap, không tạo full-screen RGBA Vec trong Rust và encoder chạy no-cache.
pub fn capture_screen(output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).context("không tạo được thư mục lưu screenshot")?;
    }

    let bitmap = OwnedBitmap::capture_primary_screen()?;
    encode_png_with_wic(bitmap.0, output)
}

/// Chỉ giữ HBITMAP sau khi BitBlt hoàn tất. Screen DC và memory DC được nhả trước
/// khi bắt đầu WIC encode để worker giữ ít GDI resource nhất có thể.
struct OwnedBitmap(HBITMAP);

impl OwnedBitmap {
    fn capture_primary_screen() -> Result<Self> {
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

            let memory_dc = CreateCompatibleDC(screen_dc);
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

            let capture_result = BitBlt(
                memory_dc,
                0,
                0,
                width,
                height,
                screen_dc,
                0,
                0,
                SRCCOPY | CAPTUREBLT,
            );

            // Bitmap phải được bỏ khỏi memory DC trước khi xóa DC / đưa sang WIC.
            SelectObject(memory_dc, previous_object);
            DeleteDC(memory_dc);
            ReleaseDC(None, screen_dc);

            if let Err(err) = capture_result {
                DeleteObject(HGDIOBJ(bitmap.0));
                return Err(err).context("BitBlt không chụp được màn hình");
            }

            Ok(Self(bitmap))
        }
    }
}

impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}

fn encode_png_with_wic(bitmap: HBITMAP, output: &Path) -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .context("không khởi tạo được COM cho WIC")?;
        let _com = ComGuard;

        let factory: IWICImagingFactory = CoCreateInstance(
            &CLSID_WICImagingFactory,
            None,
            CLSCTX_INPROC_SERVER,
        )
        .context("không tạo được WIC imaging factory")?;

        // WIC đọc trực tiếp HBITMAP: không cần copy sang Vec<u8> kích thước toàn màn hình.
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

        // Vendor = NULL: để WIC chọn PNG encoder mặc định của Windows.
        let encoder = factory
            .CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())
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
