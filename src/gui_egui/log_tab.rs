use egui::RichText;
use crate::core::config::Config;
use crate::core::logger::LogEntry;
use crate::gui_egui::styles;
use egui_extras::TableBuilder;
use std::time::{Duration, Instant};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

const PAGE_SIZE: usize = 100;
const LOG_THRESHOLD: usize = 500;

pub struct LogTab {
    logs: Vec<LogEntry>,
    auto_refresh: bool,
    fetch_count: usize,
    fetch_count_buf: String,
    last_error: Option<String>,
    loading: bool,
    last_refresh_time: Option<Instant>,
    current_page: usize,
    total_pages: usize,
    log_dir: PathBuf,
}

impl Default for LogTab {
    fn default() -> Self {
        let log_dir = Config::get_config_path()
            .parent()
            .map(|p| p.join("logs"))
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData\\wftpg\\logs"));
        
        Self {
            logs: Vec::new(),
            auto_refresh: true,
            fetch_count: 200,
            fetch_count_buf: "200".to_string(),
            last_error: None,
            loading: false,
            last_refresh_time: None,
            current_page: 1,
            total_pages: 1,
            log_dir,
        }
    }
}

impl LogTab {
    pub fn new() -> Self {
        let mut tab = Self::default();
        tab.load_logs();
        tab
    }

    fn load_logs(&mut self) {
        self.loading = true;
        self.last_error = None;
        
        let log_dir = self.log_dir.clone();
        let count = self.fetch_count;
        
        let mut all_logs = Vec::new();
        
        if let Ok(entries) = fs::read_dir(&log_dir) {
            let mut log_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name().to_string_lossy().starts_with("wftpg")
                    && e.file_name().to_string_lossy().ends_with(".log")
                })
                .collect();
            
            log_files.sort_by(|a, b| {
                b.file_name().cmp(&a.file_name())
            });
            
