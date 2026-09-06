use std::{
    env,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub chat_id: String,
    pub paired_at_unix: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BotEntry {
    pub token: String,
    pub label: String,
    pub added_at_unix: u64,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub update_offset: Option<i64>,
    #[serde(default)]
    pub devices: Vec<Device>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub bots: Vec<BotEntry>,
}

// Định dạng Tack Agent 1.0, chỉ dùng để migrate sang schema mới.
#[derive(Serialize, Deserialize)]
struct LegacyConfig {
    #[serde(default)]
    zalo_token: String,
}

#[derive(Serialize, Deserialize)]
struct LegacyPairState {
    chat_id: String,
    #[serde(default)]
    paired_at_unix: u64,
    #[serde(default)]
    update_offset: Option<i64>,
}

pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("USERPROFILE")
                    .map(|home| PathBuf::from(home).join("AppData").join("Roaming"))
            })
            .unwrap_or_else(env::temp_dir)
    }
    #[cfg(not(target_os = "windows"))]
    {
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
            .unwrap_or_else(env::temp_dir)
    }
}

pub fn config_dir() -> PathBuf {
    data_dir().join("tack-agent")
}

fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

fn legacy_pair_path() -> PathBuf {
    config_dir().join("paired.json")
}

fn log_path() -> PathBuf {
    config_dir().join("agent.log")
}

pub fn label_from_token(token: &str) -> String {
    let tail: String = token
        .trim()
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("Bot …{tail}")
}

pub fn new_bot(token: &str, active: bool) -> BotEntry {
    BotEntry {
        token: token.trim().to_string(),
        label: label_from_token(token),
        added_at_unix: unix_seconds(),
        active,
        update_offset: None,
        devices: Vec::new(),
    }
}

impl Config {
    pub fn active_bot(&self) -> Option<&BotEntry> {
        self.bots.iter().find(|bot| bot.active)
    }

    pub fn active_bot_mut(&mut self) -> Option<&mut BotEntry> {
        self.bots.iter_mut().find(|bot| bot.active)
    }

    pub fn bot_by_token_mut(&mut self, token: &str) -> Option<&mut BotEntry> {
        let token = token.trim();
        self.bots.iter_mut().find(|bot| bot.token == token)
    }

    pub fn set_active_by_token(&mut self, token: &str) {
        let target = token.trim();
        for bot in &mut self.bots {
            bot.active = bot.token == target;
        }
    }

    pub fn remove_bot(&mut self, token: &str) -> bool {
        let before = self.bots.len();
        self.bots.retain(|bot| bot.token != token.trim());
        let removed = self.bots.len() < before;
        if removed && !self.bots.is_empty() && self.active_bot().is_none() {
            self.bots[0].active = true;
        }
        removed
    }

    pub fn remove_device(&mut self, chat_id: &str) -> bool {
        let chat_id = chat_id.trim();
        let Some(bot) = self.active_bot_mut() else {
            return false;
        };
        let before = bot.devices.len();
        bot.devices.retain(|device| device.chat_id != chat_id);
        bot.devices.len() < before
    }
}

fn normalize(mut config: Config) -> Config {
    for bot in &mut config.bots {
        bot.token = bot.token.trim().to_string();
    }
    if config.active_bot().is_none() {
        if let Some(first) = config.bots.first_mut() {
            first.active = true;
        }
    }
    config
}

pub fn load_config() -> Result<Config> {
    let path = config_path();
    if let Ok(data) = fs::read(&path) {
        if let Ok(config) = serde_json::from_slice::<Config>(&data) {
            if !config.bots.is_empty() {
                return Ok(normalize(config));
            }
        }

        if let Some(migrated) = migrate_legacy(&data) {
            if !migrated.bots.is_empty() {
                save_config(&migrated)?;
                let _ = fs::remove_file(legacy_pair_path());
                append_log("migrated Tack Agent 1.0 config to 2.0 schema");
                return Ok(migrated);
            }
        }

        if serde_json::from_slice::<Config>(&data).is_ok() {
            return Ok(Config::default());
        }

        bail!(
            "{} không đọc được (JSON lỗi). Xóa file này rồi chạy lại Tack Agent để cấu hình từ đầu.",
            path.display()
        );
    }

    if let Some(token) = external_file_token() {
        let mut config = Config::default();
        config.bots.push(new_bot(&token, true));
        save_config(&config)?;
        append_log("imported bot token from gotack config");
        return Ok(config);
    }

    if let Some(token) = env_token() {
        let mut config = Config::default();
        config.bots.push(new_bot(&token, true));
        append_log("using bot token from TACK_ZALO_BOT_TOKEN");
        return Ok(config);
    }

    Ok(Config::default())
}

