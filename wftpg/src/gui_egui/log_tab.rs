use crate::core::config::Config;
use crate::core::logger::LogEntry;
use crate::gui_egui::styles;
use egui::{Color32, RichText};
use egui_extras::TableBuilder;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const MAX_DISPLAY_LOGS: usize = 500; // 最大显示 500 条，避免内存过大（从 2000 降低到 500）
const INITIAL_FETCH_COUNT: usize = 100; // 初始加载 100 条（从 200 降低到 100）
const INCREMENTAL_READ_SIZE: usize = 20; // 每次增量读取最多 20 条（从 50 降低到 20）

pub struct LogTab {
    logs: VecDeque<LogEntry>, // 使用 VecDeque 优化头部删除
    last_error: Option<String>,
    loading: bool,
    last_refresh_time: Option<Instant>,
    log_dir: PathBuf,
    scroll_to_bottom: bool,
    user_at_bottom: bool,
    new_logs_count: usize,
    // 增量读取状态
    last_file_pos: u64,
    current_log_file: Option<PathBuf>,
    // 文件监听器（事件驱动）
    log_watcher: Option<RecommendedWatcher>,
    log_rx: Option<Receiver<Result<Event, notify::Error>>>,
    needs_refresh: bool,              // 标记是否需要刷新
    last_event_time: Option<Instant>, // 上次事件时间（用于防抖）
}

impl Default for LogTab {
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
            scroll_to_bottom: true,
            user_at_bottom: true,
            new_logs_count: 0,
            last_file_pos: 0,
            current_log_file: None,
            log_watcher: None,
            log_rx: None,
            needs_refresh: false,
            last_event_time: None,
        }
    }
}

impl LogTab {
    pub fn new() -> Self {
        let mut tab = Self::default();
        // 初始化文件监听器
        tab.init_log_watcher();
        tab.load_logs();
        tab
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
                    (name.starts_with("wftpg.") || name.starts_with("wftpg-"))
                        && name.ends_with(".log")
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
                            && log_entry.fields.operation.is_none()
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
        self.new_logs_count = 0;
    }

