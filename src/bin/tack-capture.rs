#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
#[path = "../capture_windows.rs"]
mod capture;

#[cfg(not(target_os = "windows"))]
#[path = "../capture_linux.rs"]
mod capture;

use std::{env, path::PathBuf};

use anyhow::{Context, Result};

fn main() {
    if let Err(err) = run() {
        eprintln!("tack-capture: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("thiếu đường dẫn PNG đầu ra")?;

    capture::capture_screen(&output)
}
