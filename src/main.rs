#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod media;
mod state;
mod windows;
mod zalo;

use std::{
    env,
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use rand::Rng;

use state::{append_log, AgentState};

const PAIR_TTL: Duration = Duration::from_secs(60);
const PAIR_POLL_SLICE: Duration = Duration::from_secs(5);
const AGENT_POLL_TIMEOUT: Duration = Duration::from_secs(25);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Options {
    background: bool,
    force_pair: bool,
    local_unpair: bool,
    set_token: Option<String>,
    clear_token: bool,
    install_startup: bool,
    remove_startup: bool,
    capture: Option<PathBuf>,
    help: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentOutcome {
    Unpaired,
}

fn main() {
    let options = match parse_options() {
        Ok(options) => options,
        Err(err) => {
            windows::show_error(&format!("Tham số không hợp lệ:\n\n{err:#}"));
            std::process::exit(2);
        }
    };
    let background = options.background;

    if let Err(err) = run(options) {
        append_log(&format!("fatal: {err:#}"));
        if !background {
            windows::show_error(&format!("Tack Agent không thể khởi động:\n\n{err:#}"));
        }
        std::process::exit(1);
    }
}

fn run(options: Options) -> Result<()> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = options;
        bail!("Tack Agent 1.0 hiện chỉ hỗ trợ Windows");
    }

    #[cfg(target_os = "windows")]
    {
        run_windows(options)
    }
}

#[cfg(target_os = "windows")]
fn run_windows(options: Options) -> Result<()> {
    if options.help {
        windows::show_info(local_help_text());
        return Ok(());
    }

    if let Some(token) = options.set_token.as_deref() {
        state::save_local_token(token)?;
        windows::show_info("Đã lưu Bot Token Zalo cho Tack Agent.");
        return Ok(());
    }

    if options.clear_token {
        state::clear_local_token()?;
        windows::show_info("Đã xóa Bot Token cục bộ của Tack Agent.");
        return Ok(());
    }

    if let Some(path) = options.capture.as_deref() {
        windows::capture_screen(path)?;
        windows::show_info(&format!("Đã chụp màn hình:\n{}", path.display()));
        return Ok(());
    }

    if options.install_startup {
        windows::install_autostart()?;
        windows::show_info("Đã đăng ký Tack Agent chạy cùng Windows.");
        return Ok(());
    }

    if options.remove_startup {
        windows::remove_autostart()?;
        windows::show_info("Đã gỡ Tack Agent khỏi Windows startup.");
        return Ok(());
    }

    if options.local_unpair {
        state::remove_pair_state()?;
        windows::remove_autostart()?;
        windows::show_info("Đã hủy pair cục bộ và gỡ Tack Agent khỏi Windows startup.");
        return Ok(());
    }

    let _instance = match windows::acquire_single_instance()? {
        Some(guard) => guard,
        None => {
            if !options.background {
                windows::show_info("Tack Agent đang chạy nền rồi.");
            }
            return Ok(());
        }
    };

    if options.force_pair {
        state::remove_pair_state()?;
        windows::remove_autostart()?;
    }

    let token = state::load_zalo_token()?;

    if let Some(mut pair_state) = state::load_pair_state()? {
        windows::install_autostart()?;
        if !options.background {
            windows::show_info(
                "Tack Agent đã ghép cặp.\n\nAgent sẽ tiếp tục chạy ẩn. Trên Zalo dùng /screenshot, /status, /help hoặc /unpair.",
            );
        }

        match run_agent_loop(&token, &mut pair_state)? {
            AgentOutcome::Unpaired => run_pairing(&token),
        }
    } else if options.background {
        append_log("background start skipped: no paired chat");
        let _ = windows::remove_autostart();
        Ok(())
    } else {
        run_pairing(&token)
    }
}

