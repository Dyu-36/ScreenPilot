#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[path = "../agent.rs"]
mod agent;
#[path = "../ipc.rs"]
mod ipc;
#[path = "../media.rs"]
mod media;
#[path = "../platform.rs"]
mod platform;
#[path = "../state.rs"]
mod state;
#[path = "../ui.rs"]
mod ui;
#[path = "../zalo.rs"]
mod zalo;

#[cfg(target_os = "windows")]
#[path = "../windows.rs"]
mod windows;
#[cfg(not(target_os = "windows"))]
#[path = "../linux.rs"]
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

pub(crate) static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);
static UI_CONTEXT: OnceLock<egui::Context> = OnceLock::new();
static BACKGROUND_HANDOFF_STARTED: AtomicBool = AtomicBool::new(false);

fn main() {
    if let Err(err) = run() {
        state::append_log(&format!("ui fatal: {err:#}"));
        platform::show_error(&format!("Tack UI không thể khởi động:\n\n{err:#}"));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    if !acquire_ui_instance()? {
        return Ok(());
    }

    let config = state::load_config()?;
    if !config.bots.is_empty() {
        let _ = platform::install_autostart();
    }

    // Trong lúc GUI mở, GUI tạm tiếp quản poller để các thay đổi config/pair có hiệu lực
    // ngay. Khi GUI đóng, tack-agent.exe headless được khởi động lại.
    let shared = agent::Shared::new(config);
    agent::spawn(shared.clone());

    run_gui(shared)
}

fn acquire_ui_instance() -> Result<bool> {
    if try_acquire_ui()? {
        return Ok(true);
    }

    // Nếu owner hiện tại là headless agent, event này yêu cầu nó thoát để nhả mutex/socket.
    // Nếu owner là một tack-ui khác, event chỉ focus cửa sổ đó; process này sẽ tự thoát.
    ipc::notify_show();

    for _ in 0..30 {
        thread::sleep(Duration::from_millis(50));
        if try_acquire_ui()? {
            return Ok(true);
        }
    }

    Ok(false)
}

fn try_acquire_ui() -> Result<bool> {
    ipc::acquire_single_instance(|| {
        SHOW_REQUESTED.store(true, Ordering::SeqCst);
        if let Some(ctx) = UI_CONTEXT.get() {
            ctx.request_repaint();
        }
    })
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
            Ok(Box::new(ui::TackApp::new(shared, cc.egui_ctx.clone())))
        }),
    );

    if !BACKGROUND_HANDOFF_STARTED.load(Ordering::SeqCst) {
        if let Err(err) = spawn_background_after_exit() {
            state::append_log(&format!("cannot restore headless agent after UI exit: {err:#}"));
        }
    }

    result.map_err(|err| anyhow::anyhow!("gui error: {err}"))
}

/// agent.rs được dùng chung cho UI và headless binary. Screenshot luôn chạy trong
/// tack-capture one-shot nên GUI cũng không giữ framebuffer/GDI/WIC allocations.
pub(crate) fn capture_screen_isolated(output: &Path) -> Result<()> {
    let capture = sibling_executable("tack-capture")?;
    if !capture.exists() {
        bail!(
            "không tìm thấy {}. Hãy build/copy tack-capture cùng thư mục",
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

pub(crate) fn spawn_background_after_exit() -> Result<()> {
    if BACKGROUND_HANDOFF_STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let result = (|| -> Result<()> {
        let agent = sibling_executable("tack-agent")?;
        Command::new(&agent)
            .args(["--background", "--wait-for-instance"])
            .spawn()
            .with_context(|| format!("không khởi động được {}", agent.display()))?;
        Ok(())
    })();

    if result.is_err() {
        BACKGROUND_HANDOFF_STARTED.store(false, Ordering::SeqCst);
    }
    result
}

fn sibling_executable(stem: &str) -> Result<PathBuf> {
    let mut path = env::current_exe().context("không xác định được đường dẫn Tack UI")?;
    #[cfg(target_os = "windows")]
    path.set_file_name(format!("{stem}.exe"));
    #[cfg(not(target_os = "windows"))]
    path.set_file_name(stem);
    Ok(path)
}
