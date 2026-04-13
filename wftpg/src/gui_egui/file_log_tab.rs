use crate::core::config::Config;
use crate::core::i18n;
use crate::core::logger::LogEntry;
use crate::gui_egui::styles;
use egui::RichText;
use egui_extras::TableBuilder;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const MAX_DISPLAY_LOGS: usize = 500;
const INITIAL_FETCH_COUNT: usize = 100;
const INCREMENTAL_READ_SIZE: usize = 20;

pub struct FileLogTab {
    logs: VecDeque<LogEntry>,
    last_error: Option<String>,
    loading: bool,
    last_refresh_time: Option<Instant>,
    log_dir: PathBuf,
    last_file_pos: u64,
    current_log_file: Option<PathBuf>,
    log_watcher: Option<RecommendedWatcher>,
    log_rx: Option<Receiver<Result<Event, notify::Error>>>,
    needs_refresh: bool,
    last_event_time: Option<Instant>,
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
        tab.init_log_watcher();
        tab.load_logs();
        tab
    }

    fn init_log_watcher(&mut self) {
        let (tx, rx) = mpsc::channel();

        let watcher_result = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                let _ = tx.send(res);
            },
            notify::Config::default().with_poll_interval(Duration::from_millis(500)),
        );

        match watcher_result {
            Ok(mut watcher) => {
                self.watch_log_dir(&mut watcher);

                self.log_watcher = Some(watcher);
                self.log_rx = Some(rx);
            }
            Err(e) => {
                tracing::error!("Failed to create log watcher: {}", e);
            }
        }
    }

    fn watch_log_dir(&mut self, watcher: &mut RecommendedWatcher) {
        if self.log_dir.exists() {
            if let Err(e) = watcher.watch(&self.log_dir, RecursiveMode::NonRecursive) {
                tracing::warn!("Failed to watch log directory: {}", e);
            } else {
                tracing::info!("File log watcher initialized for: {:?}", self.log_dir);
            }
        } else {
            tracing::warn!("Log directory does not exist yet: {:?}", self.log_dir);
        }
    }

    pub fn check_log_events(&mut self, ctx: &egui::Context) {
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
                        for path in &event.paths {
                            if path.extension().is_some_and(|ext| ext == "log") {
                                let now = Instant::now();
                                if self
                                    .last_event_time
                                    .is_none_or(|t| t.elapsed() >= Duration::from_millis(100))
                                {
                                    self.needs_refresh = true;
                                    self.last_event_time = Some(now);
                                    tracing::debug!(
                                        "File log file changed: {:?}, will refresh",
                                        path
                                    );
                                    ctx.request_repaint();
                                }
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("File log watcher error: {}", e);
                    }
                }
            }
        }
    }

    fn load_logs(&mut self) {
        self.loading = true;
        self.last_error = None;
        self.logs.clear();

        let log_dir = &self.log_dir;

        if let Ok(entries) = fs::read_dir(log_dir) {
            let mut log_files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    (name.starts_with("file-ops.") || name.starts_with("file-ops-"))
                        && name.ends_with(".log")
                })
                .collect();

            log_files.sort_by(|a, b| {
                let a_time = a.metadata().and_then(|m| m.modified()).ok();
                let b_time = b.metadata().and_then(|m| m.modified()).ok();
                b_time.cmp(&a_time)
            });

            if let Some(latest_file) = log_files.first() {
                self.current_log_file = Some(latest_file.path());
                if let Ok(file) = File::open(latest_file.path()) {
                    let metadata = file.metadata().ok();
                    let file_size = metadata.map(|m| m.len()).unwrap_or(0);

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

                    self.last_file_pos = file_size;
                }
            }
        }

        let mut logs_vec: Vec<_> = self.logs.drain(..).collect();
        logs_vec.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        if logs_vec.len() > MAX_DISPLAY_LOGS {
            logs_vec.truncate(MAX_DISPLAY_LOGS);
        }
        self.logs.extend(logs_vec);

        self.loading = false;
        self.last_refresh_time = Some(Instant::now());
    }

    fn incrementally_read_logs(&mut self) {
        let Some(current_file) = &self.current_log_file else {
            return;
        };

        if !current_file.exists() {
            self.load_logs();
            return;
        }

        if let Ok(file) = File::open(current_file) {
            let metadata = match file.metadata() {
                Ok(m) => m,
                Err(_) => return,
            };

            let current_size = metadata.len();

            if current_size < self.last_file_pos {
                self.load_logs();
                return;
            }

            if current_size == self.last_file_pos {
                return;
            }

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

            if !new_entries.is_empty() || count == 0 {
                self.last_file_pos = current_size;
            }

            if !new_entries.is_empty() {
                for entry in new_entries.into_iter().rev() {
                    if self.logs.len() >= MAX_DISPLAY_LOGS {
                        self.logs.pop_back();
                    }
                    self.logs.push_front(entry);
                }

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
                    i18n::t_fmt("file_log.n_seconds_ago", &[&elapsed.as_secs().to_string()])
                } else if elapsed < Duration::from_secs(3600) {
                    i18n::t_fmt(
                        "file_log.n_minutes_ago",
                        &[&(elapsed.as_secs() / 60).to_string()],
                    )
                } else {
                    i18n::t_fmt(
                        "file_log.n_hours_ago",
                        &[&(elapsed.as_secs() / 3600).to_string()],
                    )
                }
            }
            None => i18n::t("file_log.not_refreshed"),
        }
    }

    fn translate_operation(&self, op: &str) -> String {
        match op {
            "UPLOAD" => i18n::t("file_log.upload"),
            "DOWNLOAD" => i18n::t("file_log.download"),
            "DELETE" => i18n::t("file_log.delete"),
            "MKDIR" => i18n::t("file_log.mkdir"),
            "RMDIR" => i18n::t("file_log.rmdir"),
            "RENAME" => i18n::t("file_log.rename"),
            "UPDATE" => i18n::t("file_log.update"),
            "SYMLINK" => i18n::t("file_log.symlink"),
            "APPEND" => i18n::t("file_log.append"),
            _ => op.to_string(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        styles::page_header(ui, "📁", &i18n::t("file_log.title"));

        let ctx = ui.ctx().clone();
        self.check_log_events(&ctx);

        if self.needs_refresh && !self.loading {
            self.incrementally_read_logs();
            self.needs_refresh = false;
        }

        ui.horizontal(|ui| {
            let refresh_text = i18n::t("file_log.refresh");
            let refresh_btn = if self.loading {
                egui::Button::new(
                    RichText::new(i18n::t("file_log.refreshing"))
                        .color(egui::Color32::GRAY)
                        .size(styles::FONT_SIZE_MD),
                )
                .fill(styles::BG_SECONDARY)
                .corner_radius(egui::CornerRadius::same(6))
            } else {
                styles::small_button(&refresh_text)
            };

            if ui.add(refresh_btn).clicked() && !self.loading {
                self.request_refresh();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let status_text = if self.loading {
                    i18n::t_fmt("file_log.loading", &[&self.logs.len().to_string()])
                } else {
                    i18n::t_fmt(
                        "file_log.total_count",
                        &[&self.logs.len().to_string(), &self.format_last_refresh()],
                    )
                };
                ui.label(
                    RichText::new(status_text)
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_MUTED_COLOR),
                );
            });
        });

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
                        RichText::new(i18n::t("file_log.loading_log"))
                            .size(styles::FONT_SIZE_MD)
                            .color(styles::TEXT_SECONDARY_COLOR),
                    );
                });
                return;
            }

            if self.logs.is_empty() {
                styles::empty_state(
                    ui,
                    "📭",
                    &i18n::t("file_log.no_logs"),
                    &i18n::t("file_log.no_logs_hint"),
                );
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

            table
                .header(styles::FONT_SIZE_MD, |mut header| {
                    header.col(|ui| {
                        ui.with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    RichText::new(i18n::t("file_log.col_time"))
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            },
                        );
                    });
                    header.col(|ui| {
                        ui.with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    RichText::new(i18n::t("file_log.col_user"))
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            },
                        );
                    });
                    header.col(|ui| {
                        ui.with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    RichText::new(i18n::t("file_log.col_client"))
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            },
                        );
                    });
                    header.col(|ui| {
                        ui.with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    RichText::new(i18n::t("file_log.col_protocol"))
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            },
                        );
                    });
                    header.col(|ui| {
                        ui.with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    RichText::new(i18n::t("file_log.col_operation"))
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            },
                        );
                    });
                    header.col(|ui| {
                        ui.with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    RichText::new(i18n::t("file_log.col_size"))
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            },
                        );
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new(i18n::t("file_log.col_file_path"))
                                .strong()
                                .color(styles::TEXT_PRIMARY_COLOR),
                        );
                    });
                })
                .body(|mut body| {
                    for entry in &self.logs {
                        body.row(styles::FONT_SIZE_MD, |mut row| {
                            row.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
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
                                    },
                                );
                            });
                            row.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        let username =
                                            entry.fields.username.as_deref().unwrap_or("-");
                                        ui.label(
                                            RichText::new(username)
                                                .size(styles::FONT_SIZE_MD)
                                                .color(styles::TEXT_PRIMARY_COLOR),
                                        );
                                    },
                                );
                            });
                            row.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        let client_ip =
                                            entry.fields.client_ip.as_deref().unwrap_or("-");
                                        ui.label(
                                            RichText::new(client_ip)
                                                .size(styles::FONT_SIZE_MD)
                                                .color(styles::TEXT_LABEL_COLOR),
                                        );
                                    },
                                );
                            });
                            row.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
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
                                    },
                                );
                            });
                            row.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        let operation =
                                            entry.fields.operation.as_deref().unwrap_or("-");
                                        let success = entry.fields.success.unwrap_or(true);
                                        let translated_op = self.translate_operation(operation);
                                        let op_color = match operation {
                                            "DELETE" | "RMDIR" => styles::DANGER_COLOR,
                                            "UPLOAD" | "MKDIR" => styles::SUCCESS_COLOR,
                                            "DOWNLOAD" => styles::INFO_COLOR,
                                            "RENAME" | "COPY" | "MOVE" => styles::WARNING_COLOR,
                                            "UPDATE" => styles::TEXT_MUTED_COLOR,
                                            _ => styles::TEXT_LABEL_COLOR,
                                        };
                                        let status_icon = if success { "√" } else { "×" };
                                        ui.label(
                                            RichText::new(format!(
                                                "{} {}",
                                                status_icon, translated_op
                                            ))
                                            .size(styles::FONT_SIZE_MD)
                                            .strong()
                                            .color(op_color),
                                        );
                                    },
                                );
                            });
                            row.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        let size_str = entry
                                            .fields
                                            .file_size
                                            .filter(|&s| s > 0)
                                            .map(format_size)
                                            .unwrap_or_else(|| "-".to_string());
                                        ui.label(
                                            RichText::new(&size_str)
                                                .size(styles::FONT_SIZE_MD)
                                                .color(styles::TEXT_LABEL_COLOR),
                                        );
                                    },
                                );
                            });
                            row.col(|ui| {
                                let file_path = entry.fields.file_path.as_deref().unwrap_or("-");
                                ui.label(
                                    RichText::new(file_path)
                                        .size(styles::FONT_SIZE_MD)
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
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
