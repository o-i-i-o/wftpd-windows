use egui::{RichText, Ui};
use crate::core::config::Config;
use crate::core::ipc::IpcClient;
use crate::gui_egui::styles;
use egui_file_dialog::FileDialog;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FileDialogTarget {
    #[default]
    None,
    AnonymousHome,
}

#[derive(Debug)]
pub struct ServerTab {
    pub config: Config,
    pub ftp_running: bool,
    pub sftp_running: bool,
    pub status_message: Option<(String, bool)>,
    file_dialog: FileDialog,
    file_dialog_target: FileDialogTarget,
}

impl Default for ServerTab {
    fn default() -> Self {
        let config = Config::load(&Config::get_config_path()).unwrap_or_default();
        Self {
            config,
            ftp_running: false,
            sftp_running: false,
            status_message: None,
            file_dialog: FileDialog::new().title("选择匿名用户目录"),
            file_dialog_target: FileDialogTarget::None,
        }
    }
}

impl ServerTab {
    pub fn new() -> Self { Self::default() }

    pub fn update_status(&mut self, ftp_running: bool, sftp_running: bool) {
        self.ftp_running = ftp_running;
        self.sftp_running = sftp_running;
    }

    pub fn save_config(&mut self) -> Result<(), anyhow::Error> {
        self.config.save(&Config::get_config_path())?;
        if IpcClient::is_server_running() {
            let _ = IpcClient::send_command(crate::core::ipc::Command {
                action: "reload".to_string(),
                service: None,
                data: None,
            });
        }
        Ok(())
    }

    fn section_header(&self, ui: &mut Ui, icon: &str, title: &str) {
        styles::section_header(ui, icon, title);
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("⚙").size(styles::FONT_SIZE_XL));
            ui.label(RichText::new("服务器配置").size(styles::FONT_SIZE_XL).strong().color(styles::TEXT_PRIMARY_COLOR));
            
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(styles::primary_button("💾 保存配置")).clicked() {
                    match self.save_config() {
                        Ok(_) => {
                            self.status_message = Some(("配置已保存".to_string(), true));
                        }
                        Err(e) => {
                            self.status_message = Some((format!("保存失败：{}", e), false));
                        }
                    }
                }
                
