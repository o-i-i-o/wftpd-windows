use egui::{Color32, RichText, Ui};
use crate::core::ipc::{IpcClient, LogEntryDto};
use crate::gui_egui::styles;

pub struct LogTab {
    logs: Vec<LogEntryDto>,
    auto_refresh: bool,
    fetch_count: usize,
    last_error: Option<String>,
}

impl Default for LogTab {
    fn default() -> Self {
        Self {
            logs: Vec::new(),
            auto_refresh: true,
            fetch_count: 200,
            last_error: None,
        }
    }
}

impl LogTab {
    pub fn new() -> Self {
        let mut tab = Self::default();
        tab.refresh();
        tab
    }

    fn refresh(&mut self) {
        if !IpcClient::is_server_running() {
            self.last_error = Some("后台服务未运行，无法获取日志".to_string());
            return;
        }
        
        match IpcClient::get_logs(self.fetch_count) {
            Ok(resp) => {
                if let Some(logs) = resp.logs {
                    self.logs = logs;
                }
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("获取日志失败：{}", e));
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("📋").size(styles::FONT_SIZE_XL));
            ui.label(RichText::new("系统日志").size(styles::FONT_SIZE_XL).strong().color(styles::TEXT_PRIMARY_COLOR));
        });
        ui.add_space(styles::SPACING_SM);

        egui::Grid::new("log_toolbar_grid")
            .num_columns(5)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                if ui.button("🔄 刷新").clicked() {
                    self.refresh();
                }
                ui.checkbox(&mut self.auto_refresh, "自动刷新");
                
                ui.label(RichText::new("显示条数:").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
                let mut count_str = self.fetch_count.to_string();
                styles::input_frame().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut count_str)
                        .desired_width(60.0)
                        .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)));
                });
                if let Ok(v) = count_str.parse::<usize>() {
                    self.fetch_count = v.clamp(1, 10_000);
                }
                
                ui.label(RichText::new(format!("共 {} 条日志", self.logs.len()))
                    .size(styles::FONT_SIZE_SM).color(styles::TEXT_MUTED_COLOR));
                ui.end_row();
            });

        if self.auto_refresh {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(2));
            self.refresh();
        }

        if let Some(err) = &self.last_error {
            styles::info_card_frame(styles::DANGER_LIGHT).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠").size(styles::FONT_SIZE_MD).color(styles::DANGER_COLOR));
                    ui.label(RichText::new(err).size(styles::FONT_SIZE_MD).color(styles::DANGER_COLOR));
                });
            });
            ui.add_space(styles::SPACING_MD);
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.logs.is_empty() {
                    styles::card_frame().show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(styles::SPACING_LG);
                            ui.label(RichText::new("📭 暂无日志记录")
                                .size(styles::FONT_SIZE_LG).color(styles::TEXT_MUTED_COLOR));
                            ui.add_space(styles::SPACING_MD);
                            ui.label(RichText::new("点击刷新按钮获取最新日志")
                                .size(styles::FONT_SIZE_MD).color(styles::TEXT_LABEL_COLOR));
                        });
                    });
                    return;
                }

                styles::card_frame().show(ui, |ui| {
                    egui::Grid::new("log_header_grid")
                        .num_columns(5)
                        .spacing([8.0, 4.0])
                        .min_col_width(60.0)
                        .show(ui, |ui| {
                            ui.add_sized([140.0, 16.0], egui::Label::new(RichText::new("时间").strong().size(styles::FONT_SIZE_SM).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([60.0, 16.0], egui::Label::new(RichText::new("级别").strong().size(styles::FONT_SIZE_SM).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([80.0, 16.0], egui::Label::new(RichText::new("来源").strong().size(styles::FONT_SIZE_SM).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([110.0, 16.0], egui::Label::new(RichText::new("客户端 IP").strong().size(styles::FONT_SIZE_SM).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.label(RichText::new("消息详情").strong().size(styles::FONT_SIZE_SM).color(styles::TEXT_PRIMARY_COLOR));
                            ui.end_row();
                        });
                });

                ui.add_space(styles::SPACING_XS);

                egui::ScrollArea::horizontal().show(ui, |ui| {
                    for (idx, entry) in self.logs.iter().enumerate() {
                        let bg_color = if idx % 2 == 0 { 
                            Color32::WHITE 
                        } else { 
                            Color32::from_rgb(252, 253, 254)
                        };
                        
                        egui::Frame::new()
                            .fill(bg_color)
                            .stroke(egui::Stroke::new(1.0, styles::BORDER_LIGHT))
                            .inner_margin(egui::Margin { left: 8, right: 8, top: 5, bottom: 5 })
                            .corner_radius(egui::CornerRadius::same(4))
                            .show(ui, |ui| {
                                egui::Grid::new(format!("log_row_{}", idx))
                                    .num_columns(5)
                                    .spacing([8.0, 4.0])
                                    .min_col_width(60.0)
                                    .show(ui, |ui| {
                                        ui.add_sized([140.0, 20.0], egui::Label::new(
                                            RichText::new(&entry.timestamp)
                                                .size(styles::FONT_SIZE_SM)
                                                .color(styles::TEXT_SECONDARY_COLOR),
                                        ));

                                        let level_color = match entry.level.as_str() {
                                            "ERROR" => styles::DANGER_COLOR,
                                            "WARN" => styles::WARNING_COLOR,
                                            "DEBUG" => styles::TEXT_MUTED_COLOR,
                                            _ => styles::SUCCESS_COLOR,
                                        };
                                        ui.add_sized([60.0, 20.0], egui::Label::new(
                                            RichText::new(&entry.level)
                                                .size(styles::FONT_SIZE_SM)
                                                .strong()
                                                .color(level_color)
                                        ));

                                        ui.add_sized([80.0, 20.0], egui::Label::new(
                                            RichText::new(&entry.source)
                                                .size(styles::FONT_SIZE_SM)
                                                .color(styles::TEXT_SECONDARY_COLOR)
                                        ));

                                        ui.add_sized([110.0, 20.0], egui::Label::new(
                                            RichText::new(
                                                entry.client_ip.as_deref().unwrap_or("-"),
                                            )
                                            .size(styles::FONT_SIZE_SM)
                                            .color(styles::TEXT_LABEL_COLOR),
                                        ));

                                        let msg = if let Some(user) = &entry.username {
                                            format!("[{}] {}", user, entry.message)
                                        } else {
                                            entry.message.clone()
                                        };
                                        ui.label(RichText::new(&msg).size(styles::FONT_SIZE_SM).color(styles::TEXT_PRIMARY_COLOR));
                                        ui.end_row();
                                    });
                            });
                    }
                });
                
                ui.add_space(styles::SPACING_MD);
            });
    }
}
