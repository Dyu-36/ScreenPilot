#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::{
    env,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context, Result};
use rand::Rng;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const ZALO_API_BASE: &str = "https://bot-api.zaloplatforms.com";
const PAIR_TTL: Duration = Duration::from_secs(60);
const POLL_SLICE: Duration = Duration::from_secs(5);

#[derive(Debug, Serialize, Deserialize)]
struct PairState {
    chat_id: String,
    paired_at_unix: u64,
}

#[derive(Debug)]
struct ZaloUpdate {
    update_id: Option<i64>,
    chat_id: String,
    text: String,
}

fn main() {
    if let Err(err) = run() {
        show_error(&format!("Tack Agent không thể khởi động:\n\n{err:#}"));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    #[cfg(not(target_os = "windows"))]
    {
        bail!("Bản tack-agent hiện tại chỉ dùng cho Windows");
    }

    #[cfg(target_os = "windows")]
    {
        run_windows_pairing()
    }
}

#[cfg(target_os = "windows")]
fn run_windows_pairing() -> Result<()> {
    let token = load_zalo_token()?;
    let pair_code = generate_pair_code();
    let deadline = Instant::now() + PAIR_TTL;

    let dismissed = Arc::new(AtomicBool::new(false));
    show_pair_window(&pair_code, dismissed.clone());

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .build()
        .context("không tạo được HTTP client")?;

    let mut offset: Option<i64> = None;

    while Instant::now() < deadline && !dismissed.load(Ordering::SeqCst) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let poll = remaining.min(POLL_SLICE);
        if poll.is_zero() {
            break;
        }

        match get_updates(&client, &token, offset, poll) {
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

                    if pair_command_matches(&update.text, &pair_code) {
                        save_pair_state(&update.chat_id)?;
                        let _ = send_message(
                            &client,
                            &token,
                            &update.chat_id,
                            "✅ Tack Agent đã ghép cặp thành công. Chương trình trên Windows sẽ tự đóng.",
                        );

                        // Pair thành công: kết thúc ngay. Windows sẽ đóng luôn cửa sổ pair.
                        std::process::exit(0);
                    }
                }
            }
            Err(err) => {
                // Lỗi mạng tạm thời không làm mất cửa sổ pair; tiếp tục thử cho đến hết 60 giây.
                let _ = write_log(&format!("zalo poll error: {err:#}"));
                std::thread::sleep(Duration::from_millis(350));
            }
        }
    }

    // Hết 60 giây hoặc người dùng bấm OK: chương trình tự đóng.
    Ok(())
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

fn get_updates(
    client: &Client,
    token: &str,
    offset: Option<i64>,
    poll_timeout: Duration,
) -> Result<Vec<ZaloUpdate>> {
    let timeout_seconds = poll_timeout.as_secs().max(1);
    let mut body = json!({"timeout": timeout_seconds});
    if let Some(offset) = offset {
        body["offset"] = json!(offset);
    }

    let value = api_call(client, token, "getUpdates", &body, poll_timeout + Duration::from_secs(15))?;
    Ok(parse_updates(value))
}

fn send_message(client: &Client, token: &str, chat_id: &str, text: &str) -> Result<()> {
    let body = json!({
        "chat_id": chat_id,
        "text": text,
    });
    let _ = api_call(client, token, "sendMessage", &body, Duration::from_secs(15))?;
    Ok(())
}

fn api_call(
    client: &Client,
    token: &str,
    method: &str,
    body: &Value,
    timeout: Duration,
) -> Result<Value> {
    let url = format!("{ZALO_API_BASE}/bot{token}/{method}");
    let response = client
        .post(url)
        .timeout(timeout)
        .json(body)
        .send()
        .with_context(|| format!("Zalo {method}: lỗi kết nối"))?;

    let status = response.status();
    let envelope: Value = response
        .json()
        .with_context(|| format!("Zalo {method}: phản hồi JSON không hợp lệ"))?;

    let ok = envelope.get("ok").and_then(Value::as_bool);
    if !status.is_success() || ok == Some(false) {
        let code = envelope
            .get("error_code")
            .and_then(Value::as_i64)
            .unwrap_or(status.as_u16() as i64);

        // Zalo dùng HTTP/API 408 cho long-poll không có update.
        if method == "getUpdates" && code == 408 {
            return Ok(Value::Array(Vec::new()));
        }

        let description = envelope
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        bail!("Zalo {method} failed ({code}): {description}");
    }

    Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
}

