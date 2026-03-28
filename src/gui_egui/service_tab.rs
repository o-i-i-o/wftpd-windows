use egui::{Color32, RichText, Ui};
use crate::core::server_manager::ServerManager;
use crate::gui_egui::styles;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
enum OperationState {
    Idle,
    Installing,
    Starting,
    Stopping,
    Restarting,
    Uninstalling,
}

#[derive(Debug, Clone)]
enum OperationResult {
    Success(String),
    Error(String),
}

pub struct ServiceTab {
    manager: ServerManager,
    status_message: Option<(String, bool)>,
    last_check: std::time::Instant,
    is_installed: bool,
    is_running: bool,
    confirming_uninstall: bool,
    operation_state: OperationState,
    operation_receiver: Option<mpsc::Receiver<OperationResult>>,
    operation_start_time: Option<Instant>,
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
            operation_state: OperationState::Idle,
            operation_receiver: None,
            operation_start_time: None,
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
        self.status_message = Some((msg.to_string(), true));
        self.refresh_status();
    }

    fn set_err(&mut self, msg: String) {
        self.status_message = Some((msg, false));
    }

    /// 检查异步操作结果
    fn check_operation_result(&mut self) {
        if self.operation_state == OperationState::Idle {
            return;
        }

        // 检查超时（30 秒）
        if let Some(start_time) = self.operation_start_time {
            if start_time.elapsed() >= Duration::from_secs(30) {
                self.operation_state = OperationState::Idle;
                self.operation_receiver = None;
                self.operation_start_time = None;
                self.set_err("操作超时，请稍后重试".to_string());
                return;
            }
        }

        // 检查操作完成
        if let Some(rx) = &self.operation_receiver {
            if let Ok(result) = rx.try_recv() {
                match result {
                    OperationResult::Success(msg) => {
                        self.set_ok(&msg);
                    }
                    OperationResult::Error(msg) => {
                        self.set_err(msg);
                    }
                }
                self.operation_state = OperationState::Idle;
                self.operation_receiver = None;
                self.operation_start_time = None;
            }
        }
    }

    /// 异步安装服务
    fn install_service_async(&mut self, ctx: &egui::Context) {
        self.operation_state = OperationState::Installing;
        self.operation_start_time = Some(Instant::now());
        
        let (tx, rx) = mpsc::channel();
        self.operation_receiver = Some(rx);
        
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let manager = ServerManager::new();
                manager.install_service()
            })) {
                Ok(Ok(_)) => OperationResult::Success("服务安装成功，开机将自动启动".to_string()),
                Ok(Err(e)) => OperationResult::Error(format!("安装失败：{}（需要管理员权限）", e)),
                Err(_) => OperationResult::Error("安装过程中发生未知错误".to_string()),
            };
            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
    }

    /// 异步启动服务
    fn start_service_async(&mut self, ctx: &egui::Context) {
        self.operation_state = OperationState::Starting;
        self.operation_start_time = Some(Instant::now());
        
        let (tx, rx) = mpsc::channel();
        self.operation_receiver = Some(rx);
        
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let manager = ServerManager::new();
                manager.start_service()
            })) {
                Ok(Ok(_)) => OperationResult::Success("服务已启动".to_string()),
                Ok(Err(e)) => OperationResult::Error(format!("启动失败：{}", e)),
                Err(_) => OperationResult::Error("启动过程中发生未知错误".to_string()),
            };
            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
    }

    /// 异步停止服务
    fn stop_service_async(&mut self, ctx: &egui::Context) {
        self.operation_state = OperationState::Stopping;
        self.operation_start_time = Some(Instant::now());
        
        let (tx, rx) = mpsc::channel();
        self.operation_receiver = Some(rx);
        
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let manager = ServerManager::new();
                manager.stop_service()
            })) {
                Ok(Ok(_)) => OperationResult::Success("服务已停止".to_string()),
                Ok(Err(e)) => OperationResult::Error(format!("停止失败：{}", e)),
                Err(_) => OperationResult::Error("停止过程中发生未知错误".to_string()),
            };
            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
    }

    /// 异步重启服务
    fn restart_service_async(&mut self, ctx: &egui::Context) {
        self.operation_state = OperationState::Restarting;
        self.operation_start_time = Some(Instant::now());
        
        let (tx, rx) = mpsc::channel();
        self.operation_receiver = Some(rx);
        
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let manager = ServerManager::new();
                manager.restart_service()
            })) {
                Ok(Ok(_)) => OperationResult::Success("服务已重启".to_string()),
                Ok(Err(e)) => OperationResult::Error(format!("重启失败：{}", e)),
                Err(_) => OperationResult::Error("重启过程中发生未知错误".to_string()),
            };
            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
    }

    /// 异步卸载服务
    fn uninstall_service_async(&mut self, ctx: &egui::Context) {
        self.operation_state = OperationState::Uninstalling;
        self.operation_start_time = Some(Instant::now());
        
        let (tx, rx) = mpsc::channel();
        self.operation_receiver = Some(rx);
        
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let manager = ServerManager::new();
                manager.uninstall_service()
            })) {
                Ok(Ok(_)) => OperationResult::Success("服务已卸载".to_string()),
                Ok(Err(e)) => OperationResult::Error(format!("卸载失败：{}", e)),
                Err(_) => OperationResult::Error("卸载过程中发生未知错误".to_string()),
            };
            let _ = tx.send(result);
            ctx_clone.request_repaint();
        });
    }

    fn section_header(&self, ui: &mut Ui, icon: &str, title: &str) {
        styles::section_header(ui, icon, title);
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        // 检查异步操作结果
        self.check_operation_result();
        
        // 定期刷新状态
        if self.last_check.elapsed().as_secs() >= 2 {
            self.refresh_status();
        }

        ui.horizontal(|ui| {
            styles::page_header(ui, "🖥", "系统服务管理");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((msg, ok)) = &self.status_message.clone() {
                    styles::status_message(ui, msg, *ok);
                }
            });
        });

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.section_header(ui, "📋", "服务信息");
            
            let available_width = ui.available_width();
            let label_width = (available_width * 0.25).clamp(80.0, 120.0);
            
            egui::Grid::new("svc_info")
                .num_columns(2)
                .spacing([20.0, 8.0])
                .min_col_width(label_width)
                .show(ui, |ui| {
                    ui.label(RichText::new("服务名称").size(styles::FONT_SIZE_MD).color(styles::TEXT_LABEL_COLOR));
                    ui.label(RichText::new("wftpd").size(styles::FONT_SIZE_MD).strong().color(styles::TEXT_PRIMARY_COLOR));
                    ui.end_row();

                    ui.label(RichText::new("显示名称").size(styles::FONT_SIZE_MD).color(styles::TEXT_LABEL_COLOR));
                    ui.label(RichText::new("WFTPD SFTP/FTP Server").size(styles::FONT_SIZE_MD).strong().color(styles::TEXT_PRIMARY_COLOR));
                    ui.end_row();

                    ui.label(RichText::new("安装状态").size(styles::FONT_SIZE_MD).color(styles::TEXT_LABEL_COLOR));
                    let (inst_txt, inst_col) = if self.is_installed {
                        ("● 已安装", styles::SUCCESS_DARK)
                    } else {
                        ("● 未安装", styles::DANGER_DARK)
                    };
                    ui.label(RichText::new(inst_txt).size(styles::FONT_SIZE_MD).color(inst_col).strong());
                    ui.end_row();

                    ui.label(RichText::new("运行状态").size(styles::FONT_SIZE_MD).color(styles::TEXT_LABEL_COLOR));
                    let (run_txt, run_col) = if self.is_running {
                        ("● 运行中", styles::SUCCESS_DARK)
                    } else {
                        ("● 已停止", styles::DANGER_DARK)
                    };
                    ui.label(RichText::new(run_txt).size(styles::FONT_SIZE_MD).color(run_col).strong());
                    ui.end_row();
                });
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.section_header(ui, "⚙", "服务操作");

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                if ui.button("🔄 刷新状态").clicked() {
                    self.refresh_status();
                }

                ui.separator();

                if !self.is_installed {
                    let btn_text = match self.operation_state {
                        OperationState::Installing => "📦 安装中...",
                        _ => "📦 安装服务",
                    };
                    let btn = egui::Button::new(
                        RichText::new(btn_text).color(Color32::WHITE).size(styles::FONT_SIZE_MD)
                    ).fill(styles::INFO_COLOR)
                     .corner_radius(egui::CornerRadius::same(6));
                    
                    let btn_response = ui.add_enabled(self.operation_state == OperationState::Idle, btn);
                    if btn_response.clicked() {
                        self.install_service_async(ui.ctx());
                    }
                } else {
                    if !self.is_running {
                        let btn_text = match self.operation_state {
                            OperationState::Starting => "▶ 启动中...",
                            _ => "▶ 启动服务",
                        };
                        let btn = egui::Button::new(
                            RichText::new(btn_text).color(Color32::WHITE).size(styles::FONT_SIZE_MD)
                        ).fill(styles::SUCCESS_DARK)
                         .corner_radius(egui::CornerRadius::same(6));
                        
                        let btn_response = ui.add_enabled(self.operation_state == OperationState::Idle, btn);
                        if btn_response.clicked() {
                            self.start_service_async(ui.ctx());
                        }
                    } else {
                        let btn_text = match self.operation_state {
                            OperationState::Stopping => "⏹ 停止中...",
                            _ => "⏹ 停止服务",
                        };
                        let btn = egui::Button::new(
                            RichText::new(btn_text).color(Color32::WHITE).size(styles::FONT_SIZE_MD)
                        ).fill(Color32::from_rgb(230, 126, 34))
                         .corner_radius(egui::CornerRadius::same(6));
                        
                        let btn_response = ui.add_enabled(self.operation_state == OperationState::Idle, btn);
                        if btn_response.clicked() {
                            self.stop_service_async(ui.ctx());
                        }

                        ui.separator();

                        // 重启服务按钮
                        let btn_text = match self.operation_state {
                            OperationState::Restarting => "🔄 重启中...",
                            _ => "🔄 重启服务",
                        };
                        let restart_btn = egui::Button::new(
                            RichText::new(btn_text).color(Color32::WHITE).size(styles::FONT_SIZE_MD)
                        ).fill(styles::INFO_COLOR)
                         .corner_radius(egui::CornerRadius::same(6));
                        
                        let restart_btn_response = ui.add_enabled(self.operation_state == OperationState::Idle, restart_btn);
                        if restart_btn_response.clicked() {
                            self.restart_service_async(ui.ctx());
                        }
                    }

                    ui.separator();

                    if self.confirming_uninstall {
                        ui.label(RichText::new("确认卸载？").size(styles::FONT_SIZE_MD).color(styles::DANGER_DARK));
                        
                        let can_operate = self.operation_state == OperationState::Idle;
                        
                        let yes_btn = egui::Button::new(
                            RichText::new("确认").color(Color32::WHITE).size(styles::FONT_SIZE_MD)
                        ).fill(styles::DANGER_DARK)
                         .corner_radius(egui::CornerRadius::same(6));
                        
                        let yes_response = ui.add_enabled(can_operate, yes_btn);
                        if yes_response.clicked() {
                            self.confirming_uninstall = false;
                            self.uninstall_service_async(ui.ctx());
                        }
                        if ui.button("取消").clicked() {
                            self.confirming_uninstall = false;
                        }
                    } else {
                        let uninstall_btn_text = match self.operation_state {
                            OperationState::Uninstalling => "🗑 卸载中...",
                            _ => "🗑 卸载服务",
                        };
                        let uninstall_btn = egui::Button::new(
                            RichText::new(uninstall_btn_text).size(styles::FONT_SIZE_MD).color(styles::DANGER_DARK)
                        ).fill(styles::DANGER_LIGHT)
                         .corner_radius(egui::CornerRadius::same(6));
                        
                        let uninstall_btn_response = ui.add_enabled(self.operation_state == OperationState::Idle, uninstall_btn);
                        if uninstall_btn_response.clicked() {
                            self.confirming_uninstall = true;
                        }
                    }
                }
            });
        });

        ui.add_space(styles::SPACING_MD);

        styles::warning_box(ui, "注意事项", &[
            "安装/卸载服务需要以管理员身份运行本程序",
            "服务安装后将设为开机自动启动（AutoStart）",
            "服务以 SYSTEM 账户运行，配置文件位于 ProgramData\\wftpg\\",
            "停止服务会断开所有当前活动的 FTP/SFTP 连接",
        ]);
    }
}
