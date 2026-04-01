use egui::{Color32, RichText, Ui};
use crate::gui_egui::styles;

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

        // 模态框背景遮罩
        egui::Area::new(egui::Id::new("licenses_modal_backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(140));
            });

        let modal_width = (screen.width() * 0.6).clamp(500.0, 800.0);
        let modal_height = (screen.height() * 0.7).clamp(400.0, 600.0);
        let center = egui::pos2(screen.center().x, screen.center().y);

        egui::Window::new("开源软件声明")
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(center)
            .fixed_size([modal_width, modal_height])
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("本项目使用了以下开源组件：")
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_SECONDARY_COLOR),
                );
                ui.add_space(styles::SPACING_MD);

                // 开源组件列表
                let licenses = [
                    ("egui", "0.34.0", "MIT OR Apache-2.0", "即时模式GUI框架"),
                    ("eframe", "0.34.0", "MIT OR Apache-2.0", "egui应用程序框架"),
                    ("egui_extras", "0.34.0", "MIT OR Apache-2.0", "egui额外组件"),
                    ("rfd", "0.17.2", "MIT", "原生文件对话框"),
                    ("tokio", "1.x", "MIT", "异步运行时"),
                    ("serde", "1.x", "MIT OR Apache-2.0", "序列化框架"),
                    ("chrono", "0.4", "MIT OR Apache-2.0", "日期时间处理"),
                    ("anyhow", "1.x", "MIT OR Apache-2.0", "错误处理"),
                    ("russh", "0.58.1", "Apache-2.0", "SSH/SFTP服务器库"),
                    ("rsa", "0.9", "MIT OR Apache-2.0", "RSA加密算法"),
                    ("argon2", "0.5.3", "MIT OR Apache-2.0", "密码哈希算法"),
                    ("windows", "0.62", "MIT OR Apache-2.0", "Windows API绑定"),
                    ("tracing", "0.1", "MIT", "结构化日志框架"),
                    ("parking_lot", "0.12", "MIT OR Apache-2.0", "高性能同步原语"),
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
                    if ui
                        .add(styles::primary_button("关闭"))
                        .clicked()
                    {
                        self.show_licenses_modal = false;
                    }
                });
            });
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();

        self.show_licenses_modal(&ctx);

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
                    "4. 端口变更、FTP/SFTP主配置变动需要重启wftpd服务才能生效",
                    "5. 请确保防火墙允许 FTP/SFTP 端口通信",
                    "6. 当前不支持管理Windows上的符号链接",
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
                ui.add_space(styles::SPACING_MD);

                // 超链接样式的按钮
                let link_text = RichText::new("📋 开源软件声明")
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
