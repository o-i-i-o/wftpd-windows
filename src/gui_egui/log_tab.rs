use egui::{RichText, Ui};
use crate::core::ipc::{IpcClient, LogEntryDto};
use crate::gui_egui::styles;
use egui_extras::TableBuilder;

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
        styles::page_header(ui, "📋", "系统日志");

        ui.horizontal(|ui| {
            if ui.add(styles::small_button("🔄 刷新")).clicked() {
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
            
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("共 {} 条日志", self.logs.len()))
                    .size(styles::FONT_SIZE_SM).color(styles::TEXT_MUTED_COLOR));
            });
        });

        if self.auto_refresh {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(2));
            self.refresh();
        }

        if let Some(err) = &self.last_error {
            styles::status_message(ui, err, false);
            ui.add_space(styles::SPACING_MD);
        }

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            
            if self.logs.is_empty() {
                styles::empty_state(ui, "📭", "暂无日志记录", "点击刷新按钮获取最新日志");
                return;
            }

            let available_width = ui.available_width();
            let table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(styles::table_column_percent(available_width, 0.16, 130.0))
                .column(styles::table_column_percent(available_width, 0.07, 65.0))
                .column(styles::table_column_percent(available_width, 0.10, 80.0))
                .column(styles::table_column_percent(available_width, 0.12, 100.0))
                .column(styles::table_column_remainder(280.0))
                .min_scrolled_height(0.0)
                .sense(egui::Sense::hover());

            table
                .header(styles::FONT_SIZE_MD, |mut header| {
                    header.col(|ui| {
                        ui.label(RichText::new("时间").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("级别").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("来源").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("客户端").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("消息详情").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                })
                .body(|mut body| {
                    for entry in &self.logs {
                        body.row(styles::FONT_SIZE_MD, |mut row| {
                            row.col(|ui| {
                                ui.label(RichText::new(&entry.timestamp)
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_SECONDARY_COLOR));
                            });
                            row.col(|ui| {
                                let level_color = match entry.level.as_str() {
                                    "ERROR" => styles::DANGER_COLOR,
                                    "WARN" => styles::WARNING_COLOR,
                                    "DEBUG" => styles::TEXT_MUTED_COLOR,
                                    _ => styles::SUCCESS_COLOR,
                                };
                                ui.label(RichText::new(&entry.level)
                                    .size(styles::FONT_SIZE_SM)
                                    .strong()
                                    .color(level_color));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(&entry.source)
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_SECONDARY_COLOR));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(
                                    entry.client_ip.as_deref().unwrap_or("-"),
                                )
                                .size(styles::FONT_SIZE_SM)
                                .color(styles::TEXT_LABEL_COLOR));
                            });
                            row.col(|ui| {
                                let msg = if let Some(user) = &entry.username {
                                    format!("[{}] {}", user, entry.message)
                                } else {
                                    entry.message.clone()
                                };
                                ui.label(RichText::new(&msg).size(styles::FONT_SIZE_SM).color(styles::TEXT_PRIMARY_COLOR));
                            });
                        });
                    }
                });
        });
    }
}
