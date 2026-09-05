use std::{
    env,
    fs,
    io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentState {
    pub chat_id: String,
    pub paired_at_unix: u64,
    #[serde(default)]
    pub update_offset: Option<i64>,
}

#[derive(Default, Serialize, Deserialize)]
struct LocalConfig {
    #[serde(default)]
    zalo_token: String,
}

pub fn appdata_dir() -> PathBuf {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .map(|home| PathBuf::from(home).join("AppData").join("Roaming"))
        })
        .unwrap_or_else(env::temp_dir)
}

pub fn config_dir() -> PathBuf {
    appdata_dir().join("tack-agent")
}

fn pair_state_path() -> PathBuf {
    config_dir().join("paired.json")
}

fn local_config_path() -> PathBuf {
    config_dir().join("config.json")
}

fn log_path() -> PathBuf {
    config_dir().join("agent.log")
}

pub fn load_pair_state() -> Result<Option<AgentState>> {
    let path = pair_state_path();
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context("không đọc được trạng thái Tack Agent"),
    };

    let mut state: AgentState =
        serde_json::from_slice(&data).context("trạng thái pair của Tack Agent bị lỗi")?;
    state.chat_id = state.chat_id.trim().to_string();
    if state.chat_id.is_empty() {
        return Ok(None);
    }
    Ok(Some(state))
}

pub fn save_pair_state(state: &AgentState) -> Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir).context("không tạo được thư mục cấu hình Tack Agent")?;
    let data = serde_json::to_vec_pretty(state).context("không mã hóa được trạng thái pair")?;
    fs::write(pair_state_path(), data).context("không lưu được trạng thái pair")?;
    Ok(())
}

pub fn remove_pair_state() -> Result<()> {
    match fs::remove_file(pair_state_path()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).context("không xóa được trạng thái pair"),
    }
}

pub fn load_zalo_token() -> Result<String> {
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

    if let Ok(data) = fs::read(local_config_path()) {
        if let Ok(config) = serde_json::from_slice::<LocalConfig>(&data) {
            let token = config.zalo_token.trim().to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }
    }

    let gotack_state = appdata_dir().join("gotack").join("zalo.json");
    if let Ok(data) = fs::read(gotack_state) {
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
        "Không tìm thấy Bot Token Zalo. Hãy cấu hình Zalo trong Gotack, đặt TACK_ZALO_BOT_TOKEN, hoặc chạy tack-agent.exe --set-token <token>."
    )
}

pub fn save_local_token(token: &str) -> Result<()> {
    let token = token.trim();
    if token.is_empty() {
        bail!("Bot Token Zalo không được để trống");
    }
    let dir = config_dir();
    fs::create_dir_all(&dir).context("không tạo được thư mục cấu hình Tack Agent")?;
    let data = serde_json::to_vec_pretty(&LocalConfig {
        zalo_token: token.to_string(),
    })?;
    fs::write(local_config_path(), data).context("không lưu được Bot Token Zalo")?;
    Ok(())
}

pub fn clear_local_token() -> Result<()> {
    match fs::remove_file(local_config_path()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).context("không xóa được Bot Token cục bộ"),
    }
}

pub fn append_log(message: &str) {
    let dir = config_dir();
    let _ = fs::create_dir_all(&dir);

    let path = log_path();
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > 2 * 1024 * 1024 {
            let _ = fs::remove_file(&path);
        }
    }

    let line = format!("[{}] {}\r\n", unix_seconds(), message.replace(['\r', '\n'], " "));
    use std::io::Write;
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn temp_screenshot_path() -> Result<PathBuf> {
    let dir = env::temp_dir().join("tack-agent");
    fs::create_dir_all(&dir).context("không tạo được thư mục tạm cho screenshot")?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    Ok(dir.join(format!("screenshot-{millis}.png")))
}
