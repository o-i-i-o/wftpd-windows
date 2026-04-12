use crate::core::config::Config;
use crate::core::config_manager::ConfigManager;
use crate::core::i18n;
use crate::core::ipc::IpcClient;
use crate::gui_egui::styles;
use egui::RichText;
use std::sync::mpsc;
use std::time::Instant;

#[derive(Debug, Clone)]
struct ValidationError {
    field: String,
    message: String,
}

fn validate_ip_cidr(input: &str) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    for (line_num, line) in input.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if !is_valid_ip_or_cidr(trimmed) {
            errors.push(ValidationError {
                field: i18n::t_fmt("security.line_n", &[&(line_num + 1).to_string()]),
                message: i18n::t_fmt("security.invalid_ip_cidr", &[&trimmed.to_string()]),
            });
        }
    }

    errors
}

fn is_valid_ip_or_cidr(input: &str) -> bool {
    if input.contains('/') {
        let parts: Vec<&str> = input.split('/').collect();
        if parts.len() != 2 {
            return false;
        }
        let ip = parts[0];
        let prefix = match parts[1].parse::<u8>() {
            Ok(p) => p,
            Err(_) => return false,
        };

        if ip.contains(':') {
            if prefix > 128 {
                return false;
            }
            is_valid_ipv6(ip)
        } else {
            if prefix > 32 {
                return false;
            }
            is_valid_ipv4(ip)
        }
    } else {
        if input.contains(':') {
            is_valid_ipv6(input)
        } else {
            is_valid_ipv4(input)
        }
    }
}

fn is_valid_ipv4(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| p.parse::<u8>().is_ok())
}

fn is_valid_ipv6(ip: &str) -> bool {
    if ip.is_empty() || ip.len() > 45 {
        return false;
    }

    // 检查是否包含非法字符（只允许 0-9, a-f, A-F, :）
    if !ip.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
        return false;
    }

    let parts: Vec<&str> = ip.split("::").collect();
    if parts.len() > 2 {
        return false;
    }

    let total_groups: usize = if parts.len() == 2 {
        let left = if parts[0].is_empty() {
            0
        } else {
            parts[0].split(':').count()
        };
        let right = if parts[1].is_empty() {
            0
        } else {
            parts[1].split(':').count()
        };
        left + right
    } else {
        ip.split(':').count()
    };

    if total_groups > 8 {
        return false;
    }

    // 验证每个 hextet（16 位值，0-FFFF）
    for part in ip.split(':') {
        if !part.is_empty() && !part.starts_with("::") && !part.ends_with("::") {
            // 跳过空的部分（由 :: 产生）
            if part.is_empty() {
                continue;
            }
            // 检查长度（最多 4 个十六进制字符）
            if part.len() > 4 {
                return false;
            }
            // 尝试解析为 u16
            if u16::from_str_radix(part, 16).is_err() {
                return false;
            }
        }
    }

    true
}

enum SaveResult {
    Success(String),
    Error(String),
}

pub struct SecurityTab {
    config_manager: ConfigManager,
    // Fail2Ban 配置
    fail2ban_enabled: bool,
    fail2ban_threshold_buf: String,
    fail2ban_ban_time_buf: String,
    // 连接限制
    max_connections_buf: String,
    max_connections_per_ip_buf: String,
    // IP 访问控制
    allowed_ips_text: String,
    denied_ips_text: String,
    // 符号链接安全
    allow_symlinks: bool,
    // 状态和错误
    status_message: Option<(String, bool)>,
    validation_errors: Vec<ValidationError>,
    fail2ban_threshold_error: Option<String>,
    fail2ban_ban_time_error: Option<String>,
    max_connections_error: Option<String>,
    max_connections_per_ip_error: Option<String>,
    save_sender: Option<mpsc::Sender<SaveResult>>,
    save_receiver: Option<mpsc::Receiver<SaveResult>>,
    is_saving: bool,
    last_save_time: Option<Instant>,
}

