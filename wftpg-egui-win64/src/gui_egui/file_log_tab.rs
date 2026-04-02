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
const DEFAULT_FETCH_COUNT: usize = 500;  // 增加默认加载数量到 500

pub struct FileLogTab {
    logs: Vec<LogEntry>,
    auto_refresh: bool,
    fetch_count: usize,
    fetch_count_buf: String,
    last_error: Option<String>,
    loading: bool,
    last_refresh_time: Option<Instant>,
    current_page: usize,
    total_pages: usize,
    stick_to_bottom: bool,
    log_dir: PathBuf,
}

impl Default for FileLogTab {
    fn default() -> Self {
        let log_dir = Config::get_config_path()
            .parent()
            .map(|p| p.join("logs"))
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData\\wftpg\\logs"));
        
        Self {
            logs: Vec::new(),
            auto_refresh: true,
            fetch_count: DEFAULT_FETCH_COUNT,  // 使用新的默认值
            fetch_count_buf: format!("{}", DEFAULT_FETCH_COUNT),
            last_error: None,
            loading: false,
            last_refresh_time: None,
            current_page: 1,
            total_pages: 1,
            stick_to_bottom: false,
            log_dir,
        }
    }
}

impl FileLogTab {
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
            // 收集所有 file-ops*.log 文件
            let mut log_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    // 匹配 file-ops.YYYY-MM-DD.log 格式（注意是点号分隔，也兼容短横线）
                    (name.starts_with("file-ops.") || name.starts_with("file-ops-")) && name.ends_with(".log")
                })
                .collect();
            
            // 按修改时间排序，最新的在前
            log_files.sort_by(|a, b| {
                let a_time = a.metadata().and_then(|m| m.modified()).ok();
                let b_time = b.metadata().and_then(|m| m.modified()).ok();
                b_time.cmp(&a_time)
            });
            
            // ✅ 只读取最新的一个日志文件
            if let Some(latest_file) = log_files.first()
                && let Ok(file) = File::open(latest_file.path()) {
                let reader = BufReader::new(file);
                // ✅ 从文件末尾开始读取（最新日志）
                let mut lines: Vec<_> = reader.lines().collect();
                // 倒序处理，优先处理最新的行
                lines.reverse();
                
                for line in lines {
                    if all_logs.len() >= count {
                        break;
                    }
                    if let Ok(line) = line
                        && let Ok(log_entry) = serde_json::from_str::<LogEntry>(&line)
                        && log_entry.fields.operation.is_some()
                    {
                        all_logs.push(log_entry);
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
        styles::page_header(ui, "📁", "文件操作日志");

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
            
            ui.label(RichText::new("显示条数:").size(styles::FONT_SIZE_LG).color(styles::TEXT_SECONDARY_COLOR));
            
            let response = styles::input_frame().show(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.fetch_count_buf)
                    .desired_width(60.0)
                    .font(egui::FontId::new(styles::FONT_SIZE_LG, egui::FontFamily::Proportional)))
            });
            
            if response.response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
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
                ui.label(RichText::new(format!("共 {} 条记录 | {}", self.logs.len(), self.format_last_refresh()))
                    .size(styles::FONT_SIZE_MD).color(styles::TEXT_MUTED_COLOR));
            });
        });

        if self.auto_refresh {
            // 只在距离上次刷新超过 5 秒时才真正刷新
            if self.last_refresh_time.is_none_or(|t| t.elapsed() >= Duration::from_secs(5))
                && !self.loading
            {
                self.request_refresh();
            }
            ui.ctx().request_repaint_after(Duration::from_secs(5));
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
                    ui.label(RichText::new("正在加载文件日志...").size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
                });
                return;
            }

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
                        ui.label(RichText::new("用户").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("客户端").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("协议").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("操作").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("大小").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                    header.col(|ui| {
                        ui.label(RichText::new("文件路径").strong().color(styles::TEXT_PRIMARY_COLOR));
                    });
                })
                .body(|mut body| {
                    for entry in display_logs {
                        body.row(styles::FONT_SIZE_MD, |mut row| {
                            row.col(|ui| {
                                ui.label(RichText::new(entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string())
                                    .size(styles::FONT_SIZE_MD)
                                    .color(styles::TEXT_SECONDARY_COLOR));
                            });
                            row.col(|ui| {
                                let username = entry.fields.username.as_deref().unwrap_or("-");
                                ui.label(RichText::new(username)
                                    .size(styles::FONT_SIZE_MD)
                                    .color(styles::TEXT_PRIMARY_COLOR));
                            });
                            row.col(|ui| {
                                let client_ip = entry.fields.client_ip.as_deref().unwrap_or("-");
                                ui.label(RichText::new(client_ip)
                                    .size(styles::FONT_SIZE_MD)
                                    .color(styles::TEXT_LABEL_COLOR));
                            });
                            row.col(|ui| {
                                let protocol = entry.fields.protocol.as_deref().unwrap_or("-");
                                let protocol_color = match protocol {
                                    "FTP" => styles::PRIMARY_COLOR,
                                    "SFTP" => styles::INFO_COLOR,
                                    _ => styles::TEXT_MUTED_COLOR,
                                };
                                ui.label(RichText::new(protocol)
                                    .size(styles::FONT_SIZE_MD)
                                    .strong()
                                    .color(protocol_color));
                            });
                            row.col(|ui| {
                                let operation = entry.fields.operation.as_deref().unwrap_or("-");
                                let success = entry.fields.success.unwrap_or(true);
                                let op_color = match operation {
                                    "DELETE" | "RMDIR" => styles::DANGER_COLOR,
                                    "UPLOAD" | "MKDIR" => styles::SUCCESS_COLOR,
                                    "DOWNLOAD" => styles::INFO_COLOR,
                                    "RENAME" | "COPY" | "MOVE" => styles::WARNING_COLOR,
                                    "UPDATE" => styles::TEXT_MUTED_COLOR,
                                    _ => styles::TEXT_LABEL_COLOR,
                                };
                                let status_icon = if success { "✓" } else { "✗" };
                                ui.label(RichText::new(format!("{} {}", status_icon, operation))
                                    .size(styles::FONT_SIZE_MD)
                                    .strong()
                                    .color(op_color));
                            });
                            row.col(|ui| {
                                let size_str = entry.fields.file_size
                                    .filter(|&s| s > 0)
                                    .map(format_size)
                                    .unwrap_or_else(|| "-".to_string());
                                ui.label(RichText::new(&size_str)
                                    .size(styles::FONT_SIZE_MD)
                                    .color(styles::TEXT_LABEL_COLOR));
                            });
                            row.col(|ui| {
                                let file_path = entry.fields.file_path.as_deref().unwrap_or("-");
                                ui.label(RichText::new(file_path)
                                    .size(styles::FONT_SIZE_MD)
                                    .color(styles::TEXT_PRIMARY_COLOR));
                            });
                        });
                        body.row(2.0, |mut row| {
                            let col_count = 7;
                            for _ in 0..col_count {
                                row.col(|ui| {
                                    let rect = ui.available_rect_before_wrap();
                                    let painter = ui.painter();
                                    painter.hline(
                                        rect.left()..=rect.right(),
                                        rect.center().y,
                                        egui::Stroke::new(1.0, styles::BORDER_COLOR),
                                    );
                                });
                            }
                        });
                    }
                });
        });

        if self.logs.len() > LOG_THRESHOLD {
            ui.add_space(styles::SPACING_SM);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.stick_to_bottom, "自动滚动到底部");
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("第 {} / {} 页", self.current_page, self.total_pages))
                        .size(styles::FONT_SIZE_MD).color(styles::TEXT_MUTED_COLOR));
                    
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
