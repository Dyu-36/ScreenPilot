use std::{
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

use anyhow::Result;
use reqwest::blocking::Client;

use crate::{
    media,
    state::{self, Config},
    zalo,
};

const AGENT_POLL_TIMEOUT: Duration = Duration::from_secs(25);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Trạng thái chia sẻ giữa UI và poller thread.
#[derive(Clone)]
pub struct Shared {
    pub config: Arc<Mutex<Config>>,
    generation: Arc<(Mutex<u64>, Condvar)>,
    /// Token đang được UI pair (poller tạm dừng polling trên token đó
    /// để tránh hai getUpdates song song trên cùng một bot).
    pub pairing_token: Arc<Mutex<Option<String>>>,
}

impl Shared {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            generation: Arc::new((Mutex::new(0), Condvar::new())),
            pairing_token: Arc::new(Mutex::new(None)),
        }
    }

    pub fn generation(&self) -> u64 {
        *self.generation.0.lock().unwrap()
    }

    pub fn bump(&self) {
        let (lock, cond) = &*self.generation;
        let mut generation = lock.lock().unwrap();
        *generation += 1;
        cond.notify_all();
    }

    /// Đọc - sửa - lưu config an toàn giữa UI thread và poller thread.
    pub fn with_config<T>(&self, mutate: impl FnOnce(&mut Config) -> T) -> T {
        let mut config = self.config.lock().unwrap();
        let result = mutate(&mut config);
        if let Err(err) = state::save_config(&config) {
            state::append_log(&format!("cannot save config: {err:#}"));
        }
        result
    }

    fn wait_for_change(&self, seen: u64) {
        let (lock, cond) = &*self.generation;
        let mut generation = lock.lock().unwrap();
        while *generation == seen {
            let (next, _) = cond
                .wait_timeout(generation, Duration::from_millis(500))
                .unwrap();
            generation = next;
        }
    }
}

pub fn spawn(shared: Shared) {
    thread::spawn(move || poller_loop(shared));
}

fn poller_loop(shared: Shared) {
    state::append_log("agent loop started");
    let mut seen = shared.generation();

    loop {
        let Some((token, offset, devices)) = snapshot(&shared) else {
            shared.wait_for_change(seen);
            seen = shared.generation();
            continue;
        };
        seen = shared.generation();
        run_active(&shared, &token, offset, devices, &mut seen);
    }
}

fn snapshot(shared: &Shared) -> Option<(String, Option<i64>, Vec<String>)> {
    let config = shared.config.lock().unwrap();
    let bot = config.active_bot()?;
    Some((
        bot.token.clone(),
        bot.update_offset,
        bot.devices
            .iter()
            .map(|device| device.chat_id.clone())
            .collect(),
    ))
}