    /// 初始化日志文件监听器
    fn init_log_watcher(&mut self) {
        // 创建通道接收文件事件
        let (tx, rx) = mpsc::channel();

        // 创建监听器 - 使用更短的轮询间隔以提高响应速度
        let watcher_result = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                let _ = tx.send(res);
            },
            notify::Config::default().with_poll_interval(Duration::from_millis(500)),
        );

        match watcher_result {
            Ok(mut watcher) => {
                // 监听日志目录
                self.watch_log_dir(&mut watcher);

                self.log_watcher = Some(watcher);
                self.log_rx = Some(rx);
            }
            Err(e) => {
                tracing::error!("Failed to create log watcher: {}", e);
            }
        }
    }

    /// 尝试监听日志目录
    fn watch_log_dir(&mut self, watcher: &mut RecommendedWatcher) {
        if self.log_dir.exists() {
            if let Err(e) = watcher.watch(&self.log_dir, RecursiveMode::NonRecursive) {
                tracing::warn!("Failed to watch log directory: {}", e);
            } else {
                tracing::info!("Log watcher initialized for: {:?}", self.log_dir);
            }
        } else {
            tracing::warn!("Log directory does not exist yet: {:?}", self.log_dir);
        }
    }

    /// 检查日志文件事件（在 UI 循环中调用）
    pub fn check_log_events(&mut self, ctx: &egui::Context) {
        // 如果日志目录不存在，直接返回
        if !self.log_dir.exists() {
            return;
        }

        if let Some(rx) = &self.log_rx {
            let mut event_count = 0;
            const MAX_EVENTS_PER_FRAME: usize = 10;
            while let Ok(result) = rx.try_recv() {
                event_count += 1;
                if event_count > MAX_EVENTS_PER_FRAME {
                    break;
                }
                match result {
                    Ok(event) => {
                        // 处理所有类型的事件（创建、修改、删除等）
                        for path in &event.paths {
                            if path.extension().is_some_and(|ext| ext == "log") {
                                let now = Instant::now();
                                // 减少防抖时间到 100ms，提高响应速度
                                if self
                                    .last_event_time
                                    .is_none_or(|t| t.elapsed() >= Duration::from_millis(100))
                                {
                                    self.needs_refresh = true;
                                    self.last_event_time = Some(now);
                                    tracing::debug!("Log file changed: {:?}, will refresh", path);
                                    // 请求 UI 重绘，确保日志能够立即显示
                                    ctx.request_repaint();
                                }
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Log watcher error: {}", e);
                    }
                }
            }
        }
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
                    && log_entry.fields.operation.is_none()
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
                let old_len = self.logs.len();

                // 将新日志插入到头部
                for entry in new_entries.into_iter().rev() {
                    if self.logs.len() >= MAX_DISPLAY_LOGS {
                        self.logs.pop_back(); // 移除最旧的
                    }
                    self.logs.push_front(entry);
                }

                // 检测是否有新日志到达
                if self.user_at_bottom {
                    self.scroll_to_bottom = true;
                } else {
                    self.new_logs_count = self
                        .new_logs_count
                        .saturating_add(self.logs.len() - old_len);
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
        styles::page_header(ui, "📋", "系统日志");

        // 先检查文件事件（事件驱动），并传入 context 用于请求重绘
        let ctx = ui.ctx().clone();
        self.check_log_events(&ctx);

        // 如果有新日志触发，则加载（防抖动已处理）
        if self.needs_refresh && !self.loading {
            self.incrementally_read_logs();
            self.needs_refresh = false;
        }

        ui.horizontal(|ui| {
            let refresh_btn = if self.loading {
                egui::Button::new(
                    RichText::new("⏳ 刷新中...")
                        .color(egui::Color32::GRAY)
                        .size(styles::FONT_SIZE_MD),
                )
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
                    format!("共 {} 条 | {}", self.logs.len(), self.format_last_refresh())
                };
                ui.label(
                    RichText::new(status_text)
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_MUTED_COLOR),
                );
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
                    ui.label(
                        RichText::new("正在加载日志...")
                            .size(styles::FONT_SIZE_MD)
                            .color(styles::TEXT_SECONDARY_COLOR),
                    );
                });
                return;
            }

            if self.logs.is_empty() {
                styles::empty_state(ui, "📭", "暂无日志记录", "服务运行后日志会在这里显示");
                return;
            }

            let available_width = ui.available_width();

            // 使用 ScrollArea 包裹表格，支持滚动
            let scroll_area_id = egui::Id::new("log_scroll_area");

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(self.scroll_to_bottom) // 根据复选框动态控制
                .id_salt(scroll_area_id)
                .show(ui, |ui| {
                    // 使用 lazy_body 优化性能，只渲染可见行
                    let table = TableBuilder::new(ui)
                        .striped(true)
                        .resizable(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(styles::table_column_percent(available_width, 0.20, 130.0))
                        .column(styles::table_column_percent(available_width, 0.08, 55.0))
                        .column(styles::table_column_percent(available_width, 0.08, 55.0))
                        .column(styles::table_column_percent(available_width, 0.12, 90.0))
                        .column(styles::table_column_remainder(280.0))
                        .min_scrolled_height(0.0)
                        .sense(egui::Sense::hover());

                    table
                        .header(styles::FONT_SIZE_MD, |mut header| {
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("时间")
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("级别")
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("协议")
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("客户端")
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("信息")
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            });
                        })
                        .body(|mut body| {
                            // 直接使用 iter() 而不收集中间 Vec
                            for entry in &self.logs {
                                body.row(styles::FONT_SIZE_MD, |mut row| {
                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(
                                                entry
                                                    .timestamp
                                                    .format("%Y-%m-%d %H:%M:%S")
                                                    .to_string(),
                                            )
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                        );
                                    });
                                    row.col(|ui| {
                                        let level_color = match entry.level {
                                            crate::core::logger::LogLevel::Error => {
                                                styles::DANGER_COLOR
                                            }
                                            crate::core::logger::LogLevel::Warning => {
                                                styles::WARNING_COLOR
                                            }
                                            crate::core::logger::LogLevel::Debug => {
                                                styles::TEXT_MUTED_COLOR
                                            }
                                            _ => styles::SUCCESS_COLOR,
                                        };
                                        ui.label(
                                            RichText::new(entry.level.to_string())
                                                .size(styles::FONT_SIZE_MD)
                                                .strong()
                                                .color(level_color),
                                        );
                                    });
                                    row.col(|ui| {
                                        let protocol =
                                            entry.fields.protocol.as_deref().unwrap_or("-");
                                        let protocol_color = match protocol {
                                            "FTP" => styles::PRIMARY_COLOR,
                                            "SFTP" => styles::INFO_COLOR,
                                            _ => styles::TEXT_MUTED_COLOR,
                                        };
                                        ui.label(
                                            RichText::new(protocol)
                                                .size(styles::FONT_SIZE_MD)
                                                .strong()
                                                .color(protocol_color),
                                        );
                                    });
                                    row.col(|ui| {
                                        let client_ip =
                                            entry.fields.client_ip.as_deref().unwrap_or("-");
                                        ui.label(
                                            RichText::new(client_ip)
                                                .size(styles::FONT_SIZE_MD)
                                                .color(styles::TEXT_LABEL_COLOR),
                                        );
                                    });
                                    row.col(|ui| {
                                        let msg = if let Some(user) = &entry.fields.username {
                                            format!("[{}] {}", user, entry.fields.message)
                                        } else {
                                            entry.fields.message.clone()
                                        };
                                        ui.label(
                                            RichText::new(&msg)
                                                .size(styles::FONT_SIZE_MD)
                                                .color(styles::TEXT_PRIMARY_COLOR),
                                        );
                                    });
                                });
                                body.row(2.0, |mut row| {
                                    let col_count = 5;
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

            // 滚动控制区域
            ui.add_space(styles::SPACING_SM);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.scroll_to_bottom, "自动滚动到底部");

                // 如果有新日志且用户不在底部，显示提示
                if self.new_logs_count > 0 && !self.user_at_bottom {
                    let btn = egui::Button::new(
                        RichText::new(format!("⬇ {} 条新日志", self.new_logs_count))
                            .color(Color32::WHITE)
                            .size(styles::FONT_SIZE_SM),
                    )
                    .fill(styles::INFO_COLOR)
                    .corner_radius(egui::CornerRadius::same(4));

                    if ui.add(btn).clicked() {
                        self.scroll_to_bottom = true;
                        self.new_logs_count = 0;
                    }
                }
            });
        });
    }
}
