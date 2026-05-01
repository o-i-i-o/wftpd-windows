use crate::core::config::Config;
use crate::core::config_manager::ConfigManager;
use crate::core::i18n;
use crate::core::ipc::IpcClient;
use crate::gui_egui::styles;
use egui::{RichText, Ui};
use std::sync::mpsc;

#[derive(Debug)]
pub struct OtherTab {
    config_manager: ConfigManager,
    status_message: Option<(String, bool)>,
    save_receiver: Option<mpsc::Receiver<Result<String, String>>>,
    is_saving: bool,
}

impl OtherTab {
    pub fn new(config_manager: ConfigManager) -> Self {
        Self {
            config_manager,
            status_message: None,
            save_receiver: None,
            is_saving: false,
        }
    }

    fn validate_config(config: &Config) -> Vec<String> {
        let mut errors = Vec::new();
        if config.logging.log_dir.trim().is_empty() {
            errors.push(i18n::t("server.log_dir_empty"));
        }
        if config.logging.max_log_files == 0 {
            errors.push(i18n::t("server.max_log_files_zero"));
        }
        errors
    }

    pub fn save_config_async(&mut self, ctx: &egui::Context, config: Config) {
        if self.is_saving {
            return;
        }

        let validation_errors = Self::validate_config(&config);
        if !validation_errors.is_empty() {
            self.status_message = Some((
                i18n::t_fmt(
                    "server.config_validation_failed",
                    &[&validation_errors.join("\n")],
                ),
                false,
            ));
            self.is_saving = false;
            return;
        }

        self.is_saving = true;

        let config_manager = self.config_manager.clone();
        let (tx, rx) = mpsc::channel();
        self.save_receiver = Some(rx);

        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            config_manager.modify(|c| *c = config.clone());

            let result = match config_manager.save(&Config::get_config_path()) {
                Ok(_) => {
                    tracing::info!("Log config saved successfully");

                    if IpcClient::is_server_running() {
                        match IpcClient::notify_reload() {
                            Ok(response) => {
                                if response.success {
                                    Ok(i18n::t("server.config_saved"))
                                } else {
                                    Ok(i18n::t_fmt(
                                        "server.config_saved_reload_failed",
                                        &[&response.message],
                                    ))
                                }
                            }
                            Err(e) => Ok(i18n::t_fmt(
                                "server.config_saved_notify_failed",
                                &[&e],
                            )),
                        }
                    } else {
                        Ok(i18n::t("server.config_saved_not_running"))
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to save log config: {}", e);
                    Err(i18n::t_fmt("server.config_save_failed", &[&e]))
                }
            };

            if let Err(e) = tx.send(result) {
                tracing::debug!("Failed to send log config save result: {}", e);
            }
            ctx_clone.request_repaint();
        });
    }

    fn check_save_result(&mut self) {
        if let Some(rx) = &self.save_receiver
            && let Ok(result) = rx.try_recv()
        {
            self.save_receiver = None;
            self.is_saving = false;

            match result {
                Ok(msg) => {
                    self.status_message = Some((msg, true));
                }
                Err(e) => {
                    self.status_message = Some((e, false));
                }
            }
        }
    }

    fn pick_folder(title: &str) -> Option<std::path::PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.check_save_result();

        let is_saving = self.is_saving;
        let mut config_to_save: Option<Config> = None;
        let ctx = ui.ctx().clone();

        self.config_manager.modify(|config| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save_text = i18n::t("server.save_config");
                    let save_btn = if is_saving {
                        egui::Button::new(
                            RichText::new(i18n::t("server.saving"))
                                .size(styles::FONT_SIZE_MD),
                        )
                        .fill(styles::BG_SECONDARY)
                        .corner_radius(egui::CornerRadius::same(6))
                    } else {
                        styles::primary_button(&save_text)
                    };

                    if ui.add(save_btn).clicked() && !is_saving {
                        config_to_save = Some(config.clone());
                    }

                    if let Some((msg, success)) = &self.status_message {
                        let msg_text = if *success {
                            RichText::new(msg)
                                .color(styles::SUCCESS_COLOR)
                                .size(styles::FONT_SIZE_SM)
                        } else {
                            RichText::new(msg)
                                .color(styles::DANGER_COLOR)
                                .size(styles::FONT_SIZE_SM)
                        };
                        ui.label(msg_text);
                    }
                });
            });

            ui.add_space(styles::SPACING_MD);

            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                styles::section_header(ui, "📋", &i18n::t("server.global_log_settings"));

                let available_width = ui.available_width();
                let label_width = (available_width * 0.15).clamp(100.0, 160.0);

                let mut log_dir = config.logging.log_dir.clone();
                styles::form_row(ui, &i18n::t("server.log_dir"), label_width, |ui| {
                    styles::input_frame().show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut log_dir)
                                .desired_width(ui.available_width() - 80.0)
                                .font(egui::FontId::new(
                                    styles::FONT_SIZE_MD,
                                    egui::FontFamily::Proportional,
                                )),
                        );
                    });
                    if ui.button(i18n::t("server.browse")).clicked()
                        && let Some(path) = Self::pick_folder(&i18n::t("server.select_log_dir"))
                    {
                        log_dir = path.to_string_lossy().to_string();
                    }
                });
                config.logging.log_dir = log_dir;

                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("server.log_dir_hint"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });

                styles::form_row(ui, &i18n::t("server.log_level"), label_width, |ui| {
                    let levels = ["trace", "debug", "info", "warn", "error"];
                    egui::ComboBox::from_id_salt("other_log_level")
                        .selected_text(&config.logging.log_level)
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            for level in levels {
                                ui.selectable_value(
                                    &mut config.logging.log_level,
                                    level.to_string(),
                                    level,
                                );
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.add_sized([label_width, 24.0], egui::Label::new(""));
                    ui.label(
                        RichText::new(i18n::t("server.log_level_hint"))
                            .size(styles::FONT_SIZE_SM)
                            .color(styles::TEXT_MUTED_COLOR)
                            .italics(),
                    );
                });

                styles::form_row_with_suffix(
                    ui,
                    &i18n::t("server.max_log_files"),
                    label_width,
                    |ui| {
                        let mut files_str = config.logging.max_log_files.to_string();
                        styles::input_frame().show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut files_str)
                                    .desired_width(80.0)
                                    .font(egui::FontId::new(
                                        styles::FONT_SIZE_MD,
                                        egui::FontFamily::Proportional,
                                    )),
                            );
                        });
                        if let Ok(v) = files_str.parse::<usize>() {
                            config.logging.max_log_files = v;
                        }
                    },
                    &i18n::t("server.max_log_files_hint"),
                );

                ui.add_space(styles::SPACING_SM);

                ui.label(
                    RichText::new(i18n::t("server.notes"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR)
                        .strong(),
                );

                egui::Frame::NONE
                    .fill(styles::BG_INFO)
                    .inner_margin(egui::Margin::same(12))
                    .corner_radius(egui::CornerRadius::same(6))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(i18n::t("server.note_1"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                            ui.label(
                                RichText::new(i18n::t("server.note_2"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                            ui.label(
                                RichText::new(i18n::t("server.note_3"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                            ui.label(
                                RichText::new(i18n::t("server.note_4"))
                                    .size(styles::FONT_SIZE_SM)
                                    .color(styles::TEXT_LABEL_COLOR),
                            );
                        });
                    });
            });

            ui.add_space(styles::SPACING_MD);

            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                styles::section_header(ui, "🌐", &i18n::t("about.language"));

                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(i18n::t("about.language_hint"))
                            .size(styles::FONT_SIZE_MD)
                            .color(styles::TEXT_SECONDARY_COLOR),
                    );
                    ui.add_space(styles::SPACING_SM);

                    ui.horizontal(|ui| {
                        for lang in i18n::Language::all() {
                            let is_current = *lang == i18n::current_language();
                            let btn = if is_current {
                                styles::primary_button(lang.display_name())
                            } else {
                                styles::secondary_button(lang.display_name())
                            };
                            if ui.add(btn).clicked() {
                                i18n::set_language(*lang);
                                save_gui_language(*lang);
                            }
                        }
                    });
                });
            });
        });

        if let Some(config) = config_to_save {
            self.save_config_async(&ctx, config);
        }
    }
}

fn save_gui_language(lang: i18n::Language) {
    let path = crate::core::config::get_program_data_path().join("gui_config.json");
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("Failed to create config directory: {}", e);
    }
    let json = serde_json::json!({ "language": lang.code() });
    if let Err(e) = std::fs::write(&path, json.to_string()) {
        tracing::warn!("Failed to save language preference: {}", e);
    }
}