                if let Some((msg, success)) = &self.status_message.clone() {
                    let msg_text = if *success {
                        RichText::new(format!("✓ {}", msg)).color(styles::SUCCESS_COLOR).size(styles::FONT_SIZE_SM)
                    } else {
                        RichText::new(format!("✗ {}", msg)).color(styles::DANGER_COLOR).size(styles::FONT_SIZE_SM)
                    };
                    ui.label(msg_text);
                }
            });
        });
        
        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.section_header(ui, "📡", "FTP 设置");
            
            ui.checkbox(&mut self.config.ftp.enabled, 
                RichText::new("启用 FTP 服务").size(styles::FONT_SIZE_MD));
            ui.add_space(styles::SPACING_MD);

            let available_width = ui.available_width();
            let label_width = (available_width * 0.15).clamp(100.0, 160.0);
            
            styles::form_row(ui, "绑定 IP", label_width, |ui| {
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.config.server.bind_ip)
                        .desired_width(ui.available_width())
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
            });
            
            styles::form_row(ui, "FTP 端口", label_width, |ui| {
                let mut port_str = self.config.server.ftp_port.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut port_str)
                        .desired_width(80.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(p) = port_str.parse::<u16>() {
                    self.config.server.ftp_port = p;
                }
            });
            
            styles::form_row(ui, "欢迎消息", label_width, |ui| {
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.config.ftp.welcome_message)
                        .desired_width(ui.available_width())
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
            });
            
            styles::form_row(ui, "编码", label_width, |ui| {
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.config.ftp.encoding)
                        .desired_width(100.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
            });
            
            styles::form_row(ui, "默认传输模式", label_width, |ui| {
                let modes = ["binary", "ascii"];
                egui::ComboBox::from_id_salt("transfer_mode")
                    .selected_text(&self.config.ftp.default_transfer_mode)
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for mode in modes {
                            ui.selectable_value(
                                &mut self.config.ftp.default_transfer_mode,
                                mode.to_string(),
                                mode
                            );
                        }
                    });
                ui.label(RichText::new("(binary: 二进制，ascii: 文本)").size(styles::FONT_SIZE_SM).color(styles::TEXT_MUTED_COLOR));
            });
            
            styles::form_row(ui, "默认连接模式", label_width, |ui| {
                let passive_label = if self.config.ftp.default_passive_mode { "被动模式 (PASV)" } else { "主动模式 (PORT)" };
                egui::ComboBox::from_id_salt("connection_mode")
                    .selected_text(passive_label)
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.config.ftp.default_passive_mode, true, "被动模式 (PASV)");
                        ui.selectable_value(&mut self.config.ftp.default_passive_mode, false, "主动模式 (PORT)");
                    });
                ui.label(RichText::new("(被动模式兼容性更好)").size(styles::FONT_SIZE_SM).color(styles::TEXT_MUTED_COLOR));
            });
            
            styles::form_row(ui, "允许匿名访问", label_width, |ui| {
                ui.checkbox(&mut self.config.ftp.allow_anonymous, "");
            });
            
            if self.config.ftp.allow_anonymous {
                styles::form_row(ui, "匿名目录", label_width, |ui| {
                    let mut anon_home = self.config.ftp.anonymous_home.clone().unwrap_or_default();
                    styles::input_frame().show(ui, |ui| {
                        ui.add(egui::TextEdit::singleline(&mut anon_home)
                            .desired_width(ui.available_width() - 80.0)
                            .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                    });
                    if ui.button("浏览...").clicked() {
                        self.file_dialog_target = FileDialogTarget::AnonymousHome;
                        self.file_dialog.pick_directory();
                    }
                    self.config.ftp.anonymous_home = if anon_home.is_empty() { None } else { Some(anon_home) };
                });
                
                if self.config.ftp.anonymous_home.as_ref().is_none_or(|s| s.trim().is_empty()) {
                    ui.horizontal(|ui| {
                        ui.add_sized([label_width, 24.0], egui::Label::new(""));
                        ui.label(RichText::new("⚠ 匿名用户目录未配置，匿名访问将无法使用").size(styles::FONT_SIZE_SM).color(styles::WARNING_COLOR));
                    });
                }
            }
            
            styles::form_row(ui, "被动端口范围", label_width, |ui| {
                let mut min_str = self.config.ftp.passive_ports.0.to_string();
                let mut max_str = self.config.ftp.passive_ports.1.to_string();
                
                ui.label(RichText::new("从").size(styles::FONT_SIZE_MD).color(styles::TEXT_MUTED_COLOR));
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut min_str)
                        .desired_width(60.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(p) = min_str.parse::<u16>() {
                    self.config.ftp.passive_ports.0 = p;
                }
                
                ui.label(RichText::new("到").size(styles::FONT_SIZE_MD).color(styles::TEXT_MUTED_COLOR));
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut max_str)
                        .desired_width(60.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(p) = max_str.parse::<u16>() {
                    self.config.ftp.passive_ports.1 = p;
                }
                
                if self.config.ftp.passive_ports.0 > self.config.ftp.passive_ports.1 {
                    ui.label(RichText::new("⚠ 起始端口不能大于结束端口").color(styles::DANGER_COLOR).size(styles::FONT_SIZE_SM));
                }
            });
            
            styles::form_row_with_suffix(ui, "最大传输速度", label_width, |ui| {
                let mut speed_str = self.config.ftp.max_speed_kbps.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut speed_str)
                        .desired_width(80.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(v) = speed_str.parse::<u64>() {
                    self.config.ftp.max_speed_kbps = v;
                }
            }, "KB/s (0 表示不限制)");
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.section_header(ui, "🔐", "SFTP 设置");
            
            ui.checkbox(&mut self.config.sftp.enabled, 
                RichText::new("启用 SFTP 服务").size(styles::FONT_SIZE_MD));
            ui.add_space(styles::SPACING_MD);

            let available_width = ui.available_width();
            let label_width = (available_width * 0.15).clamp(100.0, 160.0);
            
            styles::form_row(ui, "绑定 IP", label_width, |ui| {
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.config.sftp.bind_ip)
                        .desired_width(ui.available_width())
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
            });
            
            styles::form_row_with_suffix(ui, "SFTP 端口", label_width, |ui| {
                let mut port_str = self.config.server.sftp_port.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut port_str)
                        .desired_width(80.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(p) = port_str.parse::<u16>() {
                    self.config.server.sftp_port = p;
                }
            }, "(建议 2222)");
            
            styles::form_row(ui, "主机密钥路径", label_width, |ui| {
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.config.sftp.host_key_path)
                        .desired_width(ui.available_width())
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
            });
            
            styles::form_row(ui, "最大认证次数", label_width, |ui| {
                let mut val_str = self.config.sftp.max_auth_attempts.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut val_str)
                        .desired_width(80.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(v) = val_str.parse::<u32>() {
                    self.config.sftp.max_auth_attempts = v;
                }
            });
            
            styles::form_row_with_suffix(ui, "认证超时", label_width, |ui| {
                let mut val_str = self.config.sftp.auth_timeout.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut val_str)
                        .desired_width(100.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(v) = val_str.parse::<u64>() {
                    self.config.sftp.auth_timeout = v;
                }
            }, "秒");
            
            styles::form_row(ui, "日志级别", label_width, |ui| {
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.config.sftp.log_level)
                        .desired_width(120.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
            });
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.section_header(ui, "⚙", "通用设置");
            
            let available_width = ui.available_width();
            let label_width = (available_width * 0.15).clamp(100.0, 160.0);
            
            styles::form_row(ui, "最大连接数", label_width, |ui| {
                let mut val_str = self.config.server.max_connections.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut val_str)
                        .desired_width(80.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(v) = val_str.parse::<usize>() {
                    self.config.server.max_connections = v;
                }
            });
            
            styles::form_row_with_suffix(ui, "连接超时", label_width, |ui| {
                let mut val_str = self.config.server.connection_timeout.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut val_str)
                        .desired_width(80.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(v) = val_str.parse::<u64>() {
                    self.config.server.connection_timeout = v;
                }
            }, "秒");
            
            styles::form_row_with_suffix(ui, "空闲超时", label_width, |ui| {
                let mut val_str = self.config.server.idle_timeout.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut val_str)
                        .desired_width(80.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(v) = val_str.parse::<u64>() {
                    self.config.server.idle_timeout = v;
                }
            }, "(0 表示不限制)");
        });

        self.file_dialog.update(ui.ctx());
        if let Some(path) = self.file_dialog.take_picked() {
            match self.file_dialog_target {
                FileDialogTarget::None => {}
                FileDialogTarget::AnonymousHome => {
                    self.config.ftp.anonymous_home = Some(path.to_string_lossy().to_string());
                }
            }
        }
        if !matches!(self.file_dialog.state(), egui_file_dialog::DialogState::Open) {
            self.file_dialog_target = FileDialogTarget::None;
        }
    }
}
