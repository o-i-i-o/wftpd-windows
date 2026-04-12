use crate::core::i18n;
use crate::gui_egui::styles;
use egui::{Color32, RichText, Ui};

pub struct AboutTab {
    show_licenses_modal: bool,
}

impl Default for AboutTab {
    fn default() -> Self {
        Self::new()
    }
}

impl AboutTab {
    pub fn new() -> Self {
        Self {
            show_licenses_modal: false,
        }
    }

    fn section_header(ui: &mut Ui, icon: &str, title: &str) {
        styles::section_header(ui, icon, title);
    }

    fn show_licenses_modal(&mut self, ctx: &egui::Context) {
        if !self.show_licenses_modal {
            return;
        }

        let screen = ctx.content_rect();
        if screen.width() <= 0.0 || screen.height() <= 0.0 {
            return;
        }

        egui::Area::new(egui::Id::new("licenses_modal_backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(140));
            });

        let modal_width = (screen.width() * 0.6).clamp(500.0, 800.0);
        let modal_height = (screen.height() * 0.7).clamp(400.0, 600.0);
        let center = egui::pos2(screen.center().x, screen.center().y);

        egui::Window::new(i18n::t("about.licenses_modal_title"))
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(center)
            .fixed_size([modal_width, modal_height])
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(i18n::t("about.licenses_intro"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR),
                );
                ui.add_space(styles::SPACING_MD);

                let licenses = [
                    ("egui", "0.34.0", "MIT OR Apache-2.0", "Immediate mode GUI framework"),
                    ("eframe", "0.34.0", "MIT OR Apache-2.0", "egui application framework"),
                    ("egui_extras", "0.34.0", "MIT OR Apache-2.0", "egui extra components"),
                    ("rfd", "0.17.2", "MIT", "Native file dialog"),
                    ("tokio", "1.x", "MIT", "Async runtime"),
                    ("serde", "1.x", "MIT OR Apache-2.0", "Serialization framework"),
                    ("chrono", "0.4", "MIT OR Apache-2.0", "Date time handling"),
                    ("anyhow", "1.x", "MIT OR Apache-2.0", "Error handling"),
                    ("russh", "0.58.1", "Apache-2.0", "SSH/SFTP server library"),
                    ("rsa", "0.9", "MIT OR Apache-2.0", "RSA encryption"),
                    ("argon2", "0.5.3", "MIT OR Apache-2.0", "Password hashing"),
                    ("windows", "0.62", "MIT OR Apache-2.0", "Windows API bindings"),
                    ("tracing", "0.1", "MIT", "Structured logging"),
                    ("parking_lot", "0.12", "MIT OR Apache-2.0", "High-performance sync primitives"),
                ];

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (name, version, license, desc) in &licenses {
                            styles::card_frame().show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(format!("{} {}", name, version))
                                                .size(styles::FONT_SIZE_MD)
                                                .strong()
                                                .color(styles::TEXT_PRIMARY_COLOR),
                                        );
                                        ui.label(
                                            RichText::new(*desc)
                                                .size(styles::FONT_SIZE_SM)
                                                .color(styles::TEXT_SECONDARY_COLOR),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(*license)
                                                    .size(styles::FONT_SIZE_SM)
                                                    .color(styles::PRIMARY_COLOR),
                                            );
                                        },
                                    );
                                });
                            });
                            ui.add_space(styles::SPACING_SM);
                        }
                    });

                ui.add_space(styles::SPACING_MD);
                ui.separator();
                ui.add_space(styles::SPACING_SM);

                ui.vertical_centered(|ui| {
                    if ui.add(styles::primary_button(&i18n::t("app.close"))).clicked() {
                        self.show_licenses_modal = false;
                    }
                });
            });
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();

        self.show_licenses_modal(&ctx);

        ui.horizontal(|ui| {
            styles::page_header(ui, "ℹ", &i18n::t("about.title"));
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            Self::section_header(ui, "📦", &i18n::t("about.software_info"));

            ui.vertical(|ui| {
                ui.label(
                    RichText::new("WFTPG")
                        .size(styles::FONT_SIZE_XL)
                        .strong()
                        .color(styles::PRIMARY_COLOR),
                );
                ui.add_space(styles::SPACING_SM);
                ui.label(
                    RichText::new(i18n::t_fmt("about.version", &[env!("CARGO_PKG_VERSION")]))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR),
                );
                ui.label(
                    RichText::new(i18n::t("about.description"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR),
                );
            });
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            Self::section_header(ui, "👤", &i18n::t("about.author_info"));

            ui.vertical(|ui| {
                ui.label(
                    RichText::new(i18n::t("about.author"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_PRIMARY_COLOR),
                );
                ui.add_space(styles::SPACING_SM);
                ui.label(
                    RichText::new(i18n::t("about.email"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_PRIMARY_COLOR),
                );
                ui.add_space(styles::SPACING_SM);
                ui.label(
                    RichText::new(i18n::t("about.tech_stack"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR),
                );
            });
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            Self::section_header(ui, "⚠", &i18n::t("about.notices_title"));

            ui.vertical(|ui| {
                let notices = [
                    i18n::t("about.notice_1"),
                    i18n::t("about.notice_2"),
                    i18n::t("about.notice_3"),
                    i18n::t("about.notice_4"),
                    i18n::t("about.notice_5"),
                    i18n::t("about.notice_6"),
                ];

                for notice in &notices {
                    ui.label(
                        RichText::new(notice)
                            .size(styles::FONT_SIZE_MD)
                            .color(styles::TEXT_SECONDARY_COLOR),
                    );
                    ui.add_space(styles::SPACING_SM);
                }
            });
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            Self::section_header(ui, "🌐", &i18n::t("about.language"));

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

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            Self::section_header(ui, "📄", &i18n::t("about.license_title"));

            ui.vertical(|ui| {
                ui.label(
                    RichText::new(i18n::t("about.license_desc"))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR),
                );
                ui.add_space(styles::SPACING_SM);
                ui.label(
                    RichText::new(i18n::t("about.copyright"))
                        .size(styles::FONT_SIZE_SM)
                        .color(styles::TEXT_MUTED_COLOR),
                );
                ui.add_space(styles::SPACING_MD);

                let link_text = RichText::new(i18n::t("about.view_licenses"))
                    .size(styles::FONT_SIZE_MD)
                    .color(styles::PRIMARY_COLOR)
                    .underline();

                let link_btn = egui::Button::new(link_text)
                    .fill(Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(4));

                if ui.add(link_btn).clicked() {
                    self.show_licenses_modal = true;
                }
            });
        });
    }
}

fn save_gui_language(lang: i18n::Language) {
    use std::fs;
    use std::io::Write;

    let config_dir = crate::core::config::Config::get_config_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("C:\\ProgramData\\wftpg"));

    let lang_file = config_dir.join("gui_language.txt");
    if let Ok(mut file) = fs::File::create(&lang_file) {
        let _ = file.write_all(lang.code().as_bytes());
    }
}