fn parse_updates(value: Value) -> Vec<ZaloUpdate> {
    let values: Vec<Value> = match value {
        Value::Array(items) => items,
        Value::Object(mut object) => {
            if let Some(Value::Array(items)) = object.remove("updates") {
                items
            } else if let Some(Value::Array(items)) = object.remove("data") {
                items
            } else {
                vec![Value::Object(object)]
            }
        }
        Value::Null => Vec::new(),
        other => vec![other],
    };

    values.into_iter().filter_map(parse_update).collect()
}

fn parse_update(value: Value) -> Option<ZaloUpdate> {
    let object = value.as_object()?;
    let message = object
        .get("message")
        .and_then(Value::as_object)
        .unwrap_or(object);

    let update_id = loose_i64(object.get("update_id")).or_else(|| loose_i64(object.get("id")));

    let chat_id = loose_string(message.get("chat_id"))
        .or_else(|| {
            message
                .get("chat")
                .and_then(Value::as_object)
                .and_then(|chat| loose_string(chat.get("id")))
        })
        .or_else(|| loose_string(object.get("chat_id")))?;

    let text = loose_string(message.get("text"))
        .or_else(|| loose_string(message.get("caption")))
        .or_else(|| loose_string(message.get("message")))
        .unwrap_or_default();

    Some(ZaloUpdate {
        update_id,
        chat_id,
        text,
    })
}

fn loose_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn loose_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn load_zalo_token() -> Result<String> {
    if let Ok(token) = env::var("TACK_ZALO_BOT_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }

    if let Some(token) = option_env!("TACK_ZALO_BOT_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }

    let gotack_state = appdata_dir().join("gotack").join("zalo.json");
    if let Ok(data) = fs::read(&gotack_state) {
        if let Ok(value) = serde_json::from_slice::<Value>(&data) {
            if let Some(token) = value.get("token").and_then(Value::as_str) {
                let token = token.trim().to_string();
                if !token.is_empty() {
                    return Ok(token);
                }
            }
        }
    }

    bail!(
        "Không tìm thấy Bot Token Zalo. Hãy cấu hình Zalo trong Gotack trước, hoặc đặt biến môi trường TACK_ZALO_BOT_TOKEN."
    )
}

fn save_pair_state(chat_id: &str) -> Result<()> {
    let dir = appdata_dir().join("tack-agent");
    fs::create_dir_all(&dir).context("không tạo được thư mục cấu hình tack-agent")?;

    let state = PairState {
        chat_id: chat_id.to_string(),
        paired_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };

    let data = serde_json::to_vec_pretty(&state)?;
    let path = dir.join("paired.json");
    let tmp = dir.join("paired.json.tmp");
    fs::write(&tmp, data).context("không ghi được trạng thái pair")?;
    fs::rename(&tmp, &path).context("không lưu được trạng thái pair")?;
    Ok(())
}

fn appdata_dir() -> PathBuf {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("AppData").join("Roaming")))
        .unwrap_or_else(|| env::temp_dir())
}

fn executable_directory() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn write_log(message: &str) -> std::io::Result<()> {
    fs::write(executable_directory().join("tack-agent-error.log"), message)
}

#[cfg(target_os = "windows")]
fn show_pair_window(code: &str, dismissed: Arc<AtomicBool>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
    };

    let code = code.to_string();
    std::thread::spawn(move || {
        let message = format!(
            "Kết nối Tack Agent với Zalo\r\n\r\nTrên Zalo, nhắn cho bot:\r\n\r\n/pair {code}\r\n\r\nMã có hiệu lực trong 60 giây.\r\nChương trình sẽ tự đóng ngay khi ghép cặp thành công.\r\n\r\nNhấn OK để hủy."
        );
        let title = "Tack Agent - Pair Zalo";
        let message_w = wide(&message);
        let title_w = wide(title);

        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message_w.as_ptr(),
                title_w.as_ptr(),
                MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST,
            );
        }
        dismissed.store(true, Ordering::SeqCst);
    });
}

#[cfg(target_os = "windows")]
fn show_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND,
    };

    let message_w = wide(message);
    let title_w = wide("Tack Agent");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message_w.as_ptr(),
            title_w.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn show_error(message: &str) {
    eprintln!("{message}");
}

#[cfg(target_os = "windows")]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

// Giữ sẵn screenshot engine Windows cho bước /screenshot tiếp theo.
#[cfg(target_os = "windows")]
#[allow(dead_code)]
fn capture_screen(output: &Path) -> Result<()> {
    use xcap::Monitor;

    let monitors = Monitor::all().context("không đọc được danh sách màn hình")?;
    if monitors.is_empty() {
        return Err(anyhow!("không tìm thấy màn hình"));
    }

    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .unwrap_or(&monitors[0]);

    let image = monitor.capture_image().context("không chụp được màn hình")?;
    image.save(output).context("không lưu được screenshot")?;
    Ok(())
}
