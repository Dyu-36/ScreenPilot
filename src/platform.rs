// Re-export các hàm nền tảng theo hệ điều hành.
// Mỗi module nền tảng phải xuất các hàm: install_autostart, remove_autostart,
// capture_screen, show_info, show_error.
#[cfg(target_os = "windows")]
pub use crate::windows::*;

#[cfg(not(target_os = "windows"))]
pub use crate::linux::*;
