#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![allow(dead_code)] // shared modules expose APIs used by tack-ui; lint is per binary.

mod agent;
mod ipc;
mod media;
mod platform;
mod state;
mod zalo;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(not(target_os = "windows"))]
mod linux;

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use anyhow::{bail, Context, Result};

#[derive(Default)]
struct Options {
    background: bool,
    capture: Option<PathBuf>,
    help: bool,
    wait_for_instance: bool,
}

fn main() {
    let options = match parse_options() {
        Ok(options) => options,
        Err(err) => {
            platform::show_error(&format!("Tham số không hợp lệ:\n\n{err:#}"));
            std::process::exit(2);
        }
    };

    let silent = options.background || options.wait_for_instance;
    if let Err(err) = run(options) {
        state::append_log(&format!("fatal: {err:#}"));
        if !silent {
            platform::show_error(&format!("Tack Agent không thể khởi động:\n\n{err:#}"));
        }
        std::process::exit(1);
    }
}

fn run(options: Options) -> Result<()> {
    if options.help {
        platform::show_info(local_help_text());
        return Ok(());
    }

    if let Some(path) = options.capture.as_deref() {
        capture_screen_isolated(path)?;
        platform::show_info(&format!("Đã chụp màn hình:\n{}", path.display()));
        return Ok(());
    }

    // tack-agent.exe không chứa GUI. Chạy không tham số chỉ mở tack-ui.exe.
    if !options.background {
        launch_ui()?;
        return Ok(());
    }

    if !acquire_background_instance(options.wait_for_instance)? {
        return Ok(());
    }

    let config = state::load_config()?;
    if config.bots.is_empty() {
        state::append_log("background skipped: no configured bots");
        return Ok(());
    }

    let _ = platform::install_autostart();

    let shared = agent::Shared::new(config);
    agent::spawn(shared);
    state::append_log("headless agent started (GUI/capture-free binary)");

    loop {
        thread::park();
    }
}

fn acquire_background_instance(wait_for_instance: bool) -> Result<bool> {
    loop {
        let acquired = ipc::acquire_single_instance(|| {
            // tack-ui.exe gửi event này trước khi nhận quyền single-instance.
            // Agent nền thoát ngay để UI có thể tiếp quản polling/config.
            state::append_log("headless agent releasing instance for UI");
            std::process::exit(0);
        })?;

        if acquired {
            return Ok(true);
        }
        if !wait_for_instance {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(75));
    }
}

fn launch_ui() -> Result<()> {
    let ui = sibling_executable("tack-ui")?;
    if !ui.exists() {
        bail!(
            "không tìm thấy {}. Hãy build/copy tack-ui cùng thư mục với tack-agent",
            ui.display()
        );
    }

    Command::new(&ui)
        .spawn()
        .with_context(|| format!("không khởi động được {}", ui.display()))?;
    Ok(())
}

/// Screenshot là process one-shot riêng. tack-agent.exe thường trú không link GDI/WIC/xcap.
pub(crate) fn capture_screen_isolated(output: &Path) -> Result<()> {
    let capture = sibling_executable("tack-capture")?;
    if !capture.exists() {
        bail!(
            "không tìm thấy {}. Hãy build/copy tack-capture cùng thư mục với tack-agent",
            capture.display()
        );
    }

    let status = Command::new(&capture)
        .arg(output)
        .status()
        .with_context(|| format!("không khởi động được {}", capture.display()))?;

    if !status.success() {
        bail!("tack-capture thoát với trạng thái {status}");
    }
    Ok(())
}

fn sibling_executable(stem: &str) -> Result<PathBuf> {
    let mut path = env::current_exe().context("không xác định được đường dẫn Tack Agent hiện tại")?;
    #[cfg(target_os = "windows")]
    path.set_file_name(format!("{stem}.exe"));
    #[cfg(not(target_os = "windows"))]
    path.set_file_name(stem);
    Ok(path)
}

fn local_help_text() -> &'static str {
    "Tack Agent 2.0\n\nKiến trúc nhẹ:\n- tack-agent: headless poll/network, không GUI, không graphics capture\n- tack-ui: chỉ chạy khi mở giao diện\n- tack-capture: one-shot, chỉ chạy khi cần screenshot\n\nTùy chọn:\n--background  Chạy agent nền nhẹ\n--capture <path.png>  Chụp thử qua tack-capture\n--help  Hiện trợ giúp\n\nWindows capture dùng GDI + WIC native; xcap không còn được dùng trên Windows."
}

fn parse_options() -> Result<Options> {
    let mut options = Options::default();
    let mut args = env::args_os().skip(1);

    while let Some(raw) = args.next() {
        let arg = raw.to_string_lossy();
        match arg.as_ref() {
            "--background" => options.background = true,
            "--help" | "-h" | "/?" => options.help = true,
            "--wait-for-instance" => options.wait_for_instance = true,
            "--capture" => {
                let path = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow::anyhow!("--capture cần đường dẫn file PNG"))?;
                options.capture = Some(path);
            }
            _ => bail!("tham số không hỗ trợ: {arg}"),
        }
    }

    Ok(options)
}