impl SecurityTab {
    pub fn new(config_manager: ConfigManager) -> Self {
        let cfg = config_manager.read();
        // Fail2Ban 配置
        let fail2ban_enabled = cfg.security.fail2ban_enabled;
        let fail2ban_threshold_buf = cfg.security.fail2ban_threshold.to_string();
        let fail2ban_ban_time_buf = cfg.security.fail2ban_ban_time.to_string();
        // 连接限制
        let max_connections_buf = cfg.security.max_connections.to_string();
        let max_connections_per_ip_buf = cfg.security.max_connections_per_ip.to_string();
        // IP 访问控制
        let allowed_ips_text = cfg.security.allowed_ips.join("\n");
        let denied_ips_text = cfg.security.denied_ips.join("\n");
        // 符号链接安全
        let allow_symlinks = cfg.security.allow_symlinks;
        drop(cfg);

        let (tx, rx) = mpsc::channel();

        Self {
            config_manager,
            fail2ban_enabled,
            fail2ban_threshold_buf,
            fail2ban_ban_time_buf,
            max_connections_buf,
            max_connections_per_ip_buf,
            allowed_ips_text,
            denied_ips_text,
            allow_symlinks,
            status_message: None,
            validation_errors: Vec::new(),
            fail2ban_threshold_error: None,
            fail2ban_ban_time_error: None,
            max_connections_error: None,
            max_connections_per_ip_error: None,
            save_sender: Some(tx),
            save_receiver: Some(rx),
            is_saving: false,
            last_save_time: None,
        }
    }

    fn validate_all(&mut self) -> bool {
        self.validation_errors.clear();
        self.fail2ban_threshold_error = None;
        self.fail2ban_ban_time_error = None;
        self.max_connections_error = None;
        self.max_connections_per_ip_error = None;

        let mut valid = true;

        if let Ok(v) = self.fail2ban_threshold_buf.parse::<u32>() {
            if v == 0 {
                self.fail2ban_threshold_error = Some(i18n::t("security.must_greater_0"));
                valid = false;
            }
        } else {
            self.fail2ban_threshold_error = Some(i18n::t("security.enter_valid_number"));
            valid = false;
        }

        if let Ok(v) = self.fail2ban_ban_time_buf.parse::<u64>() {
            if v == 0 {
                self.fail2ban_ban_time_error = Some(i18n::t("security.must_greater_0"));
                valid = false;
            }
        } else {
            self.fail2ban_ban_time_error = Some(i18n::t("security.enter_valid_number"));
            valid = false;
        }

        if let Ok(v) = self.max_connections_buf.parse::<usize>() {
            if v == 0 {
                self.max_connections_error = Some(i18n::t("security.must_greater_0"));
                valid = false;
            }
        } else {
            self.max_connections_error = Some(i18n::t("security.enter_valid_number"));
            valid = false;
        }

        if let Ok(v) = self.max_connections_per_ip_buf.parse::<usize>() {
            if v == 0 {
                self.max_connections_per_ip_error = Some(i18n::t("security.must_greater_0"));
                valid = false;
            }
        } else {
            self.max_connections_per_ip_error = Some(i18n::t("security.enter_valid_number"));
            valid = false;
        }

        let allowed_errors = validate_ip_cidr(&self.allowed_ips_text);
        let denied_errors = validate_ip_cidr(&self.denied_ips_text);

        if !allowed_errors.is_empty() || !denied_errors.is_empty() {
            self.validation_errors = allowed_errors;
            self.validation_errors.extend(denied_errors);
            valid = false;
        }

        valid
    }

    fn apply_buffers_to_config(&mut self) {
        let mut cfg = self.config_manager.write();
        // Fail2Ban 配置
        cfg.security.fail2ban_enabled = self.fail2ban_enabled;
        if let Ok(v) = self.fail2ban_threshold_buf.parse::<u32>() {
            cfg.security.fail2ban_threshold = v;
        }
        if let Ok(v) = self.fail2ban_ban_time_buf.parse::<u64>() {
            cfg.security.fail2ban_ban_time = v;
        }
        // 连接限制
        if let Ok(v) = self.max_connections_buf.parse::<usize>() {
            cfg.security.max_connections = v;
        }
        if let Ok(v) = self.max_connections_per_ip_buf.parse::<usize>() {
            cfg.security.max_connections_per_ip = v;
        }

        // IP 访问控制
        cfg.security.allowed_ips = self
            .allowed_ips_text
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        cfg.security.denied_ips = self
            .denied_ips_text
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // 符号链接安全
        cfg.security.allow_symlinks = self.allow_symlinks;
    }

