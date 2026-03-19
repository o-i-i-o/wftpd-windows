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
            Err(e) => self.status_message = Some((format!("保存失败：{}", e), false)),
        }
    }

    fn section_header(&self, ui: &mut Ui, icon: &str, title: &str) {
        styles::section_header(ui, icon, title);
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        styles::page_header(ui, "🔒", "安全设置");

        if let Some((msg, success)) = &self.status_message.clone() {
            styles::status_message(ui, msg, *success);
            ui.add_space(styles::SPACING_MD);
        }

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.section_header(ui, "🔐", "登录安全");
            
            let available_width = ui.available_width();
            let label_width = (available_width * 0.2).clamp(100.0, 160.0);
            
            styles::form_row(ui, "最大登录失败次数", label_width, |ui| {
                let mut val = self.config.security.max_login_attempts.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut val)
                        .desired_width(100.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(v) = val.parse::<u32>() {
                    self.config.security.max_login_attempts = v;
                }
            });
            
            styles::form_row_with_suffix(ui, "封禁时间", label_width, |ui| {
                let mut val = self.config.security.ban_duration.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut val)
                        .desired_width(100.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(v) = val.parse::<u64>() {
                    self.config.security.ban_duration = v;
                }
            }, "秒");
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.section_header(ui, "🔑", "SFTP 主机密钥");
            
            let available_width = ui.available_width();
            let label_width = (available_width * 0.2).clamp(100.0, 160.0);
            
            styles::form_row(ui, "密钥文件路径", label_width, |ui| {
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.config.sftp.host_key_path)
                        .desired_width(ui.available_width())
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
            });
            
            ui.horizontal(|ui| {
                ui.add_sized([label_width, 24.0], egui::Label::new(""));
                ui.label(RichText::new("ℹ 密钥文件不存在时将在启动时自动生成")
                    .size(styles::FONT_SIZE_SM)
                    .color(styles::TEXT_MUTED_COLOR)
                    .italics());
            });
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.section_header(ui, "🌐", "IP 访问控制");
            
            ui.label(RichText::new("允许的 IP/CIDR").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
            ui.label(RichText::new("每行一个，0.0.0.0/0 表示允许全部")
                .size(styles::FONT_SIZE_SM)
                .color(styles::TEXT_MUTED_COLOR));
            
            styles::input_frame().show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.allowed_ips_text)
                    .desired_rows(4)
                    .desired_width(ui.available_width())
                    .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
            });

            ui.add_space(styles::SPACING_MD);
            
            ui.label(RichText::new("拒绝的 IP/CIDR").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
            ui.label(RichText::new("每行一个，优先级高于允许列表")
                .size(styles::FONT_SIZE_SM)
                .color(styles::TEXT_MUTED_COLOR));
            
            styles::input_frame().show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.denied_ips_text)
                    .desired_rows(4)
                    .desired_width(ui.available_width())
                    .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
            });
        });

        ui.add_space(styles::SPACING_MD);

        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(styles::primary_button("💾 保存安全配置")).clicked() {
                    self.save();
                }
            });
        });
    }
}
