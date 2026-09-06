use std::{fs, path::Path};

use anyhow::{Context, Result};
use xcap::Monitor;

pub fn capture_screen(output: &Path) -> Result<()> {
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