    fn save_async(&mut self, ctx: &egui::Context) {
        if self.is_saving {
            tracing::warn!("Save operation in progress");
            return;
        }

        if let Some(rx) = &self.save_receiver {
            match rx.try_recv() {
                Ok(_) => {
                    self.check_save_result();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.save_receiver = None;
                    self.save_sender = None;
                }
            }
        }

        if !self.validate_all() {
            self.status_message = Some((i18n::t("security.validation_failed"), false));
            tracing::warn!("Validation failed, aborting save");
            return;
        }

        self.apply_buffers_to_config();

        self.is_saving = true;
        self.status_message = Some((i18n::t("security.saving_config"), true));

        let config_manager = self.config_manager.clone();

        let (tx, rx) = mpsc::channel();
        self.save_sender = Some(tx.clone());
        self.save_receiver = Some(rx);

        let ctx_clone = ctx.clone();

        std::thread::spawn(move || {
            tracing::info!("Starting security config save...");
            let result = match config_manager.save(&Config::get_config_path()) {
                Ok(_) => {
                    tracing::info!("Config saved, checking backend service status...");
                    if IpcClient::is_server_running() {
                        tracing::info!("Backend service running, sending reload notification...");
                        match IpcClient::notify_reload() {
                            Ok(response) => {
                                if response.success {
                                    tracing::info!("Backend reload successful");
                                    SaveResult::Success(i18n::t("security.config_saved_reload"))
                                } else {
                                    tracing::warn!("Backend reload failed: {}", response.message);
                                    SaveResult::Success(i18n::t_fmt(
                                        "security.config_saved_reload_failed",
                                        &[&response.message],
                                    ))
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to notify backend: {}", e);
                                SaveResult::Success(i18n::t_fmt(
                                    "security.config_saved_notify_failed",
                                    &[&e.to_string()],
                                ))
                            }
                        }
                    } else {
                        tracing::warn!("Backend service not running");
                        SaveResult::Success(i18n::t("security.config_saved_not_running"))
                    }
                }
                Err(e) => {
                    tracing::error!("Save failed: {}", e);
                    SaveResult::Error(i18n::t_fmt(
                        "security.config_save_failed",
                        &[&e.to_string()],
                    ))
                }
            };

            if let Err(e) = tx.send(result) {
                tracing::error!("Failed to send save result: {}", e);
            }
            ctx_clone.request_repaint();
        });

        tracing::info!("Save thread started");
    }

    fn check_save_result(&mut self) {
        if let Some(rx) = &self.save_receiver
            && let Ok(result) = rx.try_recv()
        {
            self.is_saving = false;
            match result {
                SaveResult::Success(msg) => {
                    self.last_save_time = Some(Instant::now());
                    self.status_message = Some((msg, true));
                }
                SaveResult::Error(e) => {
                    self.status_message = Some((e, false));
                }
            }
        }
    }

    fn section_header(ui: &mut egui::Ui, icon: &str, title: &str) {
        styles::section_header(ui, icon, title);
    }

    fn format_last_save(&self) -> String {
        match self.last_save_time {
            Some(t) => {
                let elapsed = t.elapsed();
                if elapsed.as_secs() < 60 {
                    i18n::t_fmt(
                        "security.saved_n_seconds_ago",
                        &[&elapsed.as_secs().to_string()],
                    )
                } else if elapsed.as_secs() < 3600 {
                    i18n::t_fmt(
                        "security.saved_n_minutes_ago",
                        &[&(elapsed.as_secs() / 60).to_string()],
                    )
                } else {
                    i18n::t_fmt(
                        "security.saved_n_hours_ago",
                        &[&(elapsed.as_secs() / 3600).to_string()],
                    )
                }
            }
            None => i18n::t("security.not_saved"),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.check_save_result();

        ui.horizontal(|ui| {
            styles::page_header(ui, "🔒", &i18n::t("security.title"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save_text = i18n::t("security.save_config");
                let save_btn = if self.is_saving {
                    egui::Button::new(
                        RichText::new(i18n::t("security.saving"))
                            .color(egui::Color32::GRAY)
                            .size(styles::FONT_SIZE_MD),
                    )
                    .fill(styles::BG_SECONDARY)
                    .corner_radius(egui::CornerRadius::same(6))
                } else {
                    styles::primary_button(&save_text)
                };

                if ui.add(save_btn).clicked() && !self.is_saving {
                    self.save_async(ui.ctx());
                }

                ui.label(
                    RichText::new(self.format_last_save())
                        .size(styles::FONT_SIZE_SM)
                        .color(styles::TEXT_MUTED_COLOR),
                );

                if let Some((msg, success)) = &self.status_message {
                    styles::status_message(ui, msg, *success);
                }
            });
        });

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            Self::section_header(ui, "🔐", &i18n::t("security.login_security"));

            let available_width = ui.available_width();
            let label_width = (available_width * 0.2).clamp(100.0, 160.0);

            ui.label(
                RichText::new(i18n::t("security.fail2ban_protection"))
                    .size(styles::FONT_SIZE_MD)
                    .color(styles::TEXT_SECONDARY_COLOR)
                    .strong(),
            );
            ui.label(
                RichText::new(i18n::t("security.fail2ban_desc"))
                    .size(styles::FONT_SIZE_SM)
                    .color(styles::TEXT_MUTED_COLOR),
            );

            ui.add_space(styles::SPACING_XS);

            styles::form_row(
                ui,
                &i18n::t("security.enable_fail2ban"),
                label_width,
                |ui| {
                    ui.checkbox(&mut self.fail2ban_enabled, "")
                        .on_hover_text(i18n::t("security.enable_fail2ban_hint"));
                },
            );

            if self.fail2ban_enabled {
                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("security.fail_threshold"),
                    label_width,
                    |ui| {
                        let response = styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.fail2ban_threshold_buf)
                                    .desired_width(100.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            )
                        });

                        if response.response.lost_focus() {
                            if let Ok(v) = self.fail2ban_threshold_buf.parse::<u32>() {
                                if v == 0 {
                                    self.fail2ban_threshold_error =
                                        Some(i18n::t("security.must_greater_0"));
                                } else {
                                    self.fail2ban_threshold_error = None;
                                }
                            } else {
                                self.fail2ban_threshold_error =
                                    Some(i18n::t("security.enter_valid_number"));
                            }
                        }
                    },
                    &i18n::t("security.times"),
                );

