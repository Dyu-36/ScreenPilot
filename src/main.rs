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
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    thread,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use eframe::egui;

static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);
static UI_CONTEXT: OnceLock<egui::Context> = OnceLock::new();
static FOREGROUND_HANDOFF_STARTED: AtomicBool = AtomicBool::new(false);
static BACKGROUND_HANDOFF_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Options {
    background: bool,
    capture: Option<PathBuf>,
    capture_worker: Option<PathBuf>,
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

    // Worker/background nội bộ không được bật MessageBox nếu có lỗi.
    let silent = options.background || options.capture_worker.is_some() || options.wait_for_instance;

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

    // Worker screenshot là process ngắn hạn: chụp xong là thoát, toàn bộ RAM xcap
    // và framebuffer được OS thu hồi ngay thay vì giữ trong agent thường trú.
    if let Some(path) = options.capture_worker.as_deref() {
        platform::capture_screen(path)?;
        return Ok(());
    }

    if let Some(path) = options.capture.as_deref() {
        capture_screen_isolated(path)?;
        platform::show_info(&format!("Đã chụp màn hình:\n{}", path.display()));
        return Ok(());
    }

    if !acquire_instance(options.background, options.wait_for_instance)? {
        // Nếu người dùng mở app trong lúc headless agent đang chạy, yêu cầu agent
        // handoff sang process GUI. Nếu GUI đã chạy, chỉ focus lại cửa sổ hiện có.
        if !options.background {
            ipc::notify_show();
        }
        return Ok(());
    }

    let config = state::load_config()?;

    // Không giữ một process nền vô ích nếu chưa hề cấu hình bot.
    if options.background && config.bots.is_empty() {
        state::append_log("background skipped: no configured bots");
        return Ok(());
    }

    if !config.bots.is_empty() {
        let _ = platform::install_autostart();
    }

    let shared = agent::Shared::new(config);
    agent::spawn(shared.clone());

    if options.background {
        state::append_log("headless background agent started");
        loop {
            thread::park();
        }
    }

    run_gui(shared)
}

fn acquire_instance(background: bool, wait_for_instance: bool) -> Result<bool> {
    loop {
        let acquired = ipc::acquire_single_instance(move || {
            if background {
                // Agent nền không load eframe. Khi người dùng mở app, spawn process
                // GUI kế nhiệm rồi thoát để giải phóng mutex/socket và toàn bộ RAM nền.
                if FOREGROUND_HANDOFF_STARTED.swap(true, Ordering::SeqCst) {
                    return;
                }
                match spawn_foreground_after_exit() {
                    Ok(()) => std::process::exit(0),
                    Err(err) => {
                        FOREGROUND_HANDOFF_STARTED.store(false, Ordering::SeqCst);
                        state::append_log(&format!("cannot hand off to GUI: {err:#}"));
                    }
                }
            } else {
                SHOW_REQUESTED.store(true, Ordering::SeqCst);
                if let Some(ctx) = UI_CONTEXT.get() {
                    ctx.request_repaint();
                }
            }
        })?;

        if acquired {
            return Ok(true);
        }
        if !wait_for_instance {
            return Ok(false);
        }

        // Process handoff đang chờ instance cũ thoát. Poll nhẹ, không busy-loop.
        thread::sleep(Duration::from_millis(75));
    }
}

fn run_gui(shared: agent::Shared) -> Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Tack Agent")
            .with_inner_size([440.0, 580.0])
            .with_visible(true),
        ..Default::default()
    };

    let result = eframe::run_native(
        "Tack Agent",
        native_options,
        Box::new(move |cc| {
            let _ = UI_CONTEXT.set(cc.egui_ctx.clone());
            Ok(Box::new(ui::TackApp::new(
                shared,
                cc.egui_ctx.clone(),
            )))
        }),
    );

    // Bình thường TackApp đã spawn background trước khi đóng. Nhánh này là fallback
    // cho các đường đóng cửa sổ mà UI framework không đưa qua update() như mong đợi.
    if !BACKGROUND_HANDOFF_STARTED.load(Ordering::SeqCst) {
        if let Err(err) = spawn_background_after_exit() {
            state::append_log(&format!("cannot restore background agent after GUI exit: {err:#}"));
        }
    }

    result.map_err(|err| anyhow::anyhow!("gui error: {err}"))
}

pub(crate) fn capture_screen_isolated(output: &Path) -> Result<()> {
    let exe = self_executable()?;
    let status = Command::new(exe)
        .arg("--capture-worker")
        .arg(output)
        .status()
        .context("không khởi động được screenshot worker")?;

    if !status.success() {
        bail!("screenshot worker thoát với trạng thái {status}");
    }
    Ok(())
}

pub(crate) fn spawn_background_after_exit() -> Result<()> {
    if BACKGROUND_HANDOFF_STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let result = spawn_self(&["--background", "--wait-for-instance"]);
    if result.is_err() {
        BACKGROUND_HANDOFF_STARTED.store(false, Ordering::SeqCst);
    }
    result
}

fn spawn_foreground_after_exit() -> Result<()> {
    spawn_self(&["--wait-for-instance"])
}

fn spawn_self(args: &[&str]) -> Result<()> {
    let exe = self_executable()?;
    Command::new(exe)
        .args(args)
        .spawn()
        .context("không khởi động được process Tack Agent kế nhiệm")?;
    Ok(())
}

fn self_executable() -> Result<PathBuf> {
    #[cfg(not(target_os = "windows"))]
    if let Some(appimage) = env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(appimage));
    }

    env::current_exe().context("không xác định được đường dẫn Tack Agent hiện tại")
}

fn local_help_text() -> &'static str {
    "Tack Agent 2.0\n\nChạy không tham số: mở cửa sổ quản lý. Nếu agent nền đang chạy, agent sẽ handoff sang GUI.\n\nTùy chọn:\n--background  Chạy agent headless, không khởi tạo eframe/egui\n--capture <path.png>  Chụp thử màn hình bằng worker ngắn hạn\n--help  Hiện trợ giúp\n\nKhi đóng/Ẩn giao diện, GUI thoát và agent headless tự chạy lại. Screenshot cũng chỉ nạp khi có lệnh chụp."
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
            "--capture-worker" => {
                let path = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow::anyhow!("--capture-worker cần đường dẫn file PNG"))?;
                options.capture_worker = Some(path);
            }
            _ => bail!("tham số không hỗ trợ: {arg}"),
        }
    }

    if options.capture.is_some() && options.capture_worker.is_some() {
        bail!("không thể dùng đồng thời --capture và --capture-worker");
    }

    Ok(options)
}