fn run_active(
    shared: &Shared,
    token: &str,
    mut offset: Option<i64>,
    devices: Vec<String>,
    seen: &mut u64,
) {
    let Ok(client) = zalo::build_client() else {
        thread::sleep(Duration::from_secs(5));
        return;
    };
    zalo::delete_webhook(&client, token);
    state::append_log("agent polling for active bot");

    let mut backoff = Duration::from_secs(1);

    loop {
        // UI đang pair trên đúng token này: nhường đường, tránh 409 getUpdates.
        while shared.pairing_token.lock().unwrap().as_deref() == Some(token) {
            thread::sleep(Duration::from_millis(400));
            if shared.generation() != *seen {
                *seen = shared.generation();
            }
        }

        if shared.generation() != *seen {
            return;
        }

        match zalo::get_updates(&client, token, offset, AGENT_POLL_TIMEOUT) {
            Ok(updates) => {
                backoff = Duration::from_secs(1);
                for update in updates {
                    if shared.generation() != *seen {
                        return;
                    }

                    if let Some(id) = update.update_id {
                        let next = id.saturating_add(1);
                        if offset.map_or(true, |current| next > current) {
                            offset = Some(next);
                            let token = token.to_string();
                            shared.with_config(|config| {
                                if let Some(bot) = config.active_bot_mut() {
                                    if bot.token == token {
                                        bot.update_offset = Some(next);
                                    }
                                }
                            });
                        }
                    }

                    if devices.iter().any(|chat| chat == &update.chat_id) {
                        dispatch(shared, &client, token, &update);
                    } else if update.text.trim_start().starts_with('/') {
                        state::append_log(&format!(
                            "ignored command from unpaired chat {}",
                            update.chat_id
                        ));
                    }
                }
            }
            Err(err) => {
                state::append_log(&format!("agent polling error: {err}"));
                thread::sleep(backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }

        if shared.generation() != *seen {
            return;
        }
    }
}

fn dispatch(shared: &Shared, client: &Client, token: &str, update: &zalo::ZaloUpdate) {
    let command = update
        .text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let chat_id = update.chat_id.as_str();

    match command.as_str() {
        "/screenshot" | "/screen" | "/cap" => {
            handle_screenshot(client, token, chat_id);
        }
        "/status" | "/ping" => {
            let device_count = shared
                .config
                .lock()
                .unwrap()
                .active_bot()
                .map(|bot| bot.devices.len())
                .unwrap_or(0);
            let machine = std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "PC".to_string());
            let message = format!(
                "✅ Tack Agent đang online\nMáy: {machine}\nPhiên bản: {}\nThiết bị đã pair: {device_count}\nZalo: bot đang hoạt động",
                env!("CARGO_PKG_VERSION")
            );
            let _ = zalo::send_message(client, token, chat_id, &message);
        }
        "/help" | "/start" => {
            let _ = zalo::send_message(client, token, chat_id, zalo_help_text());
        }
        "/pair" => {
            let _ = zalo::send_message(
                client,
                token,
                chat_id,
                "✅ Zalo này đã pair với Tack Agent rồi. Dùng /help để xem lệnh.",
            );
        }
        "/unpair" => {
            let removed = shared.with_config(|config| config.remove_device(chat_id));
            let reply = if removed {
                "🔓 Đã gỡ thiết bị này khỏi Tack Agent."
            } else {
                "ℹ️ Thiết bị này không có trong danh sách pair của bot đang chạy."
            };
            let _ = zalo::send_message(client, token, chat_id, reply);
        }
        "" => {}
        _ => {
            let _ = zalo::send_message(
                client,
                token,
                chat_id,
                "ℹ️ Tack Agent không hỗ trợ lệnh này. Dùng /help để xem danh sách lệnh.",
            );
        }
    }
}

fn handle_screenshot(client: &Client, token: &str, chat_id: &str) {
    zalo::send_chat_action(client, token, chat_id, "typing");

    let path = match state::temp_screenshot_path() {
        Ok(path) => path,
        Err(err) => {
            state::append_log(&format!("screenshot temp path error: {err}"));
            let _ = zalo::send_message(client, token, chat_id, "⚠️ Không tạo được file screenshot tạm.");
            return;
        }
    };

    let result = (|| -> Result<()> {
        // Không capture trong process agent thường trú. Worker riêng chỉ được spawn
        // khi có lệnh, chụp xong thoát để OS thu hồi sạch framebuffer/xcap allocations.
        crate::capture_screen_isolated(&path)?;
        let url = media::upload_image(client, &path)?;
        let machine = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "PC".to_string());
        zalo::send_photo_url(
            client,
            token,
            chat_id,
            &url,
            &format!("📸 Screenshot từ {machine}"),
        )?;
        Ok(())
    })();

    let _ = std::fs::remove_file(&path);

    match result {
        Ok(()) => state::append_log("screenshot captured by worker and sent"),
        Err(err) => {
            state::append_log(&format!("screenshot command failed: {err:#}"));
            let _ = zalo::send_message(
                client,
                token,
                chat_id,
                "⚠️ Không gửi được screenshot. Xem log của Tack Agent trên máy để biết chi tiết.",
            );
        }
    }
}

fn zalo_help_text() -> &'static str {
    "🤖 Tack Agent\n\n/screenshot — chụp màn hình chính và gửi ảnh\n/status — kiểm tra agent đang online\n/help — xem danh sách lệnh\n/unpair — gỡ thiết bị này khỏi agent\n\nMọi Zalo đã pair đều điều khiển được agent."
}