                if let Some(err) = &self.fail2ban_threshold_error {
                    ui.horizontal(|ui| {
                        ui.add_sized([label_width, 24.0], egui::Label::new(""));
                        ui.label(
                            RichText::new(format!("⚠ {}", err))
                                .size(styles::FONT_SIZE_SM)
                                .color(styles::DANGER_COLOR),
                        );
                    });
                }

                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("security.ban_time"),
                    label_width,
                    |ui| {
                        let response = styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.fail2ban_ban_time_buf)
                                    .desired_width(100.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            )
                        });

                        if response.response.lost_focus() {
                            if let Ok(v) = self.fail2ban_ban_time_buf.parse::<u64>() {
                                if v == 0 {
                                    self.fail2ban_ban_time_error =
                                        Some(i18n::t("security.must_greater_0"));
                                } else {
                                    self.fail2ban_ban_time_error = None;
                                }
                            } else {
                                self.fail2ban_ban_time_error =
                                    Some(i18n::t("security.enter_valid_number"));
                            }
                        }
                    },
                    &i18n::t("security.seconds"),
                );

                if let Some(err) = &self.fail2ban_ban_time_error {
                    ui.horizontal(|ui| {
                        ui.add_sized([label_width, 24.0], egui::Label::new(""));
                        ui.label(
                            RichText::new(format!("⚠ {}", err))
                                .size(styles::FONT_SIZE_SM)
                                .color(styles::DANGER_COLOR),
                        );
                    });
                }

                ui.add_space(styles::SPACING_MD);
            }

            styles::form_row(
                ui,
                &i18n::t("security.max_connections"),
                label_width,
                |ui| {
                    let response = styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.max_connections_buf)
                                .desired_width(100.0)
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        )
                    });

                    if response.response.lost_focus() {
                        if let Ok(v) = self.max_connections_buf.parse::<usize>() {
                            if v == 0 {
                                self.max_connections_error =
                                    Some(i18n::t("security.must_greater_0"));
                            } else {
                                self.max_connections_error = None;
                            }
                        } else {
                            self.max_connections_error =
                                Some(i18n::t("security.enter_valid_number"));
                        }
                    }
                },
            );

            if let Some(err) = &self.max_connections_error {
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(format!("⚠ {}", err))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::DANGER_COLOR),
                    );
                });
            }

            styles::form_row(
                ui,
                &i18n::t("security.max_connections_per_ip"),
                label_width,
                |ui| {
                    let response = styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.max_connections_per_ip_buf)
                                .desired_width(100.0)
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        )
                    });

                    if response.response.lost_focus() {
                        if let Ok(v) = self.max_connections_per_ip_buf.parse::<usize>() {
                            if v == 0 {
                                self.max_connections_per_ip_error =
                                    Some(i18n::t("security.must_greater_0"));
                            } else {
                                self.max_connections_per_ip_error = None;
                            }
                        } else {
                            self.max_connections_per_ip_error =
                                Some(i18n::t("security.enter_valid_number"));
                        }
                    }
                },
            );

            if let Some(err) = &self.max_connections_per_ip_error {
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(format!("⚠ {}", err))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::DANGER_COLOR),
                    );
                });
            }

            ui.add_space(styles::SPACING_MD);

            ui.label(
                RichText::new(i18n::t("security.symlink_security"))
                    .size(styles::FONT_SIZE_MD)
                    .color(styles::TEXT_SECONDARY_COLOR)
                    .strong(),
            );
            ui.label(
                RichText::new(i18n::t("security.symlink_security_desc"))
                    .size(styles::FONT_SIZE_SM)
                    .color(styles::TEXT_MUTED_COLOR),
            );

            ui.add_space(styles::SPACING_XS);

            styles::form_row(ui, &i18n::t("security.allow_symlinks"), label_width, |ui| {
                ui.checkbox(&mut self.allow_symlinks, "")
                    .on_hover_text(i18n::t("security.allow_symlinks_hint"));
            });

            if !self.allow_symlinks {
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("security.symlink_disabled_secure"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::SUCCESS_COLOR),
                    );
                });
            } else {
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("security.symlink_enabled_warning"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::WARNING_COLOR),
                    );
                });
            }
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            Self::section_header(ui, "🌐", &i18n::t("security.ip_access_control"));

            ui.label(
                RichText::new(i18n::t("security.allowed_ips"))
                    .size(styles::FONT_SIZE_MD)
                    .color(styles::TEXT_SECONDARY_COLOR),
            );
            ui.label(
                RichText::new(i18n::t("security.allowed_ips_hint"))
                    .size(styles::FONT_SIZE_SM)
                    .color(styles::TEXT_MUTED_COLOR),
            );

            let allowed_response = styles::input_frame().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.allowed_ips_text)
                        .desired_rows(4)
                        .desired_width(ui.available_width())
                        .font(egui::FontId::new(
                            styles::FONT_SIZE_MD,
                            egui::FontFamily::Proportional,
                        )),
                )
            });

            if allowed_response.response.lost_focus() {
                let errors = validate_ip_cidr(&self.allowed_ips_text);
                self.validation_errors
                    .retain(|e| !e.message.contains("[Allowed]"));
                self.validation_errors
                    .extend(errors.into_iter().map(|mut e| {
                        e.message = format!("[Allowed] {}", e.message);
                        e
                    }));
            }

            ui.add_space(styles::SPACING_MD);

            ui.label(
                RichText::new(i18n::t("security.denied_ips"))
                    .size(styles::FONT_SIZE_MD)
                    .color(styles::TEXT_SECONDARY_COLOR),
            );
            ui.label(
                RichText::new(i18n::t("security.denied_ips_hint"))
                    .size(styles::FONT_SIZE_SM)
                    .color(styles::TEXT_MUTED_COLOR),
            );

            let denied_response = styles::input_frame().show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.denied_ips_text)
                        .desired_rows(4)
                        .desired_width(ui.available_width())
                        .font(egui::FontId::new(
                            styles::FONT_SIZE_MD,
                            egui::FontFamily::Proportional,
                        )),
                )
            });

            if denied_response.response.lost_focus() {
                let errors = validate_ip_cidr(&self.denied_ips_text);
                self.validation_errors
                    .retain(|e| !e.message.contains("[Denied]"));
                self.validation_errors
                    .extend(errors.into_iter().map(|mut e| {
                        e.message = format!("[Denied] {}", e.message);
                        e
                    }));
            }

            if !self.validation_errors.is_empty() {
                ui.add_space(styles::SPACING_SM);
                ui.label(
                    RichText::new(i18n::t("security.ip_validation_error"))
                        .size(styles::FONT_SIZE_SM)
                        .color(styles::DANGER_COLOR)
                        .strong(),
                );

                egui::ScrollArea::vertical()
                    .max_height(100.0)
                    .show(ui, |ui| {
                        for err in &self.validation_errors {
                            ui.label(
                                RichText::new(format!("  • {}: {}", err.field, err.message))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::WARNING_COLOR),
                            );
                        }
                    });
            }
        });
    }
}