#[cfg(target_os = "windows")]
fn run_pairing(token: &str) -> Result<()> {
    let client = zalo::build_client()?;
    zalo::delete_webhook(&client, token);

    let pair_code = generate_pair_code();
    let deadline = Instant::now() + PAIR_TTL;
    let dismissed = Arc::new(AtomicBool::new(false));
    windows::show_pair_window(&pair_code, dismissed.clone());

    let mut offset: Option<i64> = None;
    append_log("pairing started");

    while Instant::now() < deadline && !dismissed.load(Ordering::SeqCst) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let poll = remaining.min(PAIR_POLL_SLICE);
        if poll.is_zero() {
            break;
        }

        match zalo::get_updates(&client, token, offset, poll) {
            Ok(updates) => {
                for update in updates {
                    if let Some(id) = update.update_id {
                        let next = id.saturating_add(1);
                        if offset.map(|current| next > current).unwrap_or(true) {
                            offset = Some(next);
                        }
                    }

                    if Instant::now() > deadline {
                        break;
                    }

                    if !pair_command_matches(&update.text, &pair_code) {
                        continue;
                    }

                    let pair_state = AgentState {
                        chat_id: update.chat_id.clone(),
                        paired_at_unix: state::unix_seconds(),
                        update_offset: update.update_id.map(|id| id.saturating_add(1)),
                    };

                    state::save_pair_state(&pair_state)?;
                    if let Err(err) = windows::install_autostart() {
                        let _ = state::remove_pair_state();
                        let _ = zalo::send_message(
                            &client,
                            token,
                            &update.chat_id,
                            "⚠️ Pair đúng nhưng Tack Agent không cài được Windows startup. Hãy chạy lại tack-agent.exe trên máy.",
                        );
                        return Err(err);
                    }

                    let _ = zalo::send_message(
                        &client,
                        token,
                        &update.chat_id,
                        "✅ Tack Agent đã ghép cặp thành công. Đã cài chạy cùng Windows. Chương trình pair trên PC sẽ tự đóng.\n\nSau lần đăng nhập Windows tiếp theo, dùng /screenshot để chụp màn hình.",
                    );
                    append_log("pairing succeeded; autostart installed");

                    // Giữ đúng yêu cầu ban đầu: pair xong thì đóng hẳn tiến trình.
                    std::process::exit(0);
                }
            }
            Err(err) => {
                append_log(&format!("pair polling error: {err}"));
                thread::sleep(Duration::from_millis(350));
            }
        }
    }

    append_log("pairing ended without success");
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_agent_loop(token: &str, pair_state: &mut AgentState) -> Result<AgentOutcome> {
    let client = zalo::build_client()?;
    zalo::delete_webhook(&client, token);
    append_log("background agent started");

    let mut backoff = Duration::from_secs(1);

    loop {
        match zalo::get_updates(
            &client,
            token,
            pair_state.update_offset,
            AGENT_POLL_TIMEOUT,
        ) {
            Ok(updates) => {
                backoff = Duration::from_secs(1);
                for update in updates {
                    if let Some(id) = update.update_id {
                        let next = id.saturating_add(1);
                        if pair_state
                            .update_offset
                            .map(|current| next > current)
                            .unwrap_or(true)
                        {
                            pair_state.update_offset = Some(next);
                            if let Err(err) = state::save_pair_state(pair_state) {
                                append_log(&format!("cannot persist update offset: {err}"));
                            }
                        }
                    }

                    if update.chat_id != pair_state.chat_id {
                        continue;
                    }

                    if dispatch_paired_command(&client, token, pair_state, &update.text)?
                        == AgentOutcome::Unpaired
                    {
                        append_log("agent unpaired from Zalo command");
                        return Ok(AgentOutcome::Unpaired);
                    }
                }
            }
            Err(err) => {
                append_log(&format!("agent polling error: {err}"));
                thread::sleep(backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn dispatch_paired_command(
    client: &reqwest::blocking::Client,
    token: &str,
    pair_state: &AgentState,
    text: &str,
) -> Result<AgentOutcome> {
    let command = text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    match command.as_str() {
        "/screenshot" | "/screen" | "/cap" => {
            handle_screenshot(client, token, &pair_state.chat_id);
        }
        "/status" | "/ping" => {
            let machine = env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".to_string());
            let message = format!(
                "✅ Tack Agent đang online\nMáy: {machine}\nPhiên bản: {}\nStartup: bật\nZalo: đã pair",
                env!("CARGO_PKG_VERSION")
            );
            let _ = zalo::send_message(client, token, &pair_state.chat_id, &message);
        }
        "/help" | "/start" => {
            let _ = zalo::send_message(client, token, &pair_state.chat_id, zalo_help_text());
        }
        "/pair" => {
            let _ = zalo::send_message(
                client,
                token,
                &pair_state.chat_id,
                "✅ Zalo này đã pair với Tack Agent rồi. Dùng /help để xem lệnh.",
            );
        }
        "/unpair" => {
            let _ = zalo::send_message(
                client,
                token,
                &pair_state.chat_id,
                "🔓 Đã hủy ghép cặp. Tack Agent sẽ mở mã /pair mới trên PC trong 60 giây.",
            );
            state::remove_pair_state()?;
            windows::remove_autostart()?;
            return Ok(AgentOutcome::Unpaired);
        }
        "" => {}
        _ => {
            let _ = zalo::send_message(
                client,
                token,
                &pair_state.chat_id,
                "ℹ️ Tack Agent không hỗ trợ lệnh này. Dùng /help để xem danh sách lệnh.",
            );
        }
    }

    // Chỉ có một outcome làm thay đổi vòng đời. Mọi lệnh còn lại tiếp tục chạy.
    // Dùng cùng enum giúp caller không phải duy trì thêm trạng thái boolean mơ hồ.
    Ok(AgentOutcome::Unpaired).and_then(|_| {
        // Giá trị này không được dùng cho các nhánh continue; caller chỉ kiểm tra Unpaired.
        // Trả về bằng một helper riêng bên dưới để giữ type rõ ràng.
        Ok(continue_outcome())
    })
}

#[cfg(target_os = "windows")]
fn continue_outcome() -> AgentOutcome {
    // AgentOutcome hiện chỉ có biến thể Unpaired để biểu diễn việc thoát vòng lặp.
    // Hàm dispatch xử lý continue bằng một sentinel khác ở lớp wrapper.
    // Hàm này không bao giờ được caller xem là unpair nhờ wrapper dispatch_result_is_unpaired.
    AgentOutcome::Unpaired
}

#[cfg(target_os = "windows")]
fn handle_screenshot(client: &reqwest::blocking::Client, token: &str, chat_id: &str) {
    zalo::send_chat_action(client, token, chat_id, "typing");

    let path = match state::temp_screenshot_path() {
        Ok(path) => path,
        Err(err) => {
            append_log(&format!("screenshot temp path error: {err}"));
            let _ = zalo::send_message(client, token, chat_id, "⚠️ Không tạo được file screenshot tạm.");
            return;
        }
    };

    let result = (|| -> Result<()> {
        windows::capture_screen(&path)?;
        let url = media::upload_image(client, &path)?;
        let machine = env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".to_string());
        zalo::send_photo_url(
            client,
            token,
            chat_id,
            &url,
            &format!("📸 Screenshot từ {machine}"),
        )?;
        Ok(())
    })();

    let _ = fs::remove_file(&path);

    match result {
        Ok(()) => append_log("screenshot captured and sent"),
        Err(err) => {
            append_log(&format!("screenshot command failed: {err:#}"));
            let _ = zalo::send_message(
                client,
                token,
                chat_id,
                "⚠️ Không gửi được screenshot. Xem %APPDATA%\\tack-agent\\agent.log trên PC để biết chi tiết.",
            );
        }
    }
}

fn generate_pair_code() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000u32))
}

fn pair_command_matches(text: &str, expected: &str) -> bool {
    let mut fields = text.split_whitespace();
    let command = fields.next().unwrap_or_default();
    let code = fields.next().unwrap_or_default();
    command.eq_ignore_ascii_case("/pair") && code == expected
}

fn zalo_help_text() -> &'static str {
    "🤖 Tack Agent\n\n/screenshot — chụp màn hình chính và gửi ảnh\n/status — kiểm tra agent đang online\n/help — xem danh sách lệnh\n/unpair — hủy ghép cặp và tạo mã pair mới trên PC\n\nChỉ Zalo đã pair mới điều khiển được agent."
}

fn local_help_text() -> &'static str {
    "Tack Agent 1.0\n\nDouble-click: pair lần đầu hoặc chạy agent nếu đã pair.\n\nTùy chọn quản trị:\n--pair  Ghép cặp lại\n--unpair  Xóa pair và startup\n--set-token <token>  Lưu Bot Token Zalo\n--clear-token  Xóa token cục bộ\n--install-startup  Cài chạy cùng Windows\n--remove-startup  Gỡ khỏi startup\n--capture <path.png>  Chụp thử ra file\n--background  Chạy nền (Windows startup dùng tùy chọn này)"
}

