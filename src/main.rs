#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

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

    // Screenshot vẫn chạy ở worker ngắn hạn để bất kỳ allocation GDI/WIC nào
    // cũng được OS thu hồi hoàn toàn ngay sau khi chụp.
    if let Some(path) = options.capture_worker.as_deref() {
        platform::capture_screen(path)?;
        return Ok(());
    }

    if let Some(path) = options.capture.as_deref() {
        capture_screen_isolated(path)?;
        platform::show_info(&format!("Đã chụp màn hình:\n{}", path.display()));
        return Ok(());
    }

    // tack-agent.exe không chứa GUI. Chạy không tham số chỉ mở binary tack-ui.exe.
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
    state::append_log("headless agent started (GUI-free binary)");

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

pub(crate) fn capture_screen_isolated(output: &Path) -> Result<()> {
    let exe = current_program_executable()?;
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

fn sibling_executable(stem: &str) -> Result<PathBuf> {
    let mut path = current_program_executable()?;
    #[cfg(target_os = "windows")]
    path.set_file_name(format!("{stem}.exe"));
    #[cfg(not(target_os = "windows"))]
    path.set_file_name(stem);
    Ok(path)
}

fn current_program_executable() -> Result<PathBuf> {
    env::current_exe().context("không xác định được đường dẫn Tack Agent hiện tại")
}

fn local_help_text() -> &'static str {
    "Tack Agent 2.0\n\ntack-agent.exe là process nền thuần headless, không link GUI.\nChạy không tham số: mở tack-ui.exe nằm cùng thư mục.\n\nTùy chọn:\n--background  Chạy agent nền nhẹ\n--capture <path.png>  Chụp thử màn hình bằng worker ngắn hạn\n--help  Hiện trợ giúp\n\nWindows dùng GDI + WIC native; xcap chỉ còn dùng cho nền tảng không phải Windows."
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
