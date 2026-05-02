use crate::core::config::Config;
use crate::core::i18n;
use crate::gui_egui::log_watcher::LogFileWatcher;
use crate::gui_egui::styles;
use egui::{Color32, RichText};
use egui_extras::TableBuilder;
use std::path::PathBuf;

pub struct LogTab {
    watcher: LogFileWatcher,
    scroll_to_bottom: bool,
    user_at_bottom: bool,
    new_logs_count: usize,
}

impl LogTab {
    pub fn new() -> Self {
        let log_dir = Config::get_config_path()
            .parent()
            .map(|p| p.join("logs"))
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData\\wftpg\\logs"));

        let mut watcher = LogFileWatcher::new(log_dir, "wftpg.", |entry| {
            entry.fields.operation.is_none()
        });
        watcher.init();

        Self {
            watcher,
            scroll_to_bottom: true,
            user_at_bottom: true,
            new_logs_count: 0,
        }
    }

    fn format_last_refresh(&self) -> String {
        match self.watcher.last_refresh_time() {
            Some(t) => styles::format_elapsed_time(
                t.elapsed(),
                "log.n_seconds_ago",
                "log.n_minutes_ago",
                "log.n_hours_ago",
            ),
            None => i18n::t("log.not_refreshed"),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.watcher.check_events(&ctx);
        self.watcher.process_refresh();

        let log_count = self.watcher.logs().len();
        let last_refresh = self.format_last_refresh();

        ui.horizontal(|ui| {
            let refresh_label = i18n::t("log.refresh");
            let refresh_btn = styles::small_button(&refresh_label);

            if ui.add(refresh_btn).clicked() {
                self.watcher.request_refresh();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let status_text = i18n::t_fmt(
                    "log.total_count",
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
                    &i18n::t("log.no_logs"),
                    &i18n::t("log.no_logs_hint"),
                );
                return;
            }

            let available_width = ui.available_width();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(self.scroll_to_bottom)
                .id_salt("log_scroll_area")
                .show(ui, |ui| {
                    egui::Frame::NONE
                        .fill(styles::BG_SECONDARY)
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
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
                                .header(styles::TABLE_HEADER_HEIGHT, |mut header| {
                                    header.col(|ui| {
                                        ui.with_layout(
                                            egui::Layout::centered_and_justified(
                                                egui::Direction::LeftToRight,
                                            ),
                                            |ui| {
                                                ui.label(styles::table_header_text(&i18n::t(
                                                    "log.col_time",
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
                                                    "log.col_level",
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
                                                    "log.col_protocol",
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
                                                    "log.col_client",
                                                )));
                                            },
                                        );
                                    });
                                    header.col(|ui| {
                                        ui.label(styles::table_header_text(&i18n::t(
                                            "log.col_message",
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
                                                        let level_color = match entry.level {
                                                            crate::core::logger::LogLevel::Error => styles::DANGER_COLOR,
                                                            crate::core::logger::LogLevel::Warning => styles::WARNING_COLOR,
                                                            crate::core::logger::LogLevel::Debug => styles::TEXT_MUTED_COLOR,
                                                            _ => styles::SUCCESS_COLOR,
                                                        };
                                                        ui.label(
                                                            RichText::new(entry.level.to_string())
                                                                .size(styles::FONT_SIZE_MD)
                                                                .strong()
                                                                .color(level_color),
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
                                                        let protocol = entry
                                                            .fields
                                                            .protocol
                                                            .as_deref()
                                                            .unwrap_or("-");
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
                                                let translated_msg =
                                                    i18n::map_log(&entry.fields.message);
                                                ui.label(
                                                    RichText::new(&translated_msg)
                                                        .size(styles::FONT_SIZE_MD)
                                                        .color(styles::TEXT_PRIMARY_COLOR),
                                                );
                                                if let Some(user) = &entry.fields.username {
                                                    ui.label(
                                                        RichText::new(format!("({})", user))
                                                            .size(styles::FONT_SIZE_SM)
                                                            .color(styles::TEXT_MUTED_COLOR),
                                                    );
                                                }
                                            });
                                        });
                                        styles::table_draw_row_separator(&mut body, 5);
                                    }
                                });
                        });
                });

            ui.add_space(styles::SPACING_SM);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.scroll_to_bottom, i18n::t("log.auto_scroll"));

                if self.new_logs_count > 0 && !self.user_at_bottom {
                    let btn = egui::Button::new(
                        RichText::new(i18n::t_fmt(
                            "log.new_logs",
                            &[&self.new_logs_count.to_string()],
                        ))
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
