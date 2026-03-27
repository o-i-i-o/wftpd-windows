use egui::{RichText, Ui};
use crate::gui_egui::styles;

pub struct AboutTab;

impl Default for AboutTab {
    fn default() -> Self {
        Self::new()
    }
}

impl AboutTab {
    pub fn new() -> Self {
        Self
    }

    fn section_header(ui: &mut Ui, icon: &str, title: &str) {
        styles::section_header(ui, icon, title);
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            styles::page_header(ui, "ℹ", "关于");
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            Self::section_header(ui, "📦", "软件信息");

            ui.vertical(|ui| {
                ui.label(
                    RichText::new("WFTPG")
                        .size(styles::FONT_SIZE_XL)
                        .strong()
                        .color(styles::PRIMARY_COLOR),
                );
                ui.add_space(styles::SPACING_SM);
                ui.label(
                    RichText::new(format!("版本: {}", env!("CARGO_PKG_VERSION")))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR),
                );
                ui.label(
                    RichText::new("SFTP/FTP 服务器管理工具")
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR),
                );
            });
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            Self::section_header(ui, "👤", "作者信息");

            ui.vertical(|ui| {
                ui.label(
                    RichText::new("作者: 吴威富")
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_PRIMARY_COLOR),
                );
                ui.add_space(styles::SPACING_SM);
                    ui.label(
                    RichText::new("电子邮箱: boss@oi-io.cc")
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_PRIMARY_COLOR),
                );
                ui.add_space(styles::SPACING_SM);
                ui.label(
                    RichText::new("技术栈: Rust + egui")
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR),
                );
            });
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            Self::section_header(ui, "⚠", "注意事项");

            ui.vertical(|ui| {
                let notices = [
                    "1. 本软件需要管理员权限运行，用于管理 Windows 服务",
                    "2. 配置文件存储在 C:\\ProgramData\\wftpg\\ 目录",
                    "3. 修改配置后请保存，后台服务会自动重新加载",
                    "4. 端口变更需要重启服务才能生效",
                    "5. 请确保防火墙允许 FTP/SFTP 端口通信",
                ];

                for notice in &notices {
                    ui.label(
                        RichText::new(*notice)
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
            Self::section_header(ui, "📄", "开源协议");

            ui.vertical(|ui| {
                ui.label(
                    RichText::new("本软件基于 MIT 协议开源")
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR),
                );
                ui.add_space(styles::SPACING_SM);
                ui.label(
                    RichText::new("Copyright © 2026 WFTPG Contributors")
                        .size(styles::FONT_SIZE_SM)
                        .color(styles::TEXT_MUTED_COLOR),
                );
            });
        });
    }
}
