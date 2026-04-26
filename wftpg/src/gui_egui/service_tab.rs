use crate::core::i18n;
use crate::core::server_manager::ServerManager;
use crate::gui_egui::styles;
use egui::{Color32, RichText, Ui};
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
    pub fn new() -> Self {
        Self::default()
    }

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

    fn check_operation_result(&mut self) {
        if self.operation_state == OperationState::Idle {
            return;
        }

        if let Some(start_time) = self.operation_start_time
            && start_time.elapsed() >= Duration::from_secs(30)
        {
            self.operation_state = OperationState::Idle;
            self.operation_receiver = None;
            self.operation_start_time = None;
            self.set_err(i18n::t("service.operation_timeout"));
            return;
        }

        if let Some(rx) = &self.operation_receiver
            && let Ok(result) = rx.try_recv()
        {
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
                Ok(Ok(_)) => OperationResult::Success(i18n::t("service.install_success")),
                Ok(Err(e)) => {
                    OperationResult::Error(i18n::t_fmt("service.install_failed", &[&e.to_string()]))
                }
                Err(_) => OperationResult::Error(i18n::t("service.install_unknown_error")),
            };
            if let Err(e) = tx.send(result) {
                tracing::debug!("Failed to send service install result: {}", e);
            }
            ctx_clone.request_repaint();
        });
    }

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
                Ok(Ok(_)) => OperationResult::Success(i18n::t("service.start_success")),
                Ok(Err(e)) => {
                    OperationResult::Error(i18n::t_fmt("service.start_failed", &[&e.to_string()]))
                }
                Err(_) => OperationResult::Error(i18n::t("service.start_unknown_error")),
            };
            if let Err(e) = tx.send(result) {
                tracing::debug!("Failed to send service start result: {}", e);
            }
            ctx_clone.request_repaint();
        });
    }

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
                Ok(Ok(_)) => OperationResult::Success(i18n::t("service.stop_success")),
                Ok(Err(e)) => {
                    OperationResult::Error(i18n::t_fmt("service.stop_failed", &[&e.to_string()]))
                }
                Err(_) => OperationResult::Error(i18n::t("service.stop_unknown_error")),
            };
            if let Err(e) = tx.send(result) {
                tracing::debug!("Failed to send service stop result: {}", e);
            }
            ctx_clone.request_repaint();
        });
    }

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
                Ok(Ok(_)) => OperationResult::Success(i18n::t("service.restart_success")),
                Ok(Err(e)) => {
                    OperationResult::Error(i18n::t_fmt("service.restart_failed", &[&e.to_string()]))
                }
                Err(_) => OperationResult::Error(i18n::t("service.restart_unknown_error")),
            };
            if let Err(e) = tx.send(result) {
                tracing::debug!("Failed to send service restart result: {}", e);
            }
            ctx_clone.request_repaint();
        });
    }

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
                Ok(Ok(_)) => OperationResult::Success(i18n::t("service.uninstall_success")),
                Ok(Err(e)) => OperationResult::Error(i18n::t_fmt(
                    "service.uninstall_failed",
                    &[&e.to_string()],
                )),
                Err(_) => OperationResult::Error(i18n::t("service.uninstall_unknown_error")),
            };
            if let Err(e) = tx.send(result) {
                tracing::debug!("Failed to send service uninstall result: {}", e);
            }
            ctx_clone.request_repaint();
        });
    }

    fn section_header(&self, ui: &mut Ui, icon: &str, title: &str) {
        styles::section_header(ui, icon, title);
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.check_operation_result();

        if self.last_check.elapsed().as_secs() >= 5 {
            self.refresh_status();
        }

        ui.horizontal(|ui| {
            styles::page_header(ui, "🖥", &i18n::t("service.title"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((msg, ok)) = &self.status_message {
                    styles::status_message(ui, msg, *ok);
                }
            });
        });

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.section_header(ui, "📋", &i18n::t("service.service_info"));

            let available_width = ui.available_width();
            let label_width = (available_width * 0.25).clamp(80.0, 120.0);

            egui::Grid::new("svc_info")
                .num_columns(2)
                .spacing([20.0, 8.0])
                .min_col_width(label_width)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(i18n::t("service.service_name"))
                            .size(styles::FONT_SIZE_MD)
                            .color(styles::TEXT_LABEL_COLOR),
                    );
                    ui.label(
                        RichText::new("wftpd")
                            .size(styles::FONT_SIZE_MD)
                            .strong()
                            .color(styles::TEXT_PRIMARY_COLOR),
                    );
                    ui.end_row();

                    ui.label(
                        RichText::new(i18n::t("service.display_name"))
                            .size(styles::FONT_SIZE_MD)
                            .color(styles::TEXT_LABEL_COLOR),
                    );
                    ui.label(
                        RichText::new("WFTPD SFTP/FTP Server")
                            .size(styles::FONT_SIZE_MD)
                            .strong()
                            .color(styles::TEXT_PRIMARY_COLOR),
                    );
                    ui.end_row();

                    ui.label(
                        RichText::new(i18n::t("service.install_status"))
                            .size(styles::FONT_SIZE_MD)
                            .color(styles::TEXT_LABEL_COLOR),
                    );
                    let (inst_txt, inst_col) = if self.is_installed {
                        (&i18n::t("service.installed"), styles::SUCCESS_DARK)
                    } else {
                        (&i18n::t("service.not_installed"), styles::DANGER_DARK)
                    };
                    ui.label(
                        RichText::new(inst_txt)
                            .size(styles::FONT_SIZE_MD)
                            .color(inst_col)
                            .strong(),
                    );
                    ui.end_row();

                    ui.label(
                        RichText::new(i18n::t("service.run_status"))
                            .size(styles::FONT_SIZE_MD)
                            .color(styles::TEXT_LABEL_COLOR),
                    );
                    let (run_txt, run_col) = if self.is_running {
                        (&i18n::t("service.running"), styles::SUCCESS_DARK)
                    } else {
                        (&i18n::t("service.stopped"), styles::DANGER_DARK)
                    };
                    ui.label(
                        RichText::new(run_txt)
                            .size(styles::FONT_SIZE_MD)
                            .color(run_col)
                            .strong(),
                    );
                    ui.end_row();
                });
        });

        ui.add_space(styles::SPACING_MD);

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.section_header(ui, "⚙", &i18n::t("service.service_ops"));

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                if ui.button(i18n::t("service.refresh_status")).clicked() {
                    self.refresh_status();
                }

                ui.separator();

                if !self.is_installed {
                    let btn_text = match self.operation_state {
                        OperationState::Installing => &i18n::t("service.installing"),
                        _ => &i18n::t("service.install_service"),
                    };
                    let btn = egui::Button::new(
                        RichText::new(btn_text)
                            .color(Color32::WHITE)
                            .size(styles::FONT_SIZE_MD),
                    )
                    .fill(styles::INFO_COLOR)
                    .corner_radius(egui::CornerRadius::same(6));

                    let btn_response =
                        ui.add_enabled(self.operation_state == OperationState::Idle, btn);
                    if btn_response.clicked() {
                        self.install_service_async(ui.ctx());
                    }
                } else {
                    if !self.is_running {
                        let btn_text = match self.operation_state {
                            OperationState::Starting => &i18n::t("service.starting"),
                            _ => &i18n::t("service.start_service"),
                        };
                        let btn = egui::Button::new(
                            RichText::new(btn_text)
                                .color(Color32::WHITE)
                                .size(styles::FONT_SIZE_MD),
                        )
                        .fill(styles::SUCCESS_DARK)
                        .corner_radius(egui::CornerRadius::same(6));

                        let btn_response =
                            ui.add_enabled(self.operation_state == OperationState::Idle, btn);
                        if btn_response.clicked() {
                            self.start_service_async(ui.ctx());
                        }
                    } else {
                        let btn_text = match self.operation_state {
                            OperationState::Stopping => &i18n::t("service.stopping"),
                            _ => &i18n::t("service.stop_service"),
                        };
                        let btn = egui::Button::new(
                            RichText::new(btn_text)
                                .color(Color32::WHITE)
                                .size(styles::FONT_SIZE_MD),
                        )
                        .fill(Color32::from_rgb(230, 126, 34))
                        .corner_radius(egui::CornerRadius::same(6));

                        let btn_response =
                            ui.add_enabled(self.operation_state == OperationState::Idle, btn);
                        if btn_response.clicked() {
                            self.stop_service_async(ui.ctx());
                        }

                        ui.separator();

                        let btn_text = match self.operation_state {
                            OperationState::Restarting => &i18n::t("service.restarting"),
                            _ => &i18n::t("service.restart_service"),
                        };
                        let restart_btn = egui::Button::new(
                            RichText::new(btn_text)
                                .color(Color32::WHITE)
                                .size(styles::FONT_SIZE_MD),
                        )
                        .fill(styles::INFO_COLOR)
                        .corner_radius(egui::CornerRadius::same(6));

                        let restart_btn_response = ui
                            .add_enabled(self.operation_state == OperationState::Idle, restart_btn);
                        if restart_btn_response.clicked() {
                            self.restart_service_async(ui.ctx());
                        }
                    }

                    ui.separator();

                    if self.confirming_uninstall {
                        ui.label(
                            RichText::new(i18n::t("service.confirm_uninstall"))
                                .size(styles::FONT_SIZE_MD)
                                .color(styles::DANGER_DARK),
                        );

                        let can_operate = self.operation_state == OperationState::Idle;

                        let yes_btn = egui::Button::new(
                            RichText::new(i18n::t("service.confirm"))
                                .color(Color32::WHITE)
                                .size(styles::FONT_SIZE_MD),
                        )
                        .fill(styles::DANGER_DARK)
                        .corner_radius(egui::CornerRadius::same(6));

                        let yes_response = ui.add_enabled(can_operate, yes_btn);
                        if yes_response.clicked() {
                            self.confirming_uninstall = false;
                            self.uninstall_service_async(ui.ctx());
                        }
                        if ui.button(i18n::t("service.cancel")).clicked() {
                            self.confirming_uninstall = false;
                        }
                    } else {
                        let uninstall_btn_text = match self.operation_state {
                            OperationState::Uninstalling => &i18n::t("service.uninstalling"),
                            _ => &i18n::t("service.uninstall_service"),
                        };
                        let uninstall_btn = egui::Button::new(
                            RichText::new(uninstall_btn_text)
                                .size(styles::FONT_SIZE_MD)
                                .color(styles::DANGER_DARK),
                        )
                        .fill(styles::DANGER_LIGHT)
                        .corner_radius(egui::CornerRadius::same(6));

                        let uninstall_btn_response = ui.add_enabled(
                            self.operation_state == OperationState::Idle,
                            uninstall_btn,
                        );
                        if uninstall_btn_response.clicked() {
                            self.confirming_uninstall = true;
                        }
                    }
                }
            });
        });

        ui.add_space(styles::SPACING_MD);

        styles::warning_box(
            ui,
            &i18n::t("service.notes_title"),
            &[
                &i18n::t("service.note_1"),
                &i18n::t("service.note_2"),
                &i18n::t("service.note_3"),
                &i18n::t("service.note_4"),
            ],
        );
    }
}
