use egui::RichText;
use crate::core::config::Config;
use crate::core::logger::LogEntry;
use crate::gui_egui::styles;
use egui_extras::TableBuilder;
use std::time::{Duration, Instant};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::collections::VecDeque;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::{self, Receiver};

const MAX_DISPLAY_LOGS: usize = 500;  // 最大显示 500 条，避免内存过大（从 2000 降低到 500）
const INITIAL_FETCH_COUNT: usize = 100;  // 初始加载 100 条（从 200 降低到 100）
const INCREMENTAL_READ_SIZE: usize = 20;  // 每次增量读取最多 20 条（从 50 降低到 20）

pub struct FileLogTab {
    logs: VecDeque<LogEntry>,  // 使用 VecDeque 优化头部删除
    last_error: Option<String>,
    loading: bool,
    last_refresh_time: Option<Instant>,
    log_dir: PathBuf,
    // 增量读取状态
    last_file_pos: u64,
    current_log_file: Option<PathBuf>,
    // 文件监听器（事件驱动）
    log_watcher: Option<RecommendedWatcher>,
    log_rx: Option<Receiver<Result<Event, notify::Error>>>,
    needs_refresh: bool,  // 标记是否需要刷新
    last_event_time: Option<Instant>,  // 上次事件时间（用于防抖）
}

impl Default for FileLogTab {
    fn default() -> Self {
        let log_dir = Config::get_config_path()
            .parent()
            .map(|p| p.join("logs"))
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData\\wftpg\\logs"));
        
        Self {
            logs: VecDeque::with_capacity(MAX_DISPLAY_LOGS),
            last_error: None,
            loading: false,
            last_refresh_time: None,
            log_dir,
            last_file_pos: 0,
            current_log_file: None,
            log_watcher: None,
            log_rx: None,
            needs_refresh: false,
            last_event_time: None,
        }
    }
}

impl FileLogTab {
    pub fn new() -> Self {
        let mut tab = Self::default();
        // 初始化文件监听器
        tab.init_log_watcher();
        tab.load_logs();
        tab
    }

