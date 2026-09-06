use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use eframe::egui;
use rand::Rng;

use crate::{
    agent::Shared,
    platform,
    state::{self, Device},
    zalo,
};

const PAIR_TTL: Duration = Duration::from_secs(60);
const PAIR_POLL_SLICE: Duration = Duration::from_secs(5);

pub fn generate_pair_code() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000u32))
}

fn pair_command_matches(text: &str, expected: &str) -> bool {
    let mut fields = text.split_whitespace();
    let command = fields.next().unwrap_or_default();
    let code = fields.next().unwrap_or_default();
    command.eq_ignore_ascii_case("/pair") && code == expected
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    TokenInput { adding: bool },
    Pairing,
    Manage,
}

pub struct PairSession {
    code: String,
    deadline: Instant,
    result: Arc<Mutex<Option<Result<String, String>>>>,
    stop: Arc<AtomicBool>,
}

pub struct TackApp {
    shared: Shared,
    ctx: egui::Context,
    view: View,
    token_input: String,
    pair: Option<PairSession>,
    status_message: Option<String>,
}

impl TackApp {
    pub fn new(shared: Shared, ctx: egui::Context) -> Self {
        install_vietnamese_font(&ctx);
        Self {
            shared,
            ctx,
            view: View::Manage,
            token_input: String::new(),
            pair: None,
            status_message: None,
        }
    }

    fn bot_count(&self) -> usize {
        self.shared.config.lock().unwrap().bots.len()
    }

    fn current_view(&self) -> View {
        if self.pair.is_some() {
            View::Pairing
        } else if self.bot_count() == 0 {
            View::TokenInput { adding: false }
        } else {
            self.view
        }
    }

    fn start_pair_session(&mut self, token: &str) {
        let token = token.trim().to_string();
        *self.shared.pairing_token.lock().unwrap() = Some(token.clone());
        self.pair = Some(spawn_pair_session(
            self.shared.clone(),
            token,
            self.ctx.clone(),
        ));
    }

    fn close_to_background(&mut self, ctx: &egui::Context) {
        if let Some(pair) = self.pair.take() {
            pair.stop.store(true, Ordering::SeqCst);
        }
        *self.shared.pairing_token.lock().unwrap() = None;

        if let Err(err) = crate::spawn_background_after_exit() {
            state::append_log(&format!("cannot switch GUI to background: {err:#}"));
            self.status_message = Some(format!("Không thể chuyển sang chạy nền: {err:#}"));
            return;
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for TackApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|input| input.viewport().close_requested()) {
            if let Err(err) = crate::spawn_background_after_exit() {
                state::append_log(&format!("cannot restore background on window close: {err:#}"));
            }
        }

        if crate::SHOW_REQUESTED.swap(false, Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        if let Some(pair) = &self.pair {
            let outcome = pair.result.lock().unwrap().clone();
            if let Some(outcome) = outcome {
                self.pair = None;
                self.view = View::Manage;
                match outcome {
                    Ok(chat_id) => {
                        self.status_message =
                            Some(format!("Đã ghép cặp với {chat_id}. Bot đang hoạt động."));
                    }
                    Err(err) => {
                        self.status_message = Some(format!("Lỗi: {err}"));
                    }
                }
            }
        }

        let view = self.current_view();

        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                    "Đóng/Ẩn giao diện sẽ trả về Manager chạy nền để giảm RAM. Log: {}",
                    state::config_dir().join("agent.log").display()
                ))
                .small()
                .weak(),
            );
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| match view {
            View::TokenInput { adding } => self.render_token_input(ui, ctx, adding),
            View::Pairing => self.render_pairing(ui, ctx),
            View::Manage => self.render_manage(ui, ctx),
        });
    }
}

