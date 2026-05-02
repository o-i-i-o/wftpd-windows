use crate::core::config::Config;
use crate::core::i18n;
use crate::gui_egui::log_watcher::LogFileWatcher;
use crate::gui_egui::styles;
use egui::RichText;
use egui_extras::TableBuilder;
use std::path::PathBuf;

pub struct FileLogTab {
    watcher: LogFileWatcher,
}

impl FileLogTab {
    pub fn new() -> Self {
        let log_dir = Config::get_config_path()
            .parent()
            .map(|p| p.join("logs"))
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData\\wftpg\\logs"));

        let mut watcher = LogFileWatcher::new(log_dir, "file-ops.", |entry| {
            entry.fields.operation.is_some()
        });
        watcher.init();

        Self { watcher }
    }

    fn format_last_refresh(&self) -> String {
        match self.watcher.last_refresh_time() {
            Some(t) => styles::format_elapsed_time(
                t.elapsed(),
                "file_log.n_seconds_ago",
                "file_log.n_minutes_ago",
                "file_log.n_hours_ago",
            ),
            None => i18n::t("file_log.not_refreshed"),
        }
    }

    fn translate_operation(op: &str) -> String {
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

    fn clean_file_path(path: &str) -> String {
        if path.contains(" -> ") {
            let parts: Vec<&str> = path.split(" -> ").collect();
            if parts.len() == 2 {
                let left = parts[0].strip_prefix("\\\\?\\").unwrap_or(parts[0]);
                let right = parts[1].strip_prefix("\\\\?\\").unwrap_or(parts[1]);
                format!("{} -> {}", left, right)
            } else {
                path.to_string()
            }
        } else {
            path.strip_prefix("\\\\?\\").unwrap_or(path).to_string()
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.watcher.check_events(&ctx);
        self.watcher.process_refresh();

        let log_count = self.watcher.logs().len();
        let last_refresh = self.format_last_refresh();

        ui.horizontal(|ui| {
            let refresh_label = i18n::t("file_log.refresh");
            let refresh_btn = styles::small_button(&refresh_label);

            if ui.add(refresh_btn).clicked() {
                self.watcher.request_refresh();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let status_text = i18n::t_fmt(
                    "file_log.total_count",
                    &[&log_count.to_string(), &last_refresh],
                );
                ui.label(
                    RichText::new(status_text)
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_MUTED_COLOR),
                );
            });
        });

        let logs: Vec<_> = self.watcher.logs().iter().cloned().collect();

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            if logs.is_empty() {
                styles::empty_state(
                    ui,
                    "📭",
                    &i18n::t("file_log.no_logs"),
                    &i18n::t("file_log.no_logs_hint"),
                );
                return;
            }

            egui::Frame::NONE
                .fill(styles::BG_SECONDARY)
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
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
                        .header(styles::TABLE_HEADER_HEIGHT, |mut header| {
                            header.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        ui.label(styles::table_header_text(&i18n::t(
                                            "file_log.col_time",
                                        )));
                                    },
                                );
                            });
                            header.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        ui.label(styles::table_header_text(&i18n::t(
                                            "file_log.col_user",
                                        )));
                                    },
                                );
                            });
                            header.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        ui.label(styles::table_header_text(&i18n::t(
                                            "file_log.col_client",
                                        )));
                                    },
                                );
                            });
                            header.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        ui.label(styles::table_header_text(&i18n::t(
                                            "file_log.col_protocol",
                                        )));
                                    },
                                );
                            });
                            header.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        ui.label(styles::table_header_text(&i18n::t(
                                            "file_log.col_operation",
                                        )));
                                    },
                                );
                            });
                            header.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        ui.label(styles::table_header_text(&i18n::t(
                                            "file_log.col_size",
                                        )));
                                    },
                                );
                            });
                            header.col(|ui| {
                                ui.label(styles::table_header_text(&i18n::t(
                                    "file_log.col_file_path",
                                )));
                            });
                        })
                        .body(|mut body| {
                            for entry in logs {
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
                                                let client_ip = entry
                                                    .fields
                                                    .client_ip
                                                    .as_deref()
                                                    .unwrap_or("-");
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
                                                let operation = entry
                                                    .fields
                                                    .operation
                                                    .as_deref()
                                                    .unwrap_or("-");
                                                let success = entry.fields.success.unwrap_or(true);
                                                let translated_op =
                                                    Self::translate_operation(operation);
                                                let op_color = match operation {
                                                    "DELETE" | "RMDIR" => styles::DANGER_COLOR,
                                                    "UPLOAD" | "MKDIR" => styles::SUCCESS_COLOR,
                                                    "DOWNLOAD" => styles::INFO_COLOR,
                                                    "RENAME" | "COPY" | "MOVE" => {
                                                        styles::WARNING_COLOR
                                                    }
                                                    "UPDATE" => styles::TEXT_MUTED_COLOR,
                                                    _ => styles::TEXT_LABEL_COLOR,
                                                };
                                                let status_icon =
                                                    if success { "√" } else { "×" };
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
                                        let file_path =
                                            entry.fields.file_path.as_deref().unwrap_or("-");
                                        let cleaned_path = Self::clean_file_path(file_path);
                                        ui.label(
                                            RichText::new(&cleaned_path)
                                                .size(styles::FONT_SIZE_MD)
                                                .color(styles::TEXT_PRIMARY_COLOR),
                                        );
                                    });
                                });
                                styles::table_draw_row_separator(&mut body, 7);
                            }
                        });
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
