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

            if let Err(e) = tx.send(result) {
                tracing::debug!("Failed to send server config save result: {}", e);
            }
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

    fn section_header_with_save(
        ui: &mut Ui,
        icon: &str,
        title: &str,
        is_saving: bool,
        status_message: Option<&(String, bool)>,
    ) -> bool {
        let mut clicked = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(icon).size(styles::FONT_SIZE_LG));
            ui.label(
                egui::RichText::new(title)
                    .size(styles::FONT_SIZE_LG)
                    .strong()
                    .color(styles::TEXT_PRIMARY_COLOR),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save_text = i18n::t("server.save_config");
                let save_btn = if is_saving {
                    egui::Button::new(
                        egui::RichText::new(i18n::t("server.saving")).size(styles::FONT_SIZE_MD),
                    )
                    .fill(styles::BG_SECONDARY)
                    .corner_radius(egui::CornerRadius::same(6))
                } else {
                    styles::primary_button(&save_text)
                };

                if ui.add(save_btn).clicked() && !is_saving {
                    clicked = true;
                }

                if let Some((msg, success)) = status_message {
                    let msg_text = if *success {
                        egui::RichText::new(msg)
                            .color(styles::SUCCESS_COLOR)
                            .size(styles::FONT_SIZE_SM)
                    } else {
                        egui::RichText::new(msg)
                            .color(styles::DANGER_COLOR)
                            .size(styles::FONT_SIZE_SM)
                    };
                    ui.label(msg_text);
                }
            });
        });
        ui.add_space(styles::SPACING_SM);
        clicked
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
            .add_filter(
                crate::core::i18n::t("file_filter.cert"),
                &["pem", "crt", "cer"],
            )
            .pick_file()
    }

    fn pick_key_file(title: &str) -> Option<std::path::PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter(crate::core::i18n::t("file_filter.key"), &["pem", "key"])
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
            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if Self::section_header_with_save(
                    ui,
                    "📡",
                    &i18n::t("server.ftp_settings"),
                    is_saving,
                    self.status_message.as_ref(),
                ) {
                    config_to_save = Some(config.clone());
                }

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

                styles::form_row(ui, &i18n::t("server.upnp_enabled"), label_width, |ui| {
                    ui.checkbox(&mut config.ftp.upnp_enabled, "");
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
                                    .hint_text(i18n::t("server.nat_domain_hint"))
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

                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("server.hide_version_info"),
                    label_width,
                    |ui| {
                        ui.checkbox(&mut config.ftp.hide_version_info, "");
                    },
                    &i18n::t("server.hide_version_info_hint"),
                );
            });

            ui.add_space(styles::SPACING_MD);

            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if Self::section_header_with_save(
                    ui,
                    "🔒",
                    &i18n::t("server.ftps_settings"),
                    is_saving,
                    self.status_message.as_ref(),
                ) {
                    config_to_save = Some(config.clone());
                }

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
                    styles::form_row_with_suffix(
                        ui,
                        &i18n::t("server.require_ssl"),
                        label_width,
                        |ui| {
                            ui.checkbox(&mut config.ftp.ftps.require_ssl, "");
                        },
                        &i18n::t("server.require_ssl_hint"),
                    );

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
                if Self::section_header_with_save(
                    ui,
                    "🔐",
                    &i18n::t("server.sftp_settings"),
                    is_saving,
                    self.status_message.as_ref(),
                ) {
                    config_to_save = Some(config.clone());
                }

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

                styles::form_row_with_suffix(
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
                    &i18n::t("server.max_sessions_per_user_hint"),
                );
            });
        });

        if let Some(config) = config_to_save {
            self.save_config_async(&ctx, config);
        }
    }
}