impl TackApp {
    fn render_token_input(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, adding: bool) {
        ui.vertical_centered(|ui| {
            ui.heading("Manager");
        });
        ui.add_space(12.0);
        ui.label(if adding {
            "Nhập Bot Token Zalo cho bot mới:"
        } else {
            "Nhập Bot Token Zalo để bắt đầu:"
        });

        let response = ui.add(
            egui::TextEdit::singleline(&mut self.token_input)
                .password(true)
                .desired_width(f32::INFINITY),
        );

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let save = ui.button("Lưu & ghép cặp");
            if adding && ui.button("Hủy").clicked() {
                self.token_input.clear();
                self.view = View::Manage;
            }
            let enter = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if save.clicked() || enter {
                let token = self.token_input.trim().to_string();
                if token.is_empty() {
                    self.status_message = Some("Token không được để trống.".to_string());
                } else {
                    self.token_input.clear();
                    self.status_message = None;
                    self.start_pair_session(&token);
                }
            }
        });

        ui.add_space(8.0);
        if let Some(message) = &self.status_message {
            ui.colored_label(egui::Color32::LIGHT_BLUE, message);
        }
    }

    fn render_pairing(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(pair) = self.pair.as_ref() else {
            self.view = View::Manage;
            return;
        };

        ui.vertical_centered(|ui| {
            ui.heading("Ghép cặp Zalo");
        });
        ui.add_space(12.0);
        ui.label("Trên Zalo, nhắn cho bot:");
        ui.add_space(8.0);

        let command = format!("/pair {}", pair.code);
        ui.add(
            egui::Label::new(
                egui::RichText::new(&command)
                    .monospace()
                    .size(26.0)
                    .strong(),
            )
            .selectable(true),
        );
        ui.add_space(4.0);
        if ui.button("Sao chép lệnh").clicked() {
            ctx.copy_text(command.clone());
        }

        ui.add_space(8.0);
        let remaining = pair
            .deadline
            .saturating_duration_since(Instant::now())
            .as_secs();
        ui.label(format!("Mã hết hiệu lực sau {remaining} giây."));
        ctx.request_repaint_after(Duration::from_secs(1));

        ui.add_space(16.0);
        if ui.button("Hủy").clicked() {
            if let Some(pair) = self.pair.take() {
                pair.stop.store(true, Ordering::SeqCst);
            }
            *self.shared.pairing_token.lock().unwrap() = None;
            self.view = View::Manage;
        }
    }

    fn render_manage(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.heading("Manager");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Ẩn").clicked() {
                    self.close_to_background(ctx);
                }
            });
        });
        ui.label(
            egui::RichText::new(
                "GUI chỉ chạy khi cần quản lý. Nhấn Ẩn để thoát GUI và tiếp tục bằng Manager chạy nền nhẹ RAM.",
            )
            .small(),
        );
        ui.add_space(4.0);

        if let Some(message) = &self.status_message {
            ui.label(message);
            ui.add_space(4.0);
        }
        ui.separator();

        let config = self.shared.config.lock().unwrap().clone();

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(egui::RichText::new("Bot").strong());
            for bot in &config.bots {
                ui.horizontal(|ui| {
                    if bot.active {
                        ui.label(
                            egui::RichText::new("ĐANG CHẠY")
                                .small()
                                .color(egui::Color32::from_rgb(80, 200, 120)),
                        );
                    }
                    ui.label(&bot.label);
                    ui.label(
                        egui::RichText::new(format!("({} thiết bị)", bot.devices.len()))
                            .small()
                            .weak(),
                    );
                });
                ui.horizontal(|ui| {
                    if !bot.active && ui.button("Kích hoạt").clicked() {
                        let token = bot.token.clone();
                        self.shared.with_config(|config| config.set_active_by_token(&token));
                        self.shared.bump();
                        self.status_message = Some("Đã chuyển sang bot này.".to_string());
                    }
                    if ui.button("Xóa").clicked() {
                        let token = bot.token.clone();
                        let removed = self.shared.with_config(|config| config.remove_bot(&token));
                        self.shared.bump();
                        if removed {
                            if self.bot_count() == 0 {
                                let _ = platform::remove_autostart();
                            }
                            self.status_message = Some("Đã xóa bot.".to_string());
                        }
                    }
                });
                ui.add_space(6.0);
            }

            if ui.button("Thêm bot…").clicked() {
                self.view = View::TokenInput { adding: true };
            }
            ui.separator();

            ui.label(egui::RichText::new("Thiết bị đã ghép cặp (bot đang chạy)").strong());
            match config.active_bot() {
                None => {
                    ui.label("Chưa có bot nào đang chạy.");
                }
                Some(bot) => {
                    if bot.devices.is_empty() {
                        ui.label("Chưa có thiết bị nào. Nhấn \"Tạo mã /pair\" rồi nhắn lệnh từ Zalo.");
                    }
                    for device in &bot.devices {
                        ui.horizontal(|ui| {
                            ui.label(device.chat_id.clone());
                            if ui.small_button("Xóa").clicked() {
                                let chat_id = device.chat_id.clone();
                                self.shared.with_config(|config| {
                                    config.remove_device(&chat_id);
                                });
                                self.shared.bump();
                            }
                        });
                    }
                    ui.add_space(4.0);
                    if ui.button("Tạo mã /pair").clicked() {
                        let token = bot.token.clone();
                        self.start_pair_session(&token);
                    }
                }
            }
        });
    }
}

