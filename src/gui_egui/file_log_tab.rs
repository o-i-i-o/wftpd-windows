use egui::{Color32, RichText, Ui};
use crate::core::ipc::{IpcClient, FileLogEntryDto};
use crate::gui_egui::styles;

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
        ui.heading(RichText::new("📁 文件操作日志").color(styles::TEXT_PRIMARY_COLOR));
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("🔄 刷新").clicked() {
                self.refresh();
            }
            ui.checkbox(&mut self.auto_refresh, "自动刷新");
            ui.label("显示条数:");
            let mut count_str = self.fetch_count.to_string();
            if ui
                .add(egui::TextEdit::singleline(&mut count_str).desired_width(60.0))
                .changed()
                && let Ok(v) = count_str.parse::<usize>() {
                    self.fetch_count = v.clamp(1, 10_000);
                }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("共 {} 条记录", self.logs.len()))
                    .size(11.0).color(Color32::from_rgb(120, 120, 120)));
            });
        });

        if self.auto_refresh {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(2));
            self.refresh();
        }

        if let Some(err) = &self.last_error {
            egui::Frame::new()
                .fill(Color32::from_rgb(253, 230, 230))
                .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 })
                .corner_radius(egui::CornerRadius::same(6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("⚠").size(16.0).color(Color32::from_rgb(220, 38, 38)));
                        ui.label(RichText::new(err).color(Color32::from_rgb(185, 28, 28)));
                    });
                });
            ui.add_space(8.0);
        }

        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.logs.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label(RichText::new("📭 暂无文件操作记录")
                            .size(14.0).color(Color32::from_rgb(150, 150, 150)));
                        ui.add_space(8.0);
                        ui.label(RichText::new("用户进行文件操作时会在这里显示记录")
                            .size(12.0).color(Color32::from_rgb(180, 180, 180)));
                    });
                    return;
                }

                egui::Frame::new()
                    .fill(Color32::from_rgb(248, 249, 250))
                    .inner_margin(egui::Margin { left: 8, right: 8, top: 6, bottom: 6 })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_sized([140.0, 16.0], egui::Label::new(RichText::new("时间").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([80.0, 16.0], egui::Label::new(RichText::new("用户").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([110.0, 16.0], egui::Label::new(RichText::new("客户端 IP").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([60.0, 16.0], egui::Label::new(RichText::new("协议").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([80.0, 16.0], egui::Label::new(RichText::new("操作").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([70.0, 16.0], egui::Label::new(RichText::new("大小").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.label(RichText::new("文件路径").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR));
                        });
                    });

                egui::ScrollArea::horizontal().show(ui, |ui| {
                    for (idx, entry) in self.logs.iter().enumerate() {
                        let bg_color = if idx % 2 == 0 { 
                            Color32::WHITE 
                        } else { 
                            Color32::from_rgb(252, 253, 254)
                        };
                        
                        egui::Frame::new()
                            .fill(bg_color)
                            .inner_margin(egui::Margin { left: 8, right: 8, top: 5, bottom: 5 })
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.add_sized([140.0, 20.0], egui::Label::new(
                                        RichText::new(&entry.timestamp)
                                            .size(12.0)
                                            .color(Color32::from_rgb(90, 90, 90)),
                                    ));
                                    
                                    ui.add_sized([80.0, 20.0], egui::Label::new(
                                        RichText::new(&entry.username)
                                            .size(12.0)
                                            .color(Color32::from_rgb(60, 60, 60))
                                    ));
                                    
                                    ui.add_sized([110.0, 20.0], egui::Label::new(
                                        RichText::new(&entry.client_ip)
                                            .size(12.0)
                                            .color(Color32::from_rgb(100, 100, 100)),
                                    ));
                                    
                                    let protocol_color = match entry.protocol.as_str() {
                                        "FTP" => Color32::from_rgb(108, 92, 231),
                                        "SFTP" => Color32::from_rgb(41, 128, 185),
                                        _ => Color32::from_rgb(100, 100, 100),
                                    };
                                    ui.add_sized([60.0, 20.0], egui::Label::new(
                                        RichText::new(&entry.protocol)
                                            .size(11.0)
                                            .strong()
                                            .color(protocol_color)
                                    ));

                                    let (_op_bg, op_color) = match entry.operation.as_str() {
                                        "DELETE" | "RMDIR" => (Color32::from_rgb(254, 226, 226), Color32::from_rgb(185, 28, 28)),
                                        "UPLOAD" | "MKDIR" => (Color32::from_rgb(220, 252, 231), Color32::from_rgb(16, 124, 16)),
                                        "DOWNLOAD" => (Color32::from_rgb(225, 243, 252), Color32::from_rgb(8, 102, 135)),
                                        "RENAME" | "COPY" => (Color32::from_rgb(254, 249, 195), Color32::from_rgb(161, 98, 7)),
                                        "UPDATE" => (Color32::from_rgb(240, 244, 248), Color32::from_rgb(82, 95, 107)),
                                        _ => (Color32::WHITE, Color32::from_rgb(100, 100, 100)),
                                    };
                                    let status_icon = if entry.success { "✓" } else { "✗" };
                                    ui.add_sized([80.0, 20.0], egui::Label::new(
                                        RichText::new(format!("{} {}", status_icon, entry.operation))
                                            .size(11.0)
                                            .strong()
                                            .color(op_color)
                                    ));

                                    let size_str = if entry.file_size > 0 {
                                        format_size(entry.file_size)
                                    } else {
                                        "-".to_string()
                                    };
                                    ui.add_sized([70.0, 20.0], egui::Label::new(
                                        RichText::new(&size_str)
                                            .size(12.0)
                                            .color(Color32::from_rgb(100, 100, 100)),
                                    ));

                                    ui.label(RichText::new(&entry.file_path)
                                        .size(12.0)
                                        .color(Color32::from_rgb(50, 50, 50)));
                                });
                            });
                    }
                });
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
