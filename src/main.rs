#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod agent;
mod ipc;
mod media;
mod platform;
mod state;
mod ui;
mod zalo;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(not(target_os = "windows"))]
mod linux;

use std::{
    env,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
};

use anyhow::{bail, Result};
use eframe::egui;

static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);
static UI_CONTEXT: OnceLock<egui::Context> = OnceLock::new();

#[derive(Default)]
struct Options {
    background: bool,
    capture: Option<PathBuf>,
    help: bool,
}

fn main() {
    let options = match parse_options() {
        Ok(options) => options,
        Err(err) => {
            platform::show_error(&format!("Tham số không hợp lệ:\n\n{err:#}"));
            std::process::exit(2);
        }
    };
    let background = options.background;

    if let Err(err) = run(options) {
        state::append_log(&format!("fatal: {err:#}"));
        if !background {
            platform::show_error(&format!("Tack Agent không thể khởi động:\n\n{err:#}"));
        }
        std::process::exit(1);
    }
    // run() chỉ return ở các đường thoát sớm (--help, --capture, instance thứ hai).
}

fn run(options: Options) -> Result<()> {
    if options.help {
        platform::show_info(local_help_text());
        return Ok(());
    }

    if let Some(path) = options.capture.as_deref() {
        platform::capture_screen(path)?;
        platform::show_info(&format!("Đã chụp màn hình:\n{}", path.display()));
        return Ok(());
    }

    if !ipc::acquire_single_instance(|| {
        SHOW_REQUESTED.store(true, Ordering::SeqCst);
        if let Some(ctx) = UI_CONTEXT.get() {
            ctx.request_repaint();
        }
    })? {
        // Đã có tiến trình Tack Agent đang chạy: chỉ yêu cầu mở lại cửa sổ.
        if !options.background {
            ipc::notify_show();
        }
        return Ok(());
    }

    let config = state::load_config()?;
    if !config.bots.is_empty() {
        let _ = platform::install_autostart();
    }

    let shared = agent::Shared::new(config);
    agent::spawn(shared.clone());

    let shared_for_ui = shared;
    let start_hidden = options.background;

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Tack Agent")
            .with_inner_size([440.0, 580.0])
            .with_visible(!start_hidden),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Tack Agent",
        native_options,
        Box::new(move |cc| {
            let _ = UI_CONTEXT.set(cc.egui_ctx.clone());
            Ok(Box::new(ui::TackApp::new(
                shared_for_ui,
                cc.egui_ctx.clone(),
            )))
        }),
    );

    if let Err(err) = result {
        state::append_log(&format!("gui error: {err:#}"));
        if !start_hidden {
            platform::show_error(&format!("Lỗi giao diện Tack Agent:\n\n{err}"));
        }
    }

    // eframe không bao giờ kết thúc bình thường (cửa sổ chỉ bị ẩn, không thoát),
    // nhưng nếu có sự cố thì vẫn giữ tiến trình để agent tiếp tục chạy nền.
    loop {
        std::thread::park();
    }
}

fn local_help_text() -> &'static str {
    "Tack Agent 2.0\n\nChạy không tham số: mở cửa sổ quản lý (lần đầu sẽ yêu cầu nhập Bot Token Zalo và ghép cặp).\n\nTùy chọn:\n--background  Khởi động ẩn cửa sổ (Windows startup / Linux autostart dùng tùy chọn này)\n--capture <path.png>  Chụp thử màn hình ra file\n--help  Hiện trợ giúp\n\nAgent chỉ tắt khi bạn kết thúc tiến trình trong Task Manager (Windows) hoặc lệnh kill (Linux)."
}

fn parse_options() -> Result<Options> {
    let mut options = Options::default();
    let mut args = env::args_os().skip(1);

    while let Some(raw) = args.next() {
        let arg = raw.to_string_lossy();
        match arg.as_ref() {
            "--background" => options.background = true,
            "--help" | "-h" | "/?" => options.help = true,
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