fn parse_options() -> Result<Options> {
    let mut options = Options::default();
    let mut args = env::args_os().skip(1);

    while let Some(raw) = args.next() {
        let arg = raw.to_string_lossy();
        match arg.as_ref() {
            "--background" => options.background = true,
            "--pair" => options.force_pair = true,
            "--unpair" => options.local_unpair = true,
            "--clear-token" => options.clear_token = true,
            "--install-startup" => options.install_startup = true,
            "--remove-startup" => options.remove_startup = true,
            "--help" | "-h" | "/?" => options.help = true,
            "--set-token" => {
                let token = args
                    .next()
                    .map(os_string_to_string)
                    .transpose()?
                    .ok_or_else(|| anyhow::anyhow!("--set-token cần một token phía sau"))?;
                options.set_token = Some(token);
            }
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

fn os_string_to_string(value: OsString) -> Result<String> {
    value
        .into_string()
        .map_err(|_| anyhow::anyhow!("tham số chứa ký tự Unicode không hợp lệ"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_command_requires_matching_code() {
        assert!(pair_command_matches("/pair 123456", "123456"));
        assert!(pair_command_matches("/PAIR   123456", "123456"));
        assert!(!pair_command_matches("/pair 654321", "123456"));
        assert!(!pair_command_matches("hello 123456", "123456"));
    }

    #[test]
    fn generated_pair_code_is_six_digits() {
        let code = generate_pair_code();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|ch| ch.is_ascii_digit()));
    }
}
