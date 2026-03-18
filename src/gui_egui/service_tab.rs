use egui::{Color32, RichText, Ui};
use crate::core::server_manager::ServerManager;
use crate::gui_egui::styles;

pub struct ServiceTab {
    manager: ServerManager,
    status_message: Option<(String, bool)>,
    last_check: std::time::Instant,
    is_installed: bool,
    is_running: bool,
    confirming_uninstall: bool,
}

impl Default for ServiceTab {
    fn default() -> Self {
        let manager = ServerManager::new();
        let is_installed = manager.is_service_installed();
        let is_running = manager.is_service_running();
        Self {
            manager,
            status_message: None,
            last_check: std::time::Instant::now(),
            is_installed,
            is_running,
            confirming_uninstall: false,
        }
    }
}

impl ServiceTab {
    pub fn new() -> Self { Self::default() }

    fn refresh_status(&mut self) {
        self.is_installed = self.manager.is_service_installed();
        self.is_running = self.manager.is_service_running();
        self.last_check = std::time::Instant::now();
    }

    fn set_ok(&mut self, msg: &str) {
        self.status_message = Some((format!("✓ {}", msg), true));
        self.refresh_status();
    }

    fn set_err(&mut self, msg: String) {
        self.status_message = Some((format!("✗ {}", msg), false));
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        // 每 2 秒自动刷新一次状态
        if self.last_check.elapsed().as_secs() >= 2 {
            self.refresh_status();
        }

        ui.heading(RichText::new("🖥 系统服务管理").color(styles::TEXT_PRIMARY_COLOR));
        ui.separator();
        ui.add_space(4.0);

        // 状态消息
        if let Some((msg, ok)) = &self.status_message.clone() {
            let color = if *ok { Color32::from_rgb(39, 174, 96) } else { Color32::from_rgb(192, 57, 43) };
            egui::Frame::new()
                .fill(if *ok { Color32::from_rgb(240, 255, 245) } else { Color32::from_rgb(255, 243, 243) })
                .stroke(egui::Stroke::new(1.0, color))
                .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 })
                .corner_radius(egui::CornerRadius::same(4))
                .show(ui, |ui| {
                    ui.label(RichText::new(msg).color(color).size(13.0));
                });
            ui.add_space(8.0);
        }

        // 服务信息卡片
        egui::Frame::new()
            .fill(Color32::from_rgb(248, 250, 252))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(220, 225, 230)))
            .inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 12 })
            .corner_radius(egui::CornerRadius::same(6))
            .show(ui, |ui| {
                ui.label(RichText::new("服务信息").strong().size(14.0).color(styles::TEXT_PRIMARY_COLOR));
                ui.add_space(8.0);

                egui::Grid::new("svc_info")
                    .num_columns(2)
                    .spacing([20.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("服务名称").color(Color32::from_rgb(100, 100, 100)));
                        ui.label(RichText::new("wftpd").strong());
                        ui.end_row();

                        ui.label(RichText::new("显示名称").color(Color32::from_rgb(100, 100, 100)));
                        ui.label(RichText::new("WFTPD SFTP/FTP Server").strong());
                        ui.end_row();

                        ui.label(RichText::new("安装状态").color(Color32::from_rgb(100, 100, 100)));
                        let (inst_txt, inst_col) = if self.is_installed {
                            ("● 已安装", Color32::from_rgb(39, 174, 96))
                        } else {
                            ("● 未安装", Color32::from_rgb(192, 57, 43))
                        };
                        ui.label(RichText::new(inst_txt).color(inst_col).strong());
                        ui.end_row();

                        ui.label(RichText::new("运行状态").color(Color32::from_rgb(100, 100, 100)));
                        let (run_txt, run_col) = if self.is_running {
                            ("● 运行中", Color32::from_rgb(39, 174, 96))
                        } else {
                            ("● 已停止", Color32::from_rgb(192, 57, 43))
                        };
                        ui.label(RichText::new(run_txt).color(run_col).strong());
                        ui.end_row();
                    });
            });

        ui.add_space(12.0);

        // 操作按钮区
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(220, 225, 230)))
            .inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 12 })
            .corner_radius(egui::CornerRadius::same(6))
            .show(ui, |ui| {
                ui.label(RichText::new("服务操作").strong().size(14.0).color(styles::TEXT_PRIMARY_COLOR));
                ui.add_space(10.0);

                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    // 刷新按钮始终可用
                    if ui.button("🔄 刷新状态").clicked() {
                        self.refresh_status();
                    }

                    ui.separator();

                    if !self.is_installed {
                        // 安装
                        let btn = egui::Button::new(
                            RichText::new("📦 安装服务").color(Color32::WHITE)
                        ).fill(Color32::from_rgb(41, 128, 185));
                        if ui.add(btn).clicked() {
                            match self.manager.install_service() {
                                Ok(_) => self.set_ok("服务安装成功，开机将自动启动"),
                                Err(e) => self.set_err(format!("安装失败: {} （需要管理员权限）", e)),
                            }
                        }
                    } else {
                        // 启动
                        if !self.is_running {
                            let btn = egui::Button::new(
                                RichText::new("▶ 启动服务").color(Color32::WHITE)
                            ).fill(Color32::from_rgb(39, 174, 96));
                            if ui.add(btn).clicked() {
                                match self.manager.start_service() {
                                    Ok(_) => self.set_ok("服务已启动"),
                                    Err(e) => self.set_err(format!("启动失败: {}", e)),
                                }
                            }
                        } else {
                            // 停止
                            let btn = egui::Button::new(
                                RichText::new("⏹ 停止服务").color(Color32::WHITE)
                            ).fill(Color32::from_rgb(230, 126, 34));
                            if ui.add(btn).clicked() {
                                match self.manager.stop_service() {
                                    Ok(_) => self.set_ok("服务已停止"),
                                    Err(e) => self.set_err(format!("停止失败: {}", e)),
                                }
                            }
                        }

                        ui.separator();

                        // 卸载（二次确认）
                        if self.confirming_uninstall {
                            ui.label(RichText::new("确认卸载?").color(Color32::from_rgb(192, 57, 43)));
                            let yes = egui::Button::new(
                                RichText::new("确认").color(Color32::WHITE)
                            ).fill(Color32::from_rgb(192, 57, 43));
                            if ui.add(yes).clicked() {
                                self.confirming_uninstall = false;
                                match self.manager.uninstall_service() {
                                    Ok(_) => self.set_ok("服务已卸载"),
                                    Err(e) => self.set_err(format!("卸载失败: {}", e)),
                                }
                            }
                            if ui.button("取消").clicked() {
                                self.confirming_uninstall = false;
                            }
                        } else {
                            let uninstall_btn = egui::Button::new(
                                RichText::new("🗑 卸载服务").color(Color32::from_rgb(192, 57, 43))
                            );
                            if ui.add(uninstall_btn).clicked() {
                                self.confirming_uninstall = true;
                            }
                        }
                    }
                });
            });

        ui.add_space(12.0);

        // 说明
        egui::Frame::new()
            .fill(Color32::from_rgb(255, 252, 235))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(241, 196, 15)))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.label(RichText::new("⚠ 注意事项").strong().color(Color32::from_rgb(180, 120, 0)));
                ui.add_space(4.0);
                ui.label("• 安装/卸载服务需要以管理员身份运行本程序");
                ui.label("• 服务安装后将设为开机自动启动（AutoStart）");
                ui.label("• 服务以 SYSTEM 账户运行，配置文件位于 ProgramData\\wftpg\\");
                ui.label("• 停止服务会断开所有当前活动的 FTP/SFTP 连接");
            });
    }
}
