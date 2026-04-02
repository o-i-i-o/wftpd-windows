use crate::core::config::Config;
use crate::core::config_manager::ConfigManager;
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

    /// 验证配置的有效性
    fn validate_config(config: &Config) -> Vec<String> {
        let mut errors = Vec::new();

        // FTP 配置验证
        if config.ftp.enabled {
            if config.ftp.port == 0 {
                errors.push("FTP 端口不能为 0".to_string());
            }

            if config.ftp.passive_ports.0 > config.ftp.passive_ports.1 {
                errors.push(format!(
                    "被动端口范围无效：{} - {}",
                    config.ftp.passive_ports.0, config.ftp.passive_ports.1
                ));
            }

            if config.ftp.allow_anonymous {
                if let Some(ref home) = config.ftp.anonymous_home {
                    if home.trim().is_empty() {
                        errors.push("匿名用户主目录不能为空".to_string());
                    }
                } else {
                    errors.push("匿名用户已启用但未配置主目录".to_string());
                }
            }

            // FTPS 配置验证
            if config.ftp.ftps.enabled {
                if config
                    .ftp
                    .ftps
                    .cert_path
                    .as_ref()
                    .is_none_or(|p| p.trim().is_empty())
                {
                    errors.push("FTPS 已启用但未配置证书路径".to_string());
                }
                if config
                    .ftp
                    .ftps
                    .key_path
                    .as_ref()
                    .is_none_or(|p| p.trim().is_empty())
                {
                    errors.push("FTPS 已启用但未配置私钥路径".to_string());
                }
            }
        }

        // SFTP 配置验证
        if config.sftp.enabled {
            if config.sftp.port == 0 {
                errors.push("SFTP 端口不能为 0".to_string());
            }

            if config.sftp.host_key_path.trim().is_empty() {
                errors.push("SFTP 主机密钥路径不能为空".to_string());
            }

            if config.sftp.max_auth_attempts == 0 {
                errors.push("SFTP 最大认证次数必须大于 0".to_string());
            }
        }

        // 日志配置验证
        if config.logging.log_dir.trim().is_empty() {
            errors.push("日志目录不能为空".to_string());
        }

        if config.logging.max_log_size == 0 {
            errors.push("单个日志文件最大大小必须大于 0".to_string());
        }

        if config.logging.max_log_files == 0 {
            errors.push("最大日志文件数必须大于 0".to_string());
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
                format!("配置验证失败:\n{}", validation_errors.join("\n")),
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
            let result = match config_manager.save(&Config::get_config_path()) {
                Ok(_) => {
                    tracing::info!("服务器配置保存成功");

                    if IpcClient::is_server_running() {
                        match IpcClient::notify_reload() {
                            Ok(response) => {
                                if response.success {
                                    Ok("配置已保存，后端服务已重新加载配置".to_string())
                                } else {
                                    Ok(format!(
                                        "配置已保存，但后端重新加载失败: {}",
                                        response.message
                                    ))
                                }
                            }
                            Err(e) => Ok(format!(
                                "配置已保存，但通知后端失败: {}。请手动重启服务。",
                                e
                            )),
                        }
                    } else {
                        Ok("配置已保存（后端服务未运行）".to_string())
                    }
                }
                Err(e) => {
                    tracing::error!("保存服务器配置失败: {}", e);
                    Err(format!("保存失败: {}", e))
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
                        RichText::new("正在加载配置...")
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
                        RichText::new("⚠ 配置加载失败")
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
                    RichText::new("服务器配置")
                        .size(styles::FONT_SIZE_XL)
                        .strong()
                        .color(styles::TEXT_PRIMARY_COLOR),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save_btn = if is_saving {
                        egui::Button::new(
                            RichText::new("💾 保存中...")
                                .color(egui::Color32::GRAY)
                                .size(styles::FONT_SIZE_MD),
                        )
                        .fill(styles::BG_SECONDARY)
                        .corner_radius(egui::CornerRadius::same(6))
                    } else {
                        styles::primary_button("💾 保存配置")
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
                Self::section_header(ui, "📡", "FTP 设置");

                ui.checkbox(
                    &mut config.ftp.enabled,
                    RichText::new("启用 FTP 服务").size(styles::FONT_SIZE_MD),
                );
                ui.add_space(styles::SPACING_MD);

                let available_width = ui.available_width();
                let label_width = (available_width * 0.15).clamp(100.0, 160.0);

                styles::form_row(ui, "绑定 IP", label_width, |ui| {
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

                styles::form_row(ui, "FTP 端口", label_width, |ui| {
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

                styles::form_row(ui, "欢迎消息", label_width, |ui| {
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

                styles::form_row(ui, "编码", label_width, |ui| {
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

                styles::form_row(ui, "默认传输模式", label_width, |ui| {
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
                        RichText::new("(binary: 二进制，ascii: 文本)")
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR),
                    );
                });

                styles::form_row(ui, "默认连接模式", label_width, |ui| {
                    let passive_label = if config.ftp.default_passive_mode {
                        "被动模式 (PASV)"
                    } else {
                        "主动模式 (PORT)"
                    };
                    egui::ComboBox::from_id_salt("connection_mode")
                        .selected_text(passive_label)
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut config.ftp.default_passive_mode,
                                true,
                                "被动模式 (PASV)",
                            );
                            ui.selectable_value(
                                &mut config.ftp.default_passive_mode,
                                false,
                                "主动模式 (PORT)",
                            );
                        });
                    ui.label(
                        RichText::new("(被动模式兼容性更好)")
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR),
                    );
                });

                styles::form_row(ui, "允许匿名访问", label_width, |ui| {
                    ui.checkbox(&mut config.ftp.allow_anonymous, "");
                });

                if config.ftp.allow_anonymous {
                    styles::form_row(ui, "匿名目录", label_width, |ui| {
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
                        if ui.button("浏览...").clicked()
                            && let Some(path) = Self::pick_folder("选择匿名用户目录")
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
                                RichText::new("⚠ 匿名用户目录未配置，匿名访问将无法使用")
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::WARNING_COLOR),
                            );
                        });
                    }
                }

                styles::form_row(ui, "被动端口范围", label_width, |ui| {
                    let mut min_str = config.ftp.passive_ports.0.to_string();
                    let mut max_str = config.ftp.passive_ports.1.to_string();

                    ui.label(
                        RichText::new("从")
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
                        RichText::new("到")
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
                            RichText::new("⚠ 起始端口不能大于结束端口")
                                .color(styles::DANGER_COLOR)
                                .size(styles::FONT_SIZE_SM),
                        );
                    }
                });

                styles::form_row_with_suffix(
                    ui,
                    "最大传输速度",
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
                    "KB/s (0 表示不限制)",
                );

                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new("NAT 环境设置")
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR)
                        .strong(),
                );
                ui.label(
                    RichText::new(
                        "如果服务器在 NAT 网络环境中，配置以下选项以确保被动模式正常工作",
                    )
                    .size(styles::FONT_SIZE_SM)
                    .color(styles::TEXT_MUTED_COLOR)
                    .italics(),
                );
                ui.add_space(styles::SPACING_SM);

                let mut passive_ip = config.ftp.passive_ip_override.clone().unwrap_or_default();
                styles::form_row(ui, "被动模式 IP", label_width, |ui| {
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
                });
                config.ftp.passive_ip_override = if passive_ip.trim().is_empty() {
                    None
                } else {
                    Some(passive_ip)
                };

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new("PASV 响应中返回的外部 IP 地址")
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });

                let mut masq_addr = config.ftp.masquerade_address.clone().unwrap_or_default();
                styles::form_row(ui, "伪装地址", label_width, |ui| {
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
                });
                config.ftp.masquerade_address = if masq_addr.trim().is_empty() {
                    None
                } else {
                    Some(masq_addr)
                };

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new("用于 NAT 环境的公网地址或域名")
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });

                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new("连接超时设置")
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR)
                        .strong(),
                );
                ui.add_space(styles::SPACING_SM);

                styles::form_row_with_suffix(
                    ui,
                    "连接超时",
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
                    "秒",
                );

                styles::form_row_with_suffix(
                    ui,
                    "空闲超时",
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
                    "秒 (0 表示不限制)",
                );

                ui.add_space(styles::SPACING_SM);

                styles::form_row(ui, "隐藏版本信息", label_width, |ui| {
                    ui.checkbox(&mut config.ftp.hide_version_info, "");
                });
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new("在欢迎消息和服务器响应中隐藏版本号，增强安全性")
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });
            });

            ui.add_space(styles::SPACING_MD);

            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                Self::section_header(ui, "🔒", "FTPS 设置 (FTP over SSL/TLS)");

                ui.checkbox(
                    &mut config.ftp.ftps.enabled,
                    RichText::new("启用 FTPS").size(styles::FONT_SIZE_MD),
                );
                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new("FTPS 为 FTP 连接提供 SSL/TLS 加密保护")
                        .size(styles::FONT_SIZE_SM)
                        .color(styles::TEXT_MUTED_COLOR)
                        .italics(),
                );
                ui.add_space(styles::SPACING_MD);

                let available_width = ui.available_width();
                let label_width = (available_width * 0.15).clamp(100.0, 160.0);

                if config.ftp.ftps.enabled {
                    styles::form_row(ui, "强制 SSL", label_width, |ui| {
                        ui.checkbox(&mut config.ftp.ftps.require_ssl, "");
                    });
                    ui.horizontal(|ui| {
                        ui.add_sized([label_width, 24.0], egui::Label::new(""));
                        ui.label(
                            RichText::new("启用后将拒绝非加密连接")
                                .size(styles::FONT_SIZE_SM)
                                .color(styles::TEXT_MUTED_COLOR)
                                .italics(),
                        );
                    });

                    ui.add_space(styles::SPACING_SM);

                    styles::form_row(ui, "隐式 SSL", label_width, |ui| {
                        ui.checkbox(&mut config.ftp.ftps.implicit_ssl, "");
                    });
                    ui.horizontal(|ui| {
                        ui.add_sized([label_width, 24.0], egui::Label::new(""));
                        ui.label(
                            RichText::new(
                                "隐式 SSL 需要专用端口 (默认 990)，客户端无需发送 AUTH 命令",
                            )
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                        );
                    });

                    if config.ftp.ftps.implicit_ssl {
                        styles::form_row(ui, "隐式 SSL 端口", label_width, |ui| {
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
                        });
                    }

                    ui.add_space(styles::SPACING_SM);

                    let mut cert_path = config.ftp.ftps.cert_path.clone().unwrap_or_default();
                    styles::form_row(ui, "证书文件", label_width, |ui| {
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
                        if ui.button("浏览...").clicked()
                            && let Some(path) = Self::pick_cert_file("选择 FTPS 证书文件")
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
                            ("√ 证书文件已存在", styles::SUCCESS_COLOR)
                        } else {
                            ("⚠ 证书文件不存在", styles::DANGER_COLOR)
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
                                RichText::new("⚠ 证书文件未配置")
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::WARNING_COLOR)
                                    .italics(),
                            );
                        });
                    }

                    ui.add_space(styles::SPACING_SM);

                    let mut key_path = config.ftp.ftps.key_path.clone().unwrap_or_default();
                    styles::form_row(ui, "私钥文件", label_width, |ui| {
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
                        if ui.button("浏览...").clicked()
                            && let Some(path) = Self::pick_key_file("选择 FTPS 私钥文件")
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
                            ("√ 私钥文件已存在", styles::SUCCESS_COLOR)
                        } else {
                            ("⚠ 私钥文件不存在", styles::DANGER_COLOR)
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
                                RichText::new("⚠ 私钥文件未配置")
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
                Self::section_header(ui, "🔐", "SFTP 设置");

                ui.checkbox(
                    &mut config.sftp.enabled,
                    RichText::new("启用 SFTP 服务").size(styles::FONT_SIZE_MD),
                );
                ui.add_space(styles::SPACING_MD);

                let available_width = ui.available_width();
                let label_width = (available_width * 0.15).clamp(100.0, 160.0);

                styles::form_row(ui, "绑定 IP", label_width, |ui| {
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
                    "SFTP 端口",
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
                    "(建议 2222)",
                );

                let mut host_key_path = config.sftp.host_key_path.clone();
                styles::form_row(ui, "主机密钥路径", label_width, |ui| {
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
                    if ui.button("浏览...").clicked()
                        && let Some(path) = Self::pick_file("选择 SFTP 主机密钥文件")
                    {
                        host_key_path = path.to_string_lossy().to_string();
                    }
                });
                config.sftp.host_key_path = host_key_path;

                let host_key_exists =
                    std::path::Path::new(config.sftp.host_key_path.trim()).exists();
                let host_key_status = if host_key_exists {
                    ("√ 文件已存在", styles::SUCCESS_COLOR)
                } else {
                    ("ℹ 文件不存在，启动时将自动生成", styles::TEXT_MUTED_COLOR)
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

                styles::form_row(ui, "最大认证次数", label_width, |ui| {
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
                });

                styles::form_row_with_suffix(
                    ui,
                    "认证超时",
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
                    "秒",
                );

                styles::form_row(ui, "日志级别", label_width, |ui| {
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
                    RichText::new("安全增强")
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR)
                        .strong(),
                );
                ui.add_space(styles::SPACING_SM);

                styles::form_row(ui, "单用户最大会话数", label_width, |ui| {
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
                });

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new("限制单个用户同时连接的会话数量")
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });
            });

            ui.add_space(styles::SPACING_MD);

            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                Self::section_header(ui, "📋", "全局日志设置");

                let available_width = ui.available_width();
                let label_width = (available_width * 0.15).clamp(100.0, 160.0);

                let mut log_dir = config.logging.log_dir.clone();
                styles::form_row(ui, "日志目录", label_width, |ui| {
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
                    if ui.button("浏览...").clicked()
                        && let Some(path) = Self::pick_folder("选择日志目录")
                    {
                        log_dir = path.to_string_lossy().to_string();
                    }
                });
                config.logging.log_dir = log_dir;

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(
                            "系统日志和文件操作日志将分别保存为 wftpg-*.log 和 file-ops-*.log",
                        )
                        .size(styles::FONT_SIZE_SM)
                        .color(styles::TEXT_MUTED_COLOR)
                        .italics(),
                    );
                });

                styles::form_row(ui, "日志级别", label_width, |ui| {
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
                        RichText::new("日志级别越低，记录的日志越详细")
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });

                styles::form_row_with_suffix(
                    ui,
                    "单个日志文件最大大小",
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
                    "最大日志文件数",
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
                    "个 (自动清理过期日志)",
                );

                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new("说明")
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
                                RichText::new("• 日志将自动按日期滚动（每天一个文件）")
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                            ui.label(
                                RichText::new("• 超过最大文件数的旧日志会被自动清理")
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                            ui.label(
                                RichText::new("• 系统日志：wftpg-YYYY-MM-DD.log")
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                            ui.label(
                                RichText::new("• 文件操作日志：file-ops-YYYY-MM-DD.log")
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