pub fn save_config(config: &Config) -> Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir).context("không tạo được thư mục cấu hình Tack Agent")?;
    let data = serde_json::to_vec_pretty(config).context("không mã hóa được cấu hình")?;
    fs::write(config_path(), data).context("không lưu được cấu hình")?;
    Ok(())
}

fn migrate_legacy(data: &[u8]) -> Option<Config> {
    let legacy: LegacyConfig = serde_json::from_slice(data).ok()?;
    let token = legacy.zalo_token.trim().to_string();
    if token.is_empty() {
        return None;
    }

    let mut bot = new_bot(&token, true);
    if let Ok(pair_data) = fs::read(legacy_pair_path()) {
        if let Ok(pair) = serde_json::from_slice::<LegacyPairState>(&pair_data) {
            let chat_id = pair.chat_id.trim().to_string();
            if !chat_id.is_empty() {
                bot.devices.push(Device {
                    chat_id,
                    paired_at_unix: pair.paired_at_unix,
                });
                bot.update_offset = pair.update_offset;
            }
        }
    }

    let mut config = Config::default();
    config.bots.push(bot);
    Some(config)
}

fn external_file_token() -> Option<String> {
    let gotack_state = data_dir().join("gotack").join("zalo.json");
    let data = fs::read(gotack_state).ok()?;
    let value: Value = serde_json::from_slice(&data).ok()?;
    value
        .get("token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn env_token() -> Option<String> {
    if let Ok(token) = env::var("TACK_ZALO_BOT_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }

    option_env!("TACK_ZALO_BOT_TOKEN")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
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

    let clean = message.replace('\r', " ").replace('\n', " ");
    let line = format!("[{}] {}\r\n", unix_seconds(), clean);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_active_is_exclusive() {
        let mut config = Config::default();
        config.bots.push(new_bot("token-a", true));
        config.bots.push(new_bot("token-b", false));

        config.set_active_by_token("token-b");

        assert!(config.bots.iter().find(|b| b.token == "token-b").unwrap().active);
        assert!(!config.bots.iter().find(|b| b.token == "token-a").unwrap().active);
    }

    #[test]
    fn removing_active_bot_promotes_first_remaining() {
        let mut config = Config::default();
        config.bots.push(new_bot("token-a", true));
        config.bots.push(new_bot("token-b", false));
        config.bots.push(new_bot("token-c", false));

        assert!(config.remove_bot("token-a"));
        assert_eq!(config.active_bot().unwrap().token, "token-b");
    }

    #[test]
    fn device_add_is_idempotent_per_bot() {
        let mut config = Config::default();
        config.bots.push(new_bot("token-a", true));

        assert!(config.bot_by_token_mut("token-a").is_some());
        assert!(!config.remove_device("chat-1"));

        config
            .bot_by_token_mut("token-a")
            .unwrap()
            .devices
            .push(Device { chat_id: "chat-1".into(), paired_at_unix: 0 });
        // remove_device chỉ tác động lên bot active
        assert!(config.remove_device("chat-1"));
        assert!(!config.remove_device("chat-1"));
    }

    #[test]
    fn migrates_legacy_single_bot_config() {
        let legacy = serde_json::to_vec(&LegacyConfig { zalo_token: "abc123".into() }).unwrap();
        let migrated = migrate_legacy(&legacy).expect("migration should produce config");
        assert_eq!(migrated.bots.len(), 1);
        assert_eq!(migrated.bots[0].token, "abc123");
        assert!(migrated.bots[0].active);
    }

    #[test]
    fn migrates_legacy_pair_state_into_devices() {
        let legacy = serde_json::to_vec(&LegacyConfig { zalo_token: "abc123".into() }).unwrap();
        let pair = serde_json::to_vec(&LegacyPairState {
            chat_id: "chat-9".into(),
            paired_at_unix: 42,
            update_offset: Some(7),
        })
        .unwrap();
        let dir = config_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(legacy_pair_path(), &pair).unwrap();

        let migrated = migrate_legacy(&legacy).unwrap();
        let _ = fs::remove_file(legacy_pair_path());

        assert_eq!(migrated.bots[0].devices.len(), 1);
        assert_eq!(migrated.bots[0].devices[0].chat_id, "chat-9");
        assert_eq!(migrated.bots[0].update_offset, Some(7));
    }

    #[test]
    fn label_masks_token_tail() {
        assert_eq!(label_from_token("1234567890"), "Bot …7890");
        assert_eq!(label_from_token("ab"), "Bot …ab");
    }
}
