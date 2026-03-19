use egui::{RichText, Ui};
use crate::core::config::Config;
use crate::gui_egui::styles;

pub struct SecurityTab {
    config: Config,
    allowed_ips_text: String,
    denied_ips_text: String,
    status_message: Option<(String, bool)>,
}

impl Default for SecurityTab {
    fn default() -> Self {
        let config = Config::load(&Config::get_config_path()).unwrap_or_default();
        let allowed_ips_text = config.security.allowed_ips.join("\n");
        let denied_ips_text = config.security.denied_ips.join("\n");
        Self {
            config,
            allowed_ips_text,
            denied_ips_text,
            status_message: None,
        }
    }
}

impl SecurityTab {
    pub fn new() -> Self { Self::default() }

    fn save(&mut self) {
        self.config.security.allowed_ips = self
            .allowed_ips_text
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        self.config.security.denied_ips = self
            .denied_ips_text
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        match self.config.save(&Config::get_config_path()) {
            Ok(_) => self.status_message = Some(("安全配置已保存".into(), true)),
            Err(e) => self.status_message = Some((format!("保存失败: {}", e), false)),
        }
    }

    fn section_header(&self, ui: &mut Ui, icon: &str, title: &str) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon).size(styles::FONT_SIZE_LG));
            ui.label(RichText::new(title).size(styles::FONT_SIZE_LG).strong().color(styles::TEXT_PRIMARY_COLOR));
        });
        ui.add_space(styles::SPACING_SM);
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("🔒").size(styles::FONT_SIZE_XL));
            ui.label(RichText::new("安全设置").size(styles::FONT_SIZE_XL).strong().color(styles::TEXT_PRIMARY_COLOR));
        });
        ui.add_space(styles::SPACING_SM);

        if let Some((msg, success)) = &self.status_message.clone() {
            let (bg_color, text_color) = if *success {
                (styles::SUCCESS_LIGHT, styles::SUCCESS_COLOR)
            } else {
                (styles::DANGER_LIGHT, styles::DANGER_COLOR)
            };
            
            styles::info_card_frame(bg_color).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let icon = if *success { "✓" } else { "✗" };
                    ui.label(RichText::new(icon).size(styles::FONT_SIZE_MD).color(text_color));
                    ui.label(RichText::new(msg).size(styles::FONT_SIZE_MD).color(text_color));
                });
            });
            ui.add_space(styles::SPACING_MD);
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                styles::card_frame().show(ui, |ui| {
                    self.section_header(ui, "🔐", "登录安全");
                    
                    egui::Grid::new("login_security_grid")
                        .num_columns(2)
                        .spacing([16.0, 8.0])
                        .min_col_width(140.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new("最大登录失败次数:").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
                            let mut val = self.config.security.max_login_attempts.to_string();
                            styles::input_frame().show(ui, |ui| {
                                ui.add(egui::TextEdit::singleline(&mut val)
                                    .desired_width(100.0)
                                    .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                            });
                            if let Ok(v) = val.parse::<u32>() {
                                self.config.security.max_login_attempts = v;
                            }
                            ui.end_row();

                            ui.label(RichText::new("封禁时间(秒):").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
                            let mut val = self.config.security.ban_duration.to_string();
                            styles::input_frame().show(ui, |ui| {
                                ui.add(egui::TextEdit::singleline(&mut val)
                                    .desired_width(100.0)
                                    .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                            });
                            if let Ok(v) = val.parse::<u64>() {
                                self.config.security.ban_duration = v;
                            }
                            ui.end_row();
                        });
                });

                ui.add_space(styles::SPACING_LG);

                styles::card_frame().show(ui, |ui| {
                    self.section_header(ui, "🔑", "SFTP 主机密钥");
                    
                    egui::Grid::new("host_key_grid")
                        .num_columns(2)
                        .spacing([16.0, 8.0])
                        .min_col_width(140.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new("密钥文件路径:").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
                            styles::input_frame().show(ui, |ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.config.sftp.host_key_path)
                                    .desired_width(300.0)
                                    .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                            });
                            ui.end_row();

                            ui.label(RichText::new("").size(styles::FONT_SIZE_MD));
                            ui.label(RichText::new("ℹ 密钥文件不存在时将在启动时自动生成")
                                .size(styles::FONT_SIZE_SM)
                                .color(styles::TEXT_MUTED_COLOR)
                                .italics());
                            ui.end_row();
                        });
                });

                ui.add_space(styles::SPACING_LG);

                styles::card_frame().show(ui, |ui| {
                    self.section_header(ui, "🌐", "IP 访问控制");
                    
                    egui::Grid::new("ip_access_grid")
                        .num_columns(1)
                        .spacing([16.0, 8.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("允许的 IP/CIDR").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
                            ui.label(RichText::new("每行一个，0.0.0.0/0 表示允许全部")
                                .size(styles::FONT_SIZE_SM)
                                .color(styles::TEXT_MUTED_COLOR));
                            ui.end_row();
                            
                            styles::input_frame().show(ui, |ui| {
                                ui.add(egui::TextEdit::multiline(&mut self.allowed_ips_text)
                                    .desired_rows(4)
                                    .desired_width(f32::INFINITY)
                                    .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                            });
                            ui.end_row();

                            ui.label(RichText::new("拒绝的 IP/CIDR").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
                            ui.label(RichText::new("每行一个，优先级高于允许列表")
                                .size(styles::FONT_SIZE_SM)
                                .color(styles::TEXT_MUTED_COLOR));
                            ui.end_row();
                            
                            styles::input_frame().show(ui, |ui| {
                                ui.add(egui::TextEdit::multiline(&mut self.denied_ips_text)
                                    .desired_rows(4)
                                    .desired_width(f32::INFINITY)
                                    .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                            });
                            ui.end_row();
                        });
                });

                ui.add_space(styles::SPACING_LG);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(styles::primary_button("💾 保存安全配置")).clicked() {
                            self.save();
                        }
                    });
                });
                
                ui.add_space(styles::SPACING_MD);
            });
    }
}