fn install_vietnamese_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    ];

    for path in CANDIDATES {
        let Ok(data) = std::fs::read(path) else {
            continue;
        };

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "system_vn".to_owned(),
            egui::FontData::from_owned(data),
        );
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "system_vn".to_owned());
        }
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            family.push("system_vn".to_owned());
        }
        ctx.set_fonts(fonts);
        return;
    }
}

fn spawn_pair_session(shared: Shared, token: String, ctx: egui::Context) -> PairSession {
    let code = generate_pair_code();
    let result = Arc::new(Mutex::new(None));
    let stop = Arc::new(AtomicBool::new(false));
    let deadline = Instant::now() + PAIR_TTL;

    {
        let shared = shared.clone();
        let token = token.clone();
        let code = code.clone();
        let result = result.clone();
        let stop = stop.clone();

        std::thread::spawn(move || {
            let finish = |outcome: Result<String, String>| {
                *shared.pairing_token.lock().unwrap() = None;
                *result.lock().unwrap() = Some(outcome);
                shared.bump();
                ctx.request_repaint();
            };

            let client = match zalo::build_client() {
                Ok(client) => client,
                Err(err) => return finish(Err(format!("{err:#}"))),
            };
            zalo::delete_webhook(&client, &token);
            state::append_log("pairing started");

            let mut offset: Option<i64> = None;
            while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let poll = remaining.min(PAIR_POLL_SLICE);
                if poll.is_zero() {
                    break;
                }

                match zalo::get_updates(&client, &token, offset, poll) {
                    Ok(updates) => {
                        for update in updates {
                            if let Some(id) = update.update_id {
                                let next = id.saturating_add(1);
                                if offset.map_or(true, |current| next > current) {
                                    offset = Some(next);
                                }
                            }

                            if Instant::now() > deadline {
                                break;
                            }

                            if !pair_command_matches(&update.text, &code) {
                                continue;
                            }

                            let chat_id = update.chat_id.clone();
                            shared.with_config(|config| {
                                if let Some(bot) = config.bot_by_token_mut(&token) {
                                    if !bot.devices.iter().any(|device| device.chat_id == chat_id) {
                                        bot.devices.push(Device {
                                            chat_id: chat_id.clone(),
                                            paired_at_unix: state::unix_seconds(),
                                        });
                                    }
                                    bot.update_offset = offset;
                                    bot.active = true;
                                } else {
                                    let mut bot = state::new_bot(&token, true);
                                    bot.update_offset = offset;
                                    bot.devices.push(Device {
                                        chat_id: chat_id.clone(),
                                        paired_at_unix: state::unix_seconds(),
                                    });
                                    config.bots.push(bot);
                                }
                                for bot in config.bots.iter_mut() {
                                    if bot.token != token {
                                        bot.active = false;
                                    }
                                }
                            });

                            let _ = platform::install_autostart();
                            let _ = zalo::send_message(
                                &client,
                                &token,
                                &chat_id,
                                "✅ Manager đã ghép cặp thành công. Từ giờ dùng /screenshot, /status, /help hoặc /unpair.",
                            );
                            state::append_log("pairing succeeded; autostart installed");
                            return finish(Ok(chat_id));
                        }
                    }
                    Err(err) => {
                        state::append_log(&format!("pair polling error: {err}"));
                        std::thread::sleep(Duration::from_millis(350));
                    }
                }
            }

            state::append_log("pairing ended without success");
            finish(Err(
                "Hết 60 giây chờ mã /pair. Hãy thử lại.".to_string(),
            ));
        });
    }

    PairSession {
        code,
        deadline,
        result,
        stop,
    }
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