    /// 初始化日志文件监听器
    fn init_log_watcher(&mut self) {
        // 创建通道接收文件事件
        let (tx, rx) = mpsc::channel();
        
        // 创建监听器
        let watcher_result = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                let _ = tx.send(res);
            },
            notify::Config::default()
                .with_poll_interval(Duration::from_secs(2))  // 轮询间隔
        );
        
        match watcher_result {
            Ok(mut watcher) => {
                // 监听日志目录
                if self.log_dir.exists() {
                    if let Err(e) = watcher.watch(&self.log_dir, RecursiveMode::NonRecursive) {
                        tracing::warn!("Failed to watch log directory: {}", e);
                    } else {
                        tracing::info!("File log watcher initialized for: {:?}", self.log_dir);
                    }
                }
                
                self.log_watcher = Some(watcher);
                self.log_rx = Some(rx);
            }
            Err(e) => {
                tracing::error!("Failed to create log watcher: {}", e);
            }
        }
    }

    /// 检查日志文件事件（在 UI 循环中调用）
    pub fn check_log_events(&mut self) {
        if let Some(rx) = &self.log_rx {
            // 非阻塞接收所有积压的事件，但有数量限制避免处理过多事件
            let mut event_count = 0;
            const MAX_EVENTS_PER_FRAME: usize = 5;
            while let Ok(result) = rx.try_recv() {
                event_count += 1;
                if event_count > MAX_EVENTS_PER_FRAME {
                    // 丢弃多余事件，避免一帧内处理过多
                    break;
                }
                match result {
                    Ok(event) => {
                        // 只处理文件修改和创建事件
                        match event.kind {
                            EventKind::Modify(_) | EventKind::Create(_) => {
                                // 检查是否是当前正在读取的日志文件
                                for path in &event.paths {
                                    if path.extension().is_some_and(|ext| ext == "log") {
                                        // 防抖动：1 秒内的事件只触发一次
                                        let now = Instant::now();
                                        if self.last_event_time.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
                                            self.needs_refresh = true;
                                            self.last_event_time = Some(now);
                                            tracing::debug!("File log file changed: {:?}", path);
                                        }
                                        break;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        tracing::warn!("File log watcher error: {}", e);
                    }
                }
            }
        }
    }

    /// 初始化加载日志
    fn load_logs(&mut self) {
        self.loading = true;
        self.last_error = None;
        self.logs.clear();
        
        let log_dir = &self.log_dir;
        
        // 找到最新的日志文件
        if let Ok(entries) = fs::read_dir(log_dir) {
            let mut log_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    (name.starts_with("file-ops.") || name.starts_with("file-ops-")) && name.ends_with(".log")
                })
                .collect();
            
            // 按修改时间排序，最新的在前
            log_files.sort_by(|a, b| {
                let a_time = a.metadata().and_then(|m| m.modified()).ok();
                let b_time = b.metadata().and_then(|m| m.modified()).ok();
                b_time.cmp(&a_time)
            });
            
            // 读取最新日志文件的最后部分
            if let Some(latest_file) = log_files.first() {
                self.current_log_file = Some(latest_file.path());
                if let Ok(file) = File::open(latest_file.path()) {
                    let metadata = file.metadata().ok();
                    let file_size = metadata.map(|m| m.len()).unwrap_or(0);
                    
                    // 从文件末尾往前读，获取最新的 INITIAL_FETCH_COUNT 条
                    let reader = BufReader::new(file);
                    let mut lines: Vec<_> = reader.lines().collect();
                    lines.reverse();
                    
                    let mut count = 0;
                    for line in lines {
                        if count >= INITIAL_FETCH_COUNT {
                            break;
                        }
                        if let Ok(line) = line
                            && let Ok(log_entry) = serde_json::from_str::<LogEntry>(&line)
                            && log_entry.fields.operation.is_some()
                        {
                            self.logs.push_back(log_entry);
                            count += 1;
                        }
                    }
                    
                    // 记录当前文件位置（下次从这里继续读）
                    self.last_file_pos = file_size;
                }
            }
        }
        
        // 按时间戳降序排序（新的在前），然后只保留最新的 MAX_DISPLAY_LOGS
        let mut logs_vec: Vec<_> = self.logs.drain(..).collect();
        logs_vec.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        if logs_vec.len() > MAX_DISPLAY_LOGS {
            logs_vec.truncate(MAX_DISPLAY_LOGS);
        }
        self.logs.extend(logs_vec);
        
        self.loading = false;
        self.last_refresh_time = Some(Instant::now());
    }

    /// 增量读取新日志（只读新增的部分）
    fn incrementally_read_logs(&mut self) {
        let Some(current_file) = &self.current_log_file else {
            return;
        };
        
        if !current_file.exists() {
            // 文件不存在，重新初始化
            self.load_logs();
            return;
        }
        
        if let Ok(file) = File::open(current_file) {
            let metadata = match file.metadata() {
                Ok(m) => m,
                Err(_) => return,
            };
            
            let current_size = metadata.len();
            
            // 如果文件变小了（日志轮转），重新初始化
            if current_size < self.last_file_pos {
                self.load_logs();
                return;
            }
            
            // 如果没有新内容，直接返回
            if current_size == self.last_file_pos {
                return;
            }
            
            // 只读取新增的部分
            let mut reader = BufReader::new(file);
            if reader.seek(SeekFrom::Start(self.last_file_pos)).is_err() {
                return;
            }
            
            let mut new_entries = Vec::new();
            let mut count = 0;
            
            for line in reader.lines() {
                if count >= INCREMENTAL_READ_SIZE {
                    break;
                }
                if let Ok(line) = line
                    && let Ok(log_entry) = serde_json::from_str::<LogEntry>(&line)
                    && log_entry.fields.operation.is_some()
                {
                    new_entries.push(log_entry);
                    count += 1;
                }
            }
            
            // 只在成功读取后更新文件位置，避免跳过有效日志
            // 如果没有读到任何日志（都是无效行），也更新位置避免重复读取
            if !new_entries.is_empty() || count == 0 {
                self.last_file_pos = current_size;
            }
            
            // 如果有新日志，插入到队列头部（最新的在前）
            if !new_entries.is_empty() {
                for entry in new_entries.into_iter().rev() {
                    if self.logs.len() >= MAX_DISPLAY_LOGS {
                        self.logs.pop_back();  // 移除最旧的
                    }
                    self.logs.push_front(entry);
                }
                
                // 更新刷新时间
                self.last_refresh_time = Some(Instant::now());
            }
        }
    }

    fn request_refresh(&mut self) {
        if self.loading {
            return;
        }
        self.load_logs();
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

        // 先检查文件事件（事件驱动）
        self.check_log_events();

        // 如果有新日志触发，则加载（防抖动已处理）
        if self.needs_refresh && !self.loading {
            self.incrementally_read_logs();
            self.needs_refresh = false;
        }

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

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let status_text = if self.loading {
                    format!("加载中... | {} 条", self.logs.len())
                } else {
                    format!("共 {} 条记录 | {}", self.logs.len(), self.format_last_refresh())
                };
                ui.label(RichText::new(status_text)
                    .size(styles::FONT_SIZE_MD).color(styles::TEXT_MUTED_COLOR));
            });
        });

        // 移除自动刷新，改为手动刷新和事件驱动

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

            // 使用 ScrollArea 包裹表格，支持滚动
            // 使用 lazy_body 优化性能，只渲染可见行
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
                    // 直接使用 iter() 而不收集中间 Vec
                    for entry in &self.logs {
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
