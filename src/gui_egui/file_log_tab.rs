use egui::{RichText, Ui};
use crate::core::ipc::{IpcClient, FileLogEntryDto};
use crate::gui_egui::styles;
use egui_extras::TableBuilder;

pub struct FileLogTab {
    logs: Vec<FileLogEntryDto>,
    auto_refresh: bool,
    fetch_count: usize,
    last_error: Option<String>,
}

impl Default for FileLogTab {
    fn default() -> Self {
        Self {
            logs: Vec::new(),
            auto_refresh: true,
            fetch_count: 200,
            last_error: None,
        }
    }
}

impl FileLogTab {
    pub fn new() -> Self {
        let mut tab = Self::default();
        tab.refresh();
        tab
    }

    fn refresh(&mut self) {
        if !IpcClient::is_server_running() {
            self.last_error = Some("后台服务未运行，无法获取文件日志".to_string());
            return;
        }
        
        match IpcClient::get_file_logs(self.fetch_count) {
            Ok(resp) => {
                if let Some(logs) = resp.file_logs {
                    self.logs = logs;
                }
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("获取文件日志失败：{}", e));
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        styles::page_header(ui, "📁", "文件操作日志");

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
                ui.label(RichText::new(format!("共 {} 条记录", self.logs.len()))
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

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.logs.is_empty() {
                    styles::empty_state(ui, "📭", "暂无文件操作记录", "用户进行文件操作时会在这里显示记录");
                    return;
                }

                let available_width = ui.available_width();
                let table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(styles::table_column_percent(available_width, 0.12, 110.0))
                    .column(styles::table_column_percent(available_width, 0.08, 70.0))
                    .column(styles::table_column_percent(available_width, 0.10, 90.0))
                    .column(styles::table_column_percent(available_width, 0.06, 60.0))
                    .column(styles::table_column_percent(available_width, 0.10, 80.0))
                    .column(styles::table_column_percent(available_width, 0.08, 70.0))
                    .column(styles::table_column_remainder(250.0))
                    .min_scrolled_height(0.0);

                table
                    .header(styles::FONT_SIZE_MD, |mut header| {
                        header.col(|ui| {
                            ui.strong("时间");
                        });
                        header.col(|ui| {
                            ui.strong("用户");
                        });
                        header.col(|ui| {
                            ui.strong("客户端");
                        });
                        header.col(|ui| {
                            ui.strong("协议");
                        });
                        header.col(|ui| {
                            ui.strong("操作");
                        });
                        header.col(|ui| {
                            ui.strong("大小");
                        });
                        header.col(|ui| {
                            ui.strong("文件路径");
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
                                    ui.label(RichText::new(&entry.username)
                                        .size(styles::FONT_SIZE_SM)
                                        .color(styles::TEXT_PRIMARY_COLOR));
                                });
                                row.col(|ui| {
                                    ui.label(RichText::new(&entry.client_ip)
                                        .size(styles::FONT_SIZE_SM)
                                        .color(styles::TEXT_LABEL_COLOR));
                                });
                                row.col(|ui| {
                                    let protocol_color = match entry.protocol.as_str() {
                                        "FTP" => styles::PRIMARY_COLOR,
                                        "SFTP" => styles::INFO_COLOR,
                                        _ => styles::TEXT_MUTED_COLOR,
                                    };
                                    ui.label(RichText::new(&entry.protocol)
                                        .size(styles::FONT_SIZE_SM)
                                        .strong()
                                        .color(protocol_color));
                                });
                                row.col(|ui| {
                                    let op_color = match entry.operation.as_str() {
                                        "DELETE" | "RMDIR" => styles::DANGER_COLOR,
                                        "UPLOAD" | "MKDIR" => styles::SUCCESS_COLOR,
                                        "DOWNLOAD" => styles::INFO_COLOR,
                                        "RENAME" | "COPY" => styles::WARNING_COLOR,
                                        "UPDATE" => styles::TEXT_MUTED_COLOR,
                                        _ => styles::TEXT_LABEL_COLOR,
                                    };
                                    let status_icon = if entry.success { "✓" } else { "✗" };
                                    ui.label(RichText::new(format!("{} {}", status_icon, entry.operation))
                                        .size(styles::FONT_SIZE_SM)
                                        .strong()
                                        .color(op_color));
                                });
                                row.col(|ui| {
                                    let size_str = if entry.file_size > 0 {
                                        format_size(entry.file_size)
                                    } else {
                                        "-".to_string()
                                    };
                                    ui.label(RichText::new(&size_str)
                                        .size(styles::FONT_SIZE_SM)
                                        .color(styles::TEXT_LABEL_COLOR));
                                });
                                row.col(|ui| {
                                    ui.label(RichText::new(&entry.file_path)
                                        .size(styles::FONT_SIZE_SM)
                                        .color(styles::TEXT_PRIMARY_COLOR));
                                });
                            });
                        }
                    });
                
                ui.add_space(styles::SPACING_MD);
            });
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