            for entry in log_files {
                if all_logs.len() >= count {
                    break;
                }
                
                if let Ok(file) = File::open(entry.path()) {
                    let reader = BufReader::new(file);
                    for line in reader.lines() {
                        if all_logs.len() >= count {
                            break;
                        }
                        if let Ok(line) = line
                            && let Ok(log_entry) = serde_json::from_str::<LogEntry>(&line)
                        {
                            if log_entry.fields.operation.is_none() {
                                all_logs.push(log_entry);
                            }
                        }
                    }
                }
            }
        }
        
        all_logs.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        
        self.logs = all_logs;
        self.loading = false;
        self.last_refresh_time = Some(Instant::now());
        self.update_pagination();
    }

    fn request_refresh(&mut self) {
        if self.loading {
            return;
        }
        self.load_logs();
    }

    fn update_pagination(&mut self) {
        self.total_pages = if self.logs.is_empty() {
            1
        } else {
            self.logs.len().div_ceil(PAGE_SIZE)
        };
        if self.current_page > self.total_pages {
            self.current_page = self.total_pages;
        }
    }

    fn get_page_logs(&self) -> &[LogEntry] {
        let start = (self.current_page - 1) * PAGE_SIZE;
        let end = std::cmp::min(start + PAGE_SIZE, self.logs.len());
        if start < self.logs.len() {
            &self.logs[start..end]
        } else {
            &[]
        }
    }

    fn format_last_refresh(&self) -> String {
        match self.last_refresh_time {
            Some(t) => {
                let elapsed = t.elapsed();
                if elapsed < Duration::from_secs(60) {
                    format!("{} 秒前刷新", elapsed.as_secs())
                } else if elapsed < Duration::from_secs(3600) {
                    format!("{} 分钟前刷新", elapsed.as_secs() / 60)
                } else {
                    format!("{} 小时前刷新", elapsed.as_secs() / 3600)
                }
            }
            None => "未刷新".to_string(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        styles::page_header(ui, "📋", "系统日志");

        ui.horizontal(|ui| {
            let refresh_btn = if self.loading {
                egui::Button::new(RichText::new("⏳ 刷新中...").color(egui::Color32::GRAY).size(styles::FONT_SIZE_MD))
                    .fill(styles::BG_SECONDARY)
                    .corner_radius(egui::CornerRadius::same(6))
            } else {
                styles::small_button("🔄 刷新")
            };
            
            if ui.add(refresh_btn).clicked() && !self.loading {
                self.request_refresh();
            }
            
            ui.checkbox(&mut self.auto_refresh, "自动刷新");
            
            ui.label(RichText::new("显示条数:").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
            
            let response = styles::input_frame().show(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.fetch_count_buf)
                    .desired_width(60.0)
                    .font(egui::FontId::new(styles::FONT_SIZE_MD, egui::FontFamily::Proportional)))
            });
            
            if response.response.lost_focus() || response.response.has_focus() {
                if let Ok(v) = self.fetch_count_buf.parse::<usize>() {
                    let new_count = v.clamp(1, 10_000);
                    if new_count != self.fetch_count {
                        self.fetch_count = new_count;
                        self.fetch_count_buf = new_count.to_string();
                    }
                } else {
                    self.fetch_count_buf = self.fetch_count.to_string();
                }
            }
            
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("共 {} 条日志 | {}", self.logs.len(), self.format_last_refresh()))
                    .size(styles::FONT_SIZE_SM).color(styles::TEXT_MUTED_COLOR));
            });
        });

        if self.auto_refresh {
            ui.ctx().request_repaint_after(Duration::from_secs(5));
            if !self.loading {
                self.request_refresh();
            }
        }

        if let Some(err) = &self.last_error {
            styles::status_message(ui, err, false);
            ui.add_space(styles::SPACING_MD);
        }

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            
            if self.loading && self.logs.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(50.0);
                    ui.spinner();
                    ui.add_space(styles::SPACING_MD);
                    ui.label(RichText::new("正在加载日志...").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
                });
                return;
            }
            
            if self.logs.is_empty() {
                styles::empty_state(ui, "📭", "暂无日志记录", "服务运行后日志会在这里显示");
                return;
            }

            let available_width = ui.available_width();
            let table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(styles::table_column_percent(available_width, 0.16, 130.0))
                .column(styles::table_column_percent(available_width, 0.08, 70.0))
                .column(styles::table_column_percent(available_width, 0.07, 60.0))
                .column(styles::table_column_percent(available_width, 0.12, 100.0))
                .column(styles::table_column_remainder(280.0))
                .min_scrolled_height(0.0)
                .sense(egui::Sense::hover());

            let display_logs = if self.logs.len() > LOG_THRESHOLD {
                self.get_page_logs()
            } else {
                &self.logs
            };

            table
                .header(styles::FONT_SIZE_MD, |mut header| {
                    header.col(|ui| {
                        ui.label(RichText::new("时间").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("级别").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("协议").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("客户端").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("信息").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                })
                .body(|mut body| {
                    for entry in display_logs {
                        body.row(styles::FONT_SIZE_MD, |mut row| {
                            row.col(|ui| {
                                ui.label(RichText::new(entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string())
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_SECONDARY_COLOR));
                            });
                            row.col(|ui| {
                                let level_color = match entry.level {
                                    crate::core::logger::LogLevel::Error => styles::DANGER_COLOR,
                                    crate::core::logger::LogLevel::Warning => styles::WARNING_COLOR,
                                    crate::core::logger::LogLevel::Debug => styles::TEXT_MUTED_COLOR,
                                    _ => styles::SUCCESS_COLOR,
                                };
                                ui.label(RichText::new(entry.level.to_string())
                                    .size(styles::FONT_SIZE_SM)
                                    .strong()
                                    .color(level_color));
                            });
                            row.col(|ui| {
                                let protocol = entry.fields.protocol.as_deref().unwrap_or("-");
                                let protocol_color = match protocol {
                                    "FTP" => styles::PRIMARY_COLOR,
                                    "SFTP" => styles::INFO_COLOR,
                                    _ => styles::TEXT_MUTED_COLOR,
                                };
                                ui.label(RichText::new(protocol)
                                    .size(styles::FONT_SIZE_SM)
                                    .strong()
                                    .color(protocol_color));
                            });
                            row.col(|ui| {
                                let client_ip = entry.fields.client_ip.as_deref().unwrap_or("-");
                                ui.label(RichText::new(client_ip)
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR));
                            });
                            row.col(|ui| {
                                let msg = if let Some(user) = &entry.fields.username {
                                    format!("[{}] {}", user, entry.fields.message)
                                } else {
                                    entry.fields.message.clone()
                                };
                                ui.label(RichText::new(&msg).size(styles::FONT_SIZE_SM).color(styles::TEXT_PRIMARY_COLOR));
                            });
                        });
                    }
                });
        });

        if self.logs.len() > LOG_THRESHOLD {
            ui.add_space(styles::SPACING_SM);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("第 {} / {} 页", self.current_page, self.total_pages))
                        .size(styles::FONT_SIZE_SM).color(styles::TEXT_MUTED_COLOR));
                    
                    ui.add_space(styles::SPACING_SM);
                    
                    if ui.add(styles::small_button("末页")).clicked() {
                        self.current_page = self.total_pages;
                    }
                    if ui.add(styles::small_button("下一页")).clicked() && self.current_page < self.total_pages {
                        self.current_page += 1;
                    }
                    if ui.add(styles::small_button("上一页")).clicked() && self.current_page > 1 {
                        self.current_page -= 1;
                    }
                    if ui.add(styles::small_button("首页")).clicked() {
                        self.current_page = 1;
                    }
                });
            });
        }
    }
}
