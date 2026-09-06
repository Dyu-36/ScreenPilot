// Re-export các hàm nền tảng dùng bởi agent/UI.
// Screenshot nằm trong binary tack-capture riêng để agent nền không link graphics stack.
#[cfg(target_os = "windows")]
pub use crate::windows::*;

#[cfg(not(target_os = "windows"))]
pub use crate::linux::*;
