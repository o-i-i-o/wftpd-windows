use crate::core::config::Config;
use crate::core::config_manager::ConfigManager;
use crate::core::i18n;
use crate::core::ipc::IpcClient;
use crate::gui_egui::styles;
use egui::{RichText, Ui};
use std::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigLoadState {
    Loading,
    Loaded,
    Error,
}

#[derive(Debug)]
pub struct ServerTab {
    config_manager: ConfigManager,
    status_message: Option<(String, bool)>,
    config_load_state: ConfigLoadState,
    config_load_error: Option<String>,
    save_receiver: Option<mpsc::Receiver<Result<String, String>>>,
    is_saving: bool,
    // IP 映射表输入状态
    new_internal_ip: String,
    new_external_ip: String,
}

impl ServerTab {
    pub fn new(config_manager: ConfigManager) -> Self {
        Self {
            config_manager,
            status_message: None,
            config_load_state: ConfigLoadState::Loaded,
            config_load_error: None,
            save_receiver: None,
            is_saving: false,
            new_internal_ip: String::new(),
            new_external_ip: String::new(),
        }
    }

    fn validate_config(config: &Config) -> Vec<String> {
        let mut errors = Vec::new();

        if config.ftp.enabled {
            if config.ftp.port == 0 {
                errors.push(i18n::t("server.ftp_port_zero"));
            }

            if config.ftp.passive_ports.0 > config.ftp.passive_ports.1 {
                errors.push(i18n::t_fmt(
                    "server.passive_port_range_invalid",
                    &[&config.ftp.passive_ports.0, &config.ftp.passive_ports.1],
                ));
            }

            if config.ftp.allow_anonymous {
                if let Some(ref home) = config.ftp.anonymous_home {
                    if home.trim().is_empty() {
                        errors.push(i18n::t("server.anonymous_dir_empty"));
                    }
                } else {
                    errors.push(i18n::t("server.anonymous_no_home"));
                }
            }

            if config.ftp.ftps.enabled {
                if config
                    .ftp
                    .ftps
                    .cert_path
                    .as_ref()
                    .is_none_or(|p| p.trim().is_empty())
                {
                    errors.push(i18n::t("server.ftps_no_cert"));
                }
                if config
                    .ftp
                    .ftps
                    .key_path
                    .as_ref()
                    .is_none_or(|p| p.trim().is_empty())
                {
                    errors.push(i18n::t("server.ftps_no_key"));
                }
            }
        }

        if config.sftp.enabled {
            if config.sftp.port == 0 {
                errors.push(i18n::t("server.sftp_port_zero"));
            }

            if config.sftp.host_key_path.trim().is_empty() {
                errors.push(i18n::t("server.sftp_no_host_key"));
            }

            if config.sftp.max_auth_attempts == 0 {
                errors.push(i18n::t("server.sftp_max_auth_zero"));
            }
        }

        if config.logging.log_dir.trim().is_empty() {
            errors.push(i18n::t("server.log_dir_empty"));
        }

        if config.logging.max_log_size == 0 {
            errors.push(i18n::t("server.max_log_size_zero"));
        }

        if config.logging.max_log_files == 0 {
            errors.push(i18n::t("server.max_log_files_zero"));
        }

        errors
    }

    pub fn save_config_async(&mut self, ctx: &egui::Context, config: Config) {
        if self.is_saving {
            return;
        }

        // 先验证配置
        let validation_errors = Self::validate_config(&config);
        if !validation_errors.is_empty() {
            self.status_message = Some((
                i18n::t_fmt(
                    "server.config_validation_failed",
                    &[&validation_errors.join("\n")],
                ),
                false,
            ));
            self.is_saving = false;
            return;
        }

        self.is_saving = true;

        // 使用 config_manager 保存配置
        let config_manager = self.config_manager.clone();
        let (tx, rx) = mpsc::channel();
        self.save_receiver = Some(rx);

        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            // 先更新 config_manager 中的配置，确保保存的是经过验证的配置
            config_manager.modify(|c| *c = config.clone());

            let result = match config_manager.save(&Config::get_config_path()) {
                Ok(_) => {
                    tracing::info!("Server config saved successfully");

                    if IpcClient::is_server_running() {
                        match IpcClient::notify_reload() {
                            Ok(response) => {
                                if response.success {
                                    Ok(i18n::t("server.config_saved"))
                                } else {
                                    Ok(i18n::t_fmt(
                                        "server.config_saved_reload_failed",
                                        &[&response.message],
                                    ))
                                }
                            }
                            Err(e) => Ok(i18n::t_fmt("server.config_saved_notify_failed", &[&e])),
                        }
                    } else {
                        Ok(i18n::t("server.config_saved_not_running"))
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to save server config: {}", e);
                    Err(i18n::t_fmt("server.config_save_failed", &[&e]))
                }
            };

            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
    }

    fn check_save_result(&mut self) {
        if let Some(rx) = &self.save_receiver
            && let Ok(result) = rx.try_recv()
        {
            self.save_receiver = None;
            self.is_saving = false;

            match result {
                Ok(msg) => {
                    self.status_message = Some((msg, true));
                }
                Err(e) => {
                    self.status_message = Some((e, false));
                }
            }
        }
    }

    fn section_header(ui: &mut Ui, icon: &str, title: &str) {
        styles::section_header(ui, icon, title);
    }

    fn pick_folder(title: &str) -> Option<std::path::PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }

    fn pick_file(title: &str) -> Option<std::path::PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_file()
    }

    fn pick_cert_file(title: &str) -> Option<std::path::PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter("证书文件", &["pem", "crt", "cer"])
            .pick_file()
    }

    fn pick_key_file(title: &str) -> Option<std::path::PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter("私钥文件", &["pem", "key"])
            .pick_file()
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.check_save_result();

        match self.config_load_state {
            ConfigLoadState::Loading => {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 2.0 - 50.0);
                    ui.spinner();
                    ui.add_space(styles::SPACING_MD);
                    ui.label(
                        RichText::new(i18n::t("server.loading_config"))
                            .size(styles::FONT_SIZE_LG)
                            .color(styles::TEXT_SECONDARY_COLOR),
                    );
                });
                return;
            }
            ConfigLoadState::Error => {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 2.0 - 80.0);
                    ui.label(
                        RichText::new(i18n::t("server.config_load_failed"))
                            .size(styles::FONT_SIZE_LG)
                            .strong()
                            .color(styles::DANGER_COLOR),
                    );
                    ui.add_space(styles::SPACING_MD);
                    if let Some(error) = &self.config_load_error {
                        ui.label(
                            RichText::new(error)
                                .size(styles::FONT_SIZE_MD)
                                .color(styles::TEXT_SECONDARY_COLOR),
                        );
                    }
                });
                return;
            }
            ConfigLoadState::Loaded => {}
        }

        let is_saving = self.is_saving;
        let mut config_to_save: Option<Config> = None;
        let ctx = ui.ctx().clone();

        self.config_manager.modify(|config| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("⚙").size(styles::FONT_SIZE_XL));
                ui.label(
                    RichText::new(i18n::t("server.title"))
                        .size(styles::FONT_SIZE_XL)
                        .strong()
                        .color(styles::TEXT_PRIMARY_COLOR),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save_text = i18n::t("server.save_config");
                    let save_btn = if is_saving {
                        egui::Button::new(
                            RichText::new(i18n::t("server.saving"))
                                .color(egui::Color32::GRAY)
                                .size(styles::FONT_SIZE_MD),
                        )
                        .fill(styles::BG_SECONDARY)
                        .corner_radius(egui::CornerRadius::same(6))
                    } else {
                        styles::primary_button(&save_text)
                    };

                    if ui.add(save_btn).clicked() && !is_saving {
                        config_to_save = Some(config.clone());
                    }

                    if let Some((msg, success)) = &self.status_message {
                        let msg_text = if *success {
                            RichText::new(msg)
                                .color(styles::SUCCESS_COLOR)
                                .size(styles::FONT_SIZE_SM)
                        } else {
                            RichText::new(msg)
                                .color(styles::DANGER_COLOR)
                                .size(styles::FONT_SIZE_SM)
                        };
                        ui.label(msg_text);
                    }
                });
            });

            ui.add_space(styles::SPACING_MD);

            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                Self::section_header(ui, "📡", &i18n::t("server.ftp_settings"));

                ui.checkbox(
                    &mut config.ftp.enabled,
                    RichText::new(i18n::t("server.enable_ftp")).size(styles::FONT_SIZE_MD),
                );
                ui.add_space(styles::SPACING_MD);

                let available_width = ui.available_width();
                let label_width = (available_width * 0.15).clamp(100.0, 160.0);

                styles::form_row(ui, &i18n::t("server.bind_ip"), label_width, |ui| {
                    styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut config.ftp.bind_ip)
                                .desired_width(ui.available_width())
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        );
                    });
                });

                styles::form_row(ui, &i18n::t("server.ftp_port"), label_width, |ui| {
                    let mut port_str = config.ftp.port.to_string();
                    styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut port_str)
                                .desired_width(80.0)
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        );
                    });
                    if let Ok(p) = port_str.parse::<u16>() {
                        config.ftp.port = p;
                    }
                });

                styles::form_row(ui, &i18n::t("server.welcome_msg"), label_width, |ui| {
                    styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut config.ftp.welcome_message)
                                .desired_width(ui.available_width())
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        );
                    });
                });

                styles::form_row(ui, &i18n::t("server.encoding"), label_width, |ui| {
                    styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut config.ftp.encoding)
                                .desired_width(100.0)
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        );
                    });
                });

                styles::form_row(ui, &i18n::t("server.transfer_mode"), label_width, |ui| {
                    let modes = ["binary", "ascii"];
                    egui::ComboBox::from_id_salt("transfer_mode")
                        .selected_text(&config.ftp.default_transfer_mode)
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            for mode in modes {
                                ui.selectable_value(
                                    &mut config.ftp.default_transfer_mode,
                                    mode.to_string(),
                                    mode,
                                );
                            }
                        });
                    ui.label(
                        RichText::new(i18n::t("server.transfer_mode_hint"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR),
                    );
                });

                styles::form_row(ui, &i18n::t("server.connection_mode"), label_width, |ui| {
                    let passive_label = if config.ftp.default_passive_mode {
                        i18n::t("server.passive_mode")
                    } else {
                        i18n::t("server.active_mode")
                    };
                    egui::ComboBox::from_id_salt("connection_mode")
                        .selected_text(&passive_label)
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut config.ftp.default_passive_mode,
                                true,
                                i18n::t("server.passive_mode"),
                            );
                            ui.selectable_value(
                                &mut config.ftp.default_passive_mode,
                                false,
                                i18n::t("server.active_mode"),
                            );
                        });
                    ui.label(
                        RichText::new(i18n::t("server.passive_mode_hint"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR),
                    );
                });

                styles::form_row(ui, &i18n::t("server.allow_anonymous"), label_width, |ui| {
                    ui.checkbox(&mut config.ftp.allow_anonymous, "");
                });

                if config.ftp.allow_anonymous {
                    styles::form_row(ui, &i18n::t("server.anonymous_dir"), label_width, |ui| {
                        let mut anon_home = config.ftp.anonymous_home.clone().unwrap_or_default();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut anon_home)
                                    .desired_width(ui.available_width() - 80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if ui.button(i18n::t("server.browse")).clicked()
                            && let Some(path) =
                                Self::pick_folder(&i18n::t("server.select_anonymous_dir"))
                        {
                            anon_home = path.to_string_lossy().to_string();
                        }
                        config.ftp.anonymous_home = if anon_home.is_empty() {
                            None
                        } else {
                            Some(anon_home)
                        };
                    });

                    if config
                        .ftp
                        .anonymous_home
                        .as_ref()
                        .is_none_or(|s| s.trim().is_empty())
                    {
                        ui.horizontal(|ui| {
                            ui.add_sized([label_width, 24.0], egui::Label::new(""));
                            ui.label(
                                RichText::new(i18n::t("server.anonymous_dir_not_configured"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::WARNING_COLOR),
                            );
                        });
                    }
                }

                styles::form_row(
                    ui,
                    &i18n::t("server.passive_port_range"),
                    label_width,
                    |ui| {
                        let mut min_str = config.ftp.passive_ports.0.to_string();
                        let mut max_str = config.ftp.passive_ports.1.to_string();

                        ui.label(
                            RichText::new(i18n::t("server.from"))
                                .size(styles::FONT_SIZE_MD)
                                .color(styles::TEXT_MUTED_COLOR),
                        );
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut min_str)
                                    .desired_width(60.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(p) = min_str.parse::<u16>() {
                            config.ftp.passive_ports.0 = p;
                        }

                        ui.label(
                            RichText::new(i18n::t("server.to"))
                                .size(styles::FONT_SIZE_MD)
                                .color(styles::TEXT_MUTED_COLOR),
                        );
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut max_str)
                                    .desired_width(60.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(p) = max_str.parse::<u16>() {
                            config.ftp.passive_ports.1 = p;
                        }

                        if config.ftp.passive_ports.0 > config.ftp.passive_ports.1 {
                            ui.label(
                                RichText::new("⚠ start port > end port")
                                    .color(styles::DANGER_COLOR)
                                    .size(styles::FONT_SIZE_SM),
                            );
                        }
                    },
                );

                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("server.max_speed"),
                    label_width,
                    |ui| {
                        let mut speed_str = config.ftp.max_speed_kbps.to_string();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut speed_str)
                                    .desired_width(80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(v) = speed_str.parse::<u64>() {
                            config.ftp.max_speed_kbps = v;
                        }
                    },
                    &i18n::t("server.max_speed_hint"),
                );

                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new(i18n::t("server.passive_ip_override"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR)
                        .strong(),
                );
                ui.label(
                    RichText::new(i18n::t("server.passive_ip_override"))
                        .size(styles::FONT_SIZE_SM)
                        .color(styles::TEXT_MUTED_COLOR)
                        .italics(),
                );
                ui.add_space(styles::SPACING_SM);

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.checkbox(&mut config.ftp.upnp_enabled, i18n::t("server.upnp_enabled"));
                });

                ui.add_space(styles::SPACING_SM);

                let mut passive_ip = config.ftp.passive_ip_override.clone().unwrap_or_default();
                styles::form_row(
                    ui,
                    &i18n::t("server.passive_ip_override"),
                    label_width,
                    |ui| {
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut passive_ip)
                                    .desired_width(ui.available_width())
                                    .hint_text("例如: 203.0.113.50")
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                    },
                );
                config.ftp.passive_ip_override = if passive_ip.trim().is_empty() {
                    None
                } else {
                    Some(passive_ip)
                };

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("server.masquerade_address"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });

                let mut masq_addr = config.ftp.masquerade_address.clone().unwrap_or_default();
                styles::form_row(
                    ui,
                    &i18n::t("server.masquerade_address"),
                    label_width,
                    |ui| {
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut masq_addr)
                                    .desired_width(ui.available_width())
                                    .hint_text("例如: ftp.example.com")
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                    },
                );
                config.ftp.masquerade_address = if masq_addr.trim().is_empty() {
                    None
                } else {
                    Some(masq_addr)
                };

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("server.masquerade_address"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });

                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new(i18n::t("server.masquerade_map"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR)
                        .strong(),
                );
                ui.label(
                    RichText::new(i18n::t("server.masquerade_map"))
                        .size(styles::FONT_SIZE_SM)
                        .color(styles::TEXT_MUTED_COLOR)
                        .italics(),
                );
                ui.add_space(styles::SPACING_XS);

                // 显示当前映射列表
                let mut map_entries: Vec<(String, String)> = config
                    .ftp
                    .masquerade_map
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                map_entries.sort_by(|a, b| a.0.cmp(&b.0));

                if !map_entries.is_empty() {
                    egui::Frame::NONE
                        .fill(styles::BG_SECONDARY)
                        .inner_margin(egui::Margin::same(8))
                        .corner_radius(egui::CornerRadius::same(6))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                for (idx, (internal_ip, external_ip)) in
                                    map_entries.iter().enumerate()
                                {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "{} → {}",
                                                internal_ip, external_ip
                                            ))
                                            .size(styles::FONT_SIZE_SM)
                                            .color(styles::TEXT_LABEL_COLOR),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .button(
                                                        RichText::new("✕")
                                                            .size(styles::FONT_SIZE_SM)
                                                            .color(styles::DANGER_COLOR),
                                                    )
                                                    .clicked()
                                                {
                                                    config.ftp.masquerade_map.remove(internal_ip);
                                                }
                                            },
                                        );
                                    });
                                    if idx < map_entries.len() - 1 {
                                        ui.separator();
                                    }
                                }
                            });
                        });
                    ui.add_space(styles::SPACING_XS);
                }

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("server.add_new_mapping"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_SECONDARY_COLOR),
                    );
                });

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));

                    let internal_ip_valid = self.new_internal_ip.trim().is_empty()
                        || self
                            .new_internal_ip
                            .trim()
                            .parse::<std::net::Ipv4Addr>()
                            .is_ok()
                        || self
                            .new_internal_ip
                            .trim()
                            .parse::<std::net::Ipv6Addr>()
                            .is_ok();

                    let internal_ip_frame =
                        if !self.new_internal_ip.trim().is_empty() && !internal_ip_valid {
                            egui::Frame::new()
                                .stroke(egui::Stroke::new(1.0, styles::DANGER_COLOR))
                                .inner_margin(egui::Margin::same(0))
                                .corner_radius(egui::CornerRadius::same(6))
                        } else {
                            styles::input_frame()
                        };

                    internal_ip_frame.show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_internal_ip)
                                .desired_width(120.0)
                                .hint_text(i18n::t("server.internal_ip"))
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_SM,
                                    egui::FontFamily::Monospace,
                                )),
                        );
                    });

                    ui.label(
                        RichText::new("→")
                            .size(styles::FONT_SIZE_MD)
                            .color(styles::TEXT_MUTED_COLOR),
                    );

                    let external_ip_valid = self.new_external_ip.trim().is_empty()
                        || self
                            .new_external_ip
                            .trim()
                            .parse::<std::net::Ipv4Addr>()
                            .is_ok()
                        || self
                            .new_external_ip
                            .trim()
                            .parse::<std::net::Ipv6Addr>()
                            .is_ok();

                    let external_ip_frame =
                        if !self.new_external_ip.trim().is_empty() && !external_ip_valid {
                            egui::Frame::new()
                                .stroke(egui::Stroke::new(1.0, styles::DANGER_COLOR))
                                .inner_margin(egui::Margin::same(0))
                                .corner_radius(egui::CornerRadius::same(6))
                        } else {
                            styles::input_frame()
                        };

                    external_ip_frame.show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_external_ip)
                                .desired_width(120.0)
                                .hint_text(i18n::t("server.external_ip"))
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_SM,
                                    egui::FontFamily::Monospace,
                                )),
                        );
                    });

                    let can_add = !self.new_internal_ip.trim().is_empty()
                        && !self.new_external_ip.trim().is_empty()
                        && internal_ip_valid
                        && external_ip_valid;

                    let add_button = if can_add {
                        egui::Button::new(
                            RichText::new(i18n::t("server.add_mapping"))
                                .size(styles::FONT_SIZE_SM)
                                .color(egui::Color32::WHITE),
                        )
                        .fill(styles::PRIMARY_COLOR)
                    } else {
                        egui::Button::new(
                            RichText::new(i18n::t("server.add_mapping")).size(styles::FONT_SIZE_SM),
                        )
                        .fill(styles::BG_SECONDARY)
                    };

                    if ui.add(add_button).clicked() && can_add {
                        config.ftp.masquerade_map.insert(
                            self.new_internal_ip.trim().to_string(),
                            self.new_external_ip.trim().to_string(),
                        );
                        self.new_internal_ip.clear();
                        self.new_external_ip.clear();
                    }
                });

                // 显示验证错误提示
                let internal_ip_valid = self.new_internal_ip.trim().is_empty()
                    || self
                        .new_internal_ip
                        .trim()
                        .parse::<std::net::Ipv4Addr>()
                        .is_ok()
                    || self
                        .new_internal_ip
                        .trim()
                        .parse::<std::net::Ipv6Addr>()
                        .is_ok();

                let external_ip_valid = self.new_external_ip.trim().is_empty()
                    || self
                        .new_external_ip
                        .trim()
                        .parse::<std::net::Ipv4Addr>()
                        .is_ok()
                    || self
                        .new_external_ip
                        .trim()
                        .parse::<std::net::Ipv6Addr>()
                        .is_ok();

                if !self.new_internal_ip.trim().is_empty() && !internal_ip_valid {
                    ui.horizontal(|ui| {
                        ui.add_sized([label_width, 24.0], egui::Label::new(""));
                        ui.label(
                            RichText::new(i18n::t("server.internal_ip_invalid"))
                                .size(styles::FONT_SIZE_XS)
                                .color(styles::DANGER_COLOR),
                        );
                    });
                }

                if !self.new_external_ip.trim().is_empty() && !external_ip_valid {
                    ui.horizontal(|ui| {
                        ui.add_sized([label_width, 24.0], egui::Label::new(""));
                        ui.label(
                            RichText::new(i18n::t("server.external_ip_invalid"))
                                .size(styles::FONT_SIZE_XS)
                                .color(styles::DANGER_COLOR),
                        );
                    });
                }

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("server.masquerade_priority"))
                            .size(styles::FONT_SIZE_XS)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });

                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new(i18n::t("server.timeout_settings"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR)
                        .strong(),
                );
                ui.add_space(styles::SPACING_SM);

                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("server.connection_timeout"),
                    label_width,
                    |ui| {
                        let mut val_str = config.ftp.connection_timeout.to_string();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut val_str)
                                    .desired_width(80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(v) = val_str.parse::<u64>() {
                            config.ftp.connection_timeout = v;
                        }
                    },
                    &i18n::t("server.seconds"),
                );

                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("server.idle_timeout"),
                    label_width,
                    |ui| {
                        let mut val_str = config.ftp.idle_timeout.to_string();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut val_str)
                                    .desired_width(80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(v) = val_str.parse::<u64>() {
                            config.ftp.idle_timeout = v;
                        }
                    },
                    &i18n::t("server.idle_timeout_hint"),
                );

                ui.add_space(styles::SPACING_SM);

                styles::form_row(
                    ui,
                    &i18n::t("server.hide_version_info"),
                    label_width,
                    |ui| {
                        ui.checkbox(&mut config.ftp.hide_version_info, "");
                    },
                );
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("server.hide_version_info_hint"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });
            });

            ui.add_space(styles::SPACING_MD);

            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                Self::section_header(ui, "🔒", &i18n::t("server.ftps_settings"));

                ui.checkbox(
                    &mut config.ftp.ftps.enabled,
                    RichText::new(i18n::t("server.enable_ftps")).size(styles::FONT_SIZE_MD),
                );
                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new(i18n::t("server.ftps_description"))
                        .size(styles::FONT_SIZE_SM)
                        .color(styles::TEXT_MUTED_COLOR)
                        .italics(),
                );
                ui.add_space(styles::SPACING_MD);

                let available_width = ui.available_width();
                let label_width = (available_width * 0.15).clamp(100.0, 160.0);

                if config.ftp.ftps.enabled {
                    styles::form_row(ui, &i18n::t("server.require_ssl"), label_width, |ui| {
                        ui.checkbox(&mut config.ftp.ftps.require_ssl, "");
                    });
                    ui.horizontal(|ui| {
                        ui.add_sized([label_width, 24.0], egui::Label::new(""));
                        ui.label(
                            RichText::new(i18n::t("server.require_ssl_hint"))
                                .size(styles::FONT_SIZE_SM)
                                .color(styles::TEXT_MUTED_COLOR)
                                .italics(),
                        );
                    });

                    ui.add_space(styles::SPACING_SM);

                    styles::form_row(ui, &i18n::t("server.implicit_ssl"), label_width, |ui| {
                        ui.checkbox(&mut config.ftp.ftps.implicit_ssl, "");
                    });
                    ui.horizontal(|ui| {
                        ui.add_sized([label_width, 24.0], egui::Label::new(""));
                        ui.label(
                            RichText::new(i18n::t("server.implicit_ssl_hint"))
                                .size(styles::FONT_SIZE_SM)
                                .color(styles::TEXT_MUTED_COLOR)
                                .italics(),
                        );
                    });

                    if config.ftp.ftps.implicit_ssl {
                        styles::form_row(
                            ui,
                            &i18n::t("server.implicit_ssl_port"),
                            label_width,
                            |ui| {
                                let mut port_str = config.ftp.ftps.implicit_ssl_port.to_string();
                                styles::input_frame().show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut port_str)
                                            .desired_width(80.0)
                                            .font(egui::FontId::new(
                                                styles::FONT_SIZE_MD,
                                                egui::FontFamily::Proportional,
                                            )),
                                    );
                                });
                                if let Ok(p) = port_str.parse::<u16>() {
                                    config.ftp.ftps.implicit_ssl_port = p;
                                }
                            },
                        );
                    }

                    ui.add_space(styles::SPACING_SM);

                    let mut cert_path = config.ftp.ftps.cert_path.clone().unwrap_or_default();
                    styles::form_row(ui, &i18n::t("server.cert_file"), label_width, |ui| {
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut cert_path)
                                    .desired_width(ui.available_width() - 80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if ui.button(i18n::t("server.browse")).clicked()
                            && let Some(path) =
                                Self::pick_cert_file(&i18n::t("server.select_cert_file"))
                        {
                            cert_path = path.to_string_lossy().to_string();
                        }
                    });
                    config.ftp.ftps.cert_path = if cert_path.trim().is_empty() {
                        None
                    } else {
                        Some(cert_path)
                    };

                    if let Some(cert_path) = &config.ftp.ftps.cert_path {
                        let cert_exists = std::path::Path::new(cert_path).exists();
                        let cert_status = if cert_exists {
                            (i18n::t("server.cert_exists"), styles::SUCCESS_COLOR)
                        } else {
                            (i18n::t("server.cert_not_exists"), styles::DANGER_COLOR)
                        };
                        ui.horizontal(|ui| {
                            ui.add_sized([label_width, 24.0], egui::Label::new(""));
                            ui.label(
                                RichText::new(cert_status.0)
                                    .size(styles::FONT_SIZE_SM)
                                    .color(cert_status.1)
                                    .italics(),
                            );
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.add_sized([label_width, 24.0], egui::Label::new(""));
                            ui.label(
                                RichText::new(i18n::t("server.cert_not_configured"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::WARNING_COLOR)
                                    .italics(),
                            );
                        });
                    }

                    ui.add_space(styles::SPACING_SM);

                    let mut key_path = config.ftp.ftps.key_path.clone().unwrap_or_default();
                    styles::form_row(ui, &i18n::t("server.key_file"), label_width, |ui| {
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut key_path)
                                    .desired_width(ui.available_width() - 80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if ui.button(i18n::t("server.browse")).clicked()
                            && let Some(path) =
                                Self::pick_key_file(&i18n::t("server.select_key_file"))
                        {
                            key_path = path.to_string_lossy().to_string();
                        }
                    });
                    config.ftp.ftps.key_path = if key_path.trim().is_empty() {
                        None
                    } else {
                        Some(key_path)
                    };

                    if let Some(key_path) = &config.ftp.ftps.key_path {
                        let key_exists = std::path::Path::new(key_path).exists();
                        let key_status = if key_exists {
                            (i18n::t("server.key_exists"), styles::SUCCESS_COLOR)
                        } else {
                            (i18n::t("server.key_not_exists"), styles::DANGER_COLOR)
                        };
                        ui.horizontal(|ui| {
                            ui.add_sized([label_width, 24.0], egui::Label::new(""));
                            ui.label(
                                RichText::new(key_status.0)
                                    .size(styles::FONT_SIZE_SM)
                                    .color(key_status.1)
                                    .italics(),
                            );
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.add_sized([label_width, 24.0], egui::Label::new(""));
                            ui.label(
                                RichText::new(i18n::t("server.key_not_configured"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::WARNING_COLOR)
                                    .italics(),
                            );
                        });
                    }
                }
            });

            ui.add_space(styles::SPACING_MD);

            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                Self::section_header(ui, "🔐", &i18n::t("server.sftp_settings"));

                ui.checkbox(
                    &mut config.sftp.enabled,
                    RichText::new(i18n::t("server.enable_sftp")).size(styles::FONT_SIZE_MD),
                );
                ui.add_space(styles::SPACING_MD);

                let available_width = ui.available_width();
                let label_width = (available_width * 0.15).clamp(100.0, 160.0);

                styles::form_row(ui, &i18n::t("server.bind_ip"), label_width, |ui| {
                    styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut config.sftp.bind_ip)
                                .desired_width(ui.available_width())
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        );
                    });
                });

                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("server.sftp_port"),
                    label_width,
                    |ui| {
                        let mut port_str = config.sftp.port.to_string();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut port_str)
                                    .desired_width(80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(p) = port_str.parse::<u16>() {
                            config.sftp.port = p;
                        }
                    },
                    &i18n::t("server.sftp_port_hint"),
                );

                let mut host_key_path = config.sftp.host_key_path.clone();
                styles::form_row(ui, &i18n::t("server.host_key_path"), label_width, |ui| {
                    styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut host_key_path)
                                .desired_width(ui.available_width() - 80.0)
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        );
                    });
                    if ui.button(i18n::t("server.browse")).clicked()
                        && let Some(path) = Self::pick_file(&i18n::t("server.select_host_key_file"))
                    {
                        host_key_path = path.to_string_lossy().to_string();
                    }
                });
                config.sftp.host_key_path = host_key_path;

                let host_key_exists =
                    std::path::Path::new(config.sftp.host_key_path.trim()).exists();
                let host_key_status = if host_key_exists {
                    (i18n::t("server.file_exists"), styles::SUCCESS_COLOR)
                } else {
                    (
                        i18n::t("server.host_key_auto_gen"),
                        styles::TEXT_MUTED_COLOR,
                    )
                };

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(host_key_status.0)
                            .size(styles::FONT_SIZE_SM)
                            .color(host_key_status.1)
                            .italics(),
                    );
                });

                styles::form_row(
                    ui,
                    &i18n::t("server.max_auth_attempts"),
                    label_width,
                    |ui| {
                        let mut val_str = config.sftp.max_auth_attempts.to_string();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut val_str)
                                    .desired_width(80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(v) = val_str.parse::<u32>() {
                            config.sftp.max_auth_attempts = v;
                        }
                    },
                );

                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("server.auth_timeout"),
                    label_width,
                    |ui| {
                        let mut val_str = config.sftp.auth_timeout.to_string();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut val_str)
                                    .desired_width(100.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(v) = val_str.parse::<u64>() {
                            config.sftp.auth_timeout = v;
                        }
                    },
                    &i18n::t("server.seconds"),
                );

                styles::form_row(ui, &i18n::t("server.log_level"), label_width, |ui| {
                    styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut config.sftp.log_level)
                                .desired_width(120.0)
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        );
                    });
                });

                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new(i18n::t("server.security_enhancement"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR)
                        .strong(),
                );
                ui.add_space(styles::SPACING_SM);

                styles::form_row(
                    ui,
                    &i18n::t("server.max_sessions_per_user"),
                    label_width,
                    |ui| {
                        let mut val_str = config.sftp.max_sessions_per_user.to_string();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut val_str)
                                    .desired_width(80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(v) = val_str.parse::<u32>() {
                            config.sftp.max_sessions_per_user = v;
                        }
                    },
                );

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("server.max_sessions_per_user_hint"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });
            });

            ui.add_space(styles::SPACING_MD);

            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                Self::section_header(ui, "📋", &i18n::t("server.global_log_settings"));

                let available_width = ui.available_width();
                let label_width = (available_width * 0.15).clamp(100.0, 160.0);

                let mut log_dir = config.logging.log_dir.clone();
                styles::form_row(ui, &i18n::t("server.log_dir"), label_width, |ui| {
                    styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut log_dir)
                                .desired_width(ui.available_width() - 80.0)
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        );
                    });
                    if ui.button(i18n::t("server.browse")).clicked()
                        && let Some(path) = Self::pick_folder(&i18n::t("server.select_log_dir"))
                    {
                        log_dir = path.to_string_lossy().to_string();
                    }
                });
                config.logging.log_dir = log_dir;

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("server.log_dir_hint"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });

                styles::form_row(ui, &i18n::t("server.log_level"), label_width, |ui| {
                    let levels = ["trace", "debug", "info", "warn", "error"];
                    egui::ComboBox::from_id_salt("log_level")
                        .selected_text(&config.logging.log_level)
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            for level in levels {
                                ui.selectable_value(
                                    &mut config.logging.log_level,
                                    level.to_string(),
                                    level,
                                );
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("server.log_level_hint"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });

                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("server.max_log_size"),
                    label_width,
                    |ui| {
                        let size_mb = config.logging.max_log_size / 1024 / 1024;
                        let mut size_str = size_mb.to_string();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut size_str)
                                    .desired_width(80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(v) = size_str.parse::<u64>() {
                            config.logging.max_log_size = v * 1024 * 1024;
                        }
                    },
                    "MB",
                );

                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("server.max_log_files"),
                    label_width,
                    |ui| {
                        let mut files_str = config.logging.max_log_files.to_string();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut files_str)
                                    .desired_width(80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(v) = files_str.parse::<usize>() {
                            config.logging.max_log_files = v;
                        }
                    },
                    &i18n::t("server.max_log_files_hint"),
                );

                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new(i18n::t("server.notes"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR)
                        .strong(),
                );

                egui::Frame::NONE
                    .fill(styles::BG_INFO)
                    .inner_margin(egui::Margin::same(12))
                    .corner_radius(egui::CornerRadius::same(6))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(i18n::t("server.note_1"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                            ui.label(
                                RichText::new(i18n::t("server.note_2"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                            ui.label(
                                RichText::new(i18n::t("server.note_3"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                            ui.label(
                                RichText::new(i18n::t("server.note_4"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                        });
                    });
            });
        });

        if let Some(config) = config_to_save {
            self.save_config_async(&ctx, config);
        }
    }
}
