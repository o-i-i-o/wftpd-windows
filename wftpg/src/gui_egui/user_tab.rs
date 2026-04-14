use crate::core::config::Config;
use crate::core::i18n;
use crate::core::users::{Permissions, User, UserManager};
use crate::gui_egui::styles;
use egui::{Color32, Frame, RichText, Ui};
use egui_extras::TableBuilder;

#[derive(Debug, Clone, PartialEq)]
enum ModalMode {
    None,
    AddUser,
    EditUser(String),
    ConfirmDelete(String),
}

pub struct UserTab {
    user_manager: UserManager,
    modal: ModalMode,
    form_username: String,
    form_password: String,
    form_confirm_password: String,
    form_home_dir: String,
    form_is_admin: bool,
    form_permissions: Permissions,
    form_error: Option<String>,
    status_message: Option<(String, bool)>,
}

impl Default for UserTab {
    fn default() -> Self {
        let user_manager = match UserManager::load(&Config::get_users_path()) {
            Ok(um) => um,
            Err(e) => {
                tracing::warn!("{}", i18n::t_fmt("users.load_failed", &[&e.to_string()]));
                UserManager::default()
            }
        };
        Self {
            user_manager,
            modal: ModalMode::None,
            form_username: String::new(),
            form_password: String::new(),
            form_confirm_password: String::new(),
            form_home_dir: String::new(),
            form_is_admin: false,
            form_permissions: Permissions::full(),
            form_error: None,
            status_message: None,
        }
    }
}

impl UserTab {
    pub fn new() -> Self {
        Self::default()
    }

    fn save(&mut self) {
        match self.user_manager.save(&Config::get_users_path()) {
            Ok(_) => {
                tracing::info!("User config saved");
                match crate::core::ipc::IpcClient::notify_reload() {
                    Ok(response) if response.success => {
                        self.status_message = Some((i18n::t("users.user_saved"), true));
                    }
                    Ok(response) => {
                        tracing::warn!("Backend reload failed: {}", response.message);
                        self.status_message = Some((i18n::t("users.user_saved"), true));
                    }
                    Err(e) => {
                        tracing::warn!("Failed to notify backend reload: {}", e);
                        self.status_message = Some((i18n::t("users.user_saved"), true));
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to save user config: {}", e);
                self.status_message =
                    Some((i18n::t_fmt("users.save_failed", &[&e.to_string()]), false));
            }
        }
    }

    fn open_add_modal(&mut self) {
        self.form_username.clear();
        self.form_password.clear();
        self.form_confirm_password.clear();
        self.form_home_dir.clear();
        self.form_is_admin = false;
        self.form_permissions = Permissions::full();
        self.form_error = None;
        self.modal = ModalMode::AddUser;
    }

    fn open_edit_modal(&mut self, user: &User) {
        self.form_username = user.username.clone();
        self.form_password.clear();
        self.form_confirm_password.clear();
        self.form_home_dir = user.home_dir.clone();
        self.form_is_admin = user.is_admin;
        self.form_permissions = user.permissions;
        self.form_error = None;
        self.modal = ModalMode::EditUser(user.username.clone());
    }

    fn validate_form(&self, is_add: bool) -> Option<String> {
        if is_add && self.form_username.trim().is_empty() {
            return Some(i18n::t("users.username_empty"));
        }
        if is_add && self.form_password.is_empty() {
            return Some(i18n::t("users.password_empty"));
        }
        if !self.form_password.is_empty() && self.form_password != self.form_confirm_password {
            return Some(i18n::t("users.password_mismatch"));
        }
        if self.form_home_dir.trim().is_empty() {
            return Some(i18n::t("users.home_dir_empty"));
        }
        None
    }

    fn pick_home_directory(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(i18n::t("users.select_home_dir"))
            .pick_folder()
        {
            self.form_home_dir = path.to_string_lossy().to_string();
        }
    }

    fn show_modal(&mut self, ctx: &egui::Context) {
        if self.modal == ModalMode::None {
            return;
        }
        let screen = ctx.content_rect();
        if screen.width() <= 0.0 || screen.height() <= 0.0 {
            return;
        }
        egui::Area::new(egui::Id::new("modal_backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                let screen = ctx.content_rect();
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(140));
            });

        let is_add = matches!(&self.modal, ModalMode::AddUser);
        let is_confirm = matches!(&self.modal, ModalMode::ConfirmDelete(_));
        let title = match &self.modal {
            ModalMode::AddUser => i18n::t("users.add_modal_title"),
            ModalMode::EditUser(_) => i18n::t("users.edit_modal_title"),
            ModalMode::ConfirmDelete(_) => i18n::t("users.confirm_delete_title"),
            ModalMode::None => String::new(),
        };
        let mw: f32 = if is_confirm {
            320.0
        } else {
            (screen.width() * 0.4).clamp(380.0, 550.0)
        };
        let center = egui::pos2(screen.center().x, screen.center().y);
        let mut close_modal = false;
        let mut do_submit = false;
        let mut delete_target: Option<String> = None;

        egui::Window::new(&title)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(center)
            .fixed_size([mw, 0.0])
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                if is_confirm {
                    if let ModalMode::ConfirmDelete(ref name) = self.modal {
                        ui.vertical_centered(|ui| {
                            ui.add_space(styles::SPACING_SM);
                            ui.label(
                                RichText::new(i18n::t_fmt("users.confirm_delete_msg", &[name]))
                                    .size(styles::FONT_SIZE_MD),
                            );
                            ui.label(
                                RichText::new(i18n::t("users.cannot_undo"))
                                    .color(styles::DANGER_COLOR)
                                    .size(styles::FONT_SIZE_MD),
                            );
                            ui.add_space(styles::SPACING_MD);
                        });
                        ui.horizontal(|ui| {
                            let w = (mw - 32.0) / 2.0;
                            if ui
                                .add_sized([w, 32.0], egui::Button::new(i18n::t("users.cancel")))
                                .clicked()
                            {
                                close_modal = true;
                            }
                            let del = egui::Button::new(
                                RichText::new(i18n::t("users.confirm_delete"))
                                    .color(Color32::WHITE)
                                    .size(styles::FONT_SIZE_MD),
                            )
                            .fill(styles::DANGER_DARK)
                            .corner_radius(egui::CornerRadius::same(6));
                            if ui.add_sized([w, 32.0], del).clicked() {
                                delete_target = Some(name.clone());
                                close_modal = true;
                            }
                        });
                    }
                } else {
                    let label_width = 80.0;
                    let input_width = mw - label_width - 80.0;

                    let password_label = if is_add {
                        i18n::t("users.password_label")
                    } else {
                        i18n::t("users.new_password_label")
                    };
                    let password_hint = if is_add {
                        i18n::t("users.enter_password")
                    } else {
                        i18n::t("users.leave_empty_no_change")
                    };
                    let ok_button_text = if is_add {
                        i18n::t("users.add")
                    } else {
                        i18n::t("users.save")
                    };

                    Frame::new()
                        .stroke(egui::Stroke::new(1.0, styles::BORDER_COLOR))
                        .inner_margin(egui::Margin {
                            left: 12,
                            right: 12,
                            top: 8,
                            bottom: 8,
                        })
                        .corner_radius(egui::CornerRadius::same(6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [label_width, 24.0],
                                    egui::Label::new(
                                        RichText::new(i18n::t("users.username_label"))
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                    ),
                                );
                                if is_add {
                                    styles::input_frame().show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.form_username)
                                                .desired_width(input_width)
                                                .hint_text(i18n::t("users.enter_username"))
                                                .font(egui::FontId::new(
                                                    styles::FONT_SIZE_MD,
                                                    egui::FontFamily::Proportional,
                                                )),
                                        );
                                    });
                                } else {
                                    ui.label(
                                        RichText::new(&self.form_username)
                                            .strong()
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_PRIMARY_COLOR),
                                    );
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [label_width, 24.0],
                                    egui::Label::new(
                                        RichText::new(&password_label)
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                    ),
                                );
                                styles::input_frame().show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.form_password)
                                            .password(true)
                                            .desired_width(input_width)
                                            .hint_text(&password_hint)
                                            .font(egui::FontId::new(
                                                styles::FONT_SIZE_MD,
                                                egui::FontFamily::Proportional,
                                            )),
                                    );
                                });
                            });

                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [label_width, 24.0],
                                    egui::Label::new(
                                        RichText::new(i18n::t("users.confirm_password_label"))
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                    ),
                                );
                                styles::input_frame().show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.form_confirm_password)
                                            .password(true)
                                            .desired_width(input_width)
                                            .hint_text(i18n::t("users.enter_password_again"))
                                            .font(egui::FontId::new(
                                                styles::FONT_SIZE_MD,
                                                egui::FontFamily::Proportional,
                                            )),
                                    );
                                });
                            });

                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [label_width, 24.0],
                                    egui::Label::new(
                                        RichText::new(i18n::t("users.home_dir_label"))
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                    ),
                                );
                                styles::input_frame().show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.form_home_dir)
                                            .desired_width(ui.available_width() - 80.0)
                                            .hint_text(i18n::t("users.home_dir_example"))
                                            .font(egui::FontId::new(
                                                styles::FONT_SIZE_MD,
                                                egui::FontFamily::Proportional,
                                            )),
                                    );
                                });
                                if ui.button(i18n::t("server.browse")).clicked() {
                                    self.pick_home_directory();
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [label_width, 24.0],
                                    egui::Label::new(
                                        RichText::new(i18n::t("users.permissions_label"))
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                    ),
                                );
                                ui.checkbox(&mut self.form_is_admin, i18n::t("users.grant_admin"));
                            });

                            ui.add_space(styles::SPACING_XS);

                            ui.horizontal_wrapped(|ui| {
                                ui.checkbox(
                                    &mut self.form_permissions.can_read,
                                    i18n::t("users.perm_read"),
                                );
                                ui.checkbox(
                                    &mut self.form_permissions.can_write,
                                    i18n::t("users.perm_write"),
                                );
                                ui.checkbox(
                                    &mut self.form_permissions.can_delete,
                                    i18n::t("users.perm_delete"),
                                );
                                ui.checkbox(
                                    &mut self.form_permissions.can_list,
                                    i18n::t("users.perm_list"),
                                );
                                ui.checkbox(
                                    &mut self.form_permissions.can_mkdir,
                                    i18n::t("users.perm_mkdir"),
                                );
                                ui.checkbox(
                                    &mut self.form_permissions.can_rmdir,
                                    i18n::t("users.perm_rmdir"),
                                );
                                ui.checkbox(
                                    &mut self.form_permissions.can_rename,
                                    i18n::t("users.perm_rename"),
                                );
                                ui.checkbox(
                                    &mut self.form_permissions.can_append,
                                    i18n::t("users.perm_append"),
                                );
                            });
                        });

                    if let Some(ref err) = self.form_error {
                        ui.add_space(styles::SPACING_XS);
                        ui.label(
                            RichText::new(err)
                                .color(styles::DANGER_COLOR)
                                .size(styles::FONT_SIZE_MD),
                        );
                    }
                    ui.add_space(styles::SPACING_SM);
                    ui.separator();
                    ui.add_space(styles::SPACING_XS);
                    ui.horizontal(|ui| {
                        let w = (mw - 32.0) / 2.0;
                        if ui
                            .add_sized([w, 30.0], egui::Button::new(i18n::t("users.cancel")))
                            .clicked()
                        {
                            close_modal = true;
                        }
                        let ok = egui::Button::new(
                            RichText::new(&ok_button_text)
                                .color(Color32::WHITE)
                                .size(styles::FONT_SIZE_MD),
                        )
                        .fill(styles::PRIMARY_COLOR)
                        .corner_radius(egui::CornerRadius::same(6));
                        if ui.add_sized([w, 30.0], ok).clicked() {
                            do_submit = true;
                        }
                    });
                }
            });

        if do_submit {
            if let Some(e) = self.validate_form(is_add) {
                self.form_error = Some(e);
            } else if is_add {
                match self.user_manager.add_user(
                    self.form_username.trim(),
                    &self.form_password,
                    self.form_home_dir.trim(),
                    self.form_is_admin,
                ) {
                    Ok(_) => {
                        let username = self.form_username.trim();
                        match self
                            .user_manager
                            .update_permissions(username, self.form_permissions)
                        {
                            Ok(_) => {
                                tracing::info!("User {} permissions updated", username);
                            }
                            Err(e) => {
                                tracing::warn!("User {} permission update failed: {}", username, e);
                                self.status_message = Some((
                                    i18n::t_fmt("users.user_added_perm_failed", &[&e.to_string()]),
                                    false,
                                ));
                            }
                        }
                        self.save();
                        self.modal = ModalMode::None;
                    }
                    Err(e) => {
                        tracing::error!("Add user {} failed: {}", self.form_username.trim(), e);
                        self.form_error = Some(i18n::t_fmt("users.add_failed", &[&e.to_string()]));
                    }
                }
            } else if let ModalMode::EditUser(ref uname) = self.modal {
                let mut has_error = false;
                let mut error_messages = Vec::new();

                match self
                    .user_manager
                    .update_home_dir(uname, self.form_home_dir.trim())
                {
                    Ok(_) => {
                        tracing::info!("User {} home dir updated", uname);
                    }
                    Err(e) => {
                        tracing::error!("User {} home dir update failed: {}", uname, e);
                        error_messages.push(i18n::t_fmt(
                            "users.home_dir_update_failed",
                            &[&e.to_string()],
                        ));
                        has_error = true;
                    }
                }

                if !self.form_password.is_empty() {
                    match self
                        .user_manager
                        .update_password(uname, &self.form_password)
                    {
                        Ok(_) => {
                            tracing::info!("User {} password updated", uname);
                        }
                        Err(e) => {
                            tracing::error!("User {} password update failed: {}", uname, e);
                            error_messages.push(i18n::t_fmt(
                                "users.password_update_failed",
                                &[&e.to_string()],
                            ));
                            has_error = true;
                        }
                    }
                }

                match self
                    .user_manager
                    .update_permissions(uname, self.form_permissions)
                {
                    Ok(_) => {
                        tracing::info!("User {} permissions updated", uname);
                    }
                    Err(e) => {
                        tracing::error!("User {} permissions update failed: {}", uname, e);
                        error_messages
                            .push(i18n::t_fmt("users.perm_update_failed", &[&e.to_string()]));
                        has_error = true;
                    }
                }

                match self.user_manager.set_user_admin(uname, self.form_is_admin) {
                    Ok(_) => {
                        tracing::info!("User {} admin status updated", uname);
                    }
                    Err(e) => {
                        tracing::error!("User {} admin status update failed: {}", uname, e);
                        error_messages.push(i18n::t_fmt(
                            "users.admin_status_update_failed",
                            &[&e.to_string()],
                        ));
                        has_error = true;
                    }
                }

                if has_error {
                    self.status_message = Some((
                        i18n::t_fmt("users.partial_update_failed", &[&error_messages.join(", ")]),
                        false,
                    ));
                } else {
                    self.save();
                }
                self.modal = ModalMode::None;
            }
        }

        if let Some(name) = delete_target {
            match self.user_manager.remove_user(&name) {
                Ok(_) => {
                    tracing::info!("User {} deleted", name);
                    self.save();
                }
                Err(e) => {
                    tracing::error!("Failed to delete user {}: {}", name, e);
                }
            }
            self.modal = ModalMode::None;
        }

        if close_modal {
            self.modal = ModalMode::None;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();

        self.show_modal(&ctx);

        styles::page_header(ui, "👥", &i18n::t("users.title"));

        ui.horizontal(|ui| {
            if ui
                .add(styles::primary_button(&i18n::t("users.add_user")))
                .clicked()
            {
                self.open_add_modal();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(i18n::t("log.refresh")).clicked() {
                    match UserManager::load(&Config::get_users_path()) {
                        Ok(um) => {
                            self.user_manager = um;
                            self.status_message =
                                Some((i18n::t("users.user_list_refreshed"), true));
                        }
                        Err(e) => {
                            self.status_message = Some((
                                i18n::t_fmt("users.refresh_failed", &[&e.to_string()]),
                                false,
                            ));
                        }
                    }
                }

                if let Some((msg, success)) = &self.status_message {
                    styles::status_message(ui, msg, *success);
                }
            });
        });

        ui.add_space(styles::SPACING_MD);

        let users = self.user_manager.get_users();
        if users.is_empty() {
            styles::empty_state(
                ui,
                "👥",
                &i18n::t("users.no_users"),
                &i18n::t("users.no_users_hint"),
            );
            return;
        }

        ui.label(
            RichText::new(i18n::t_fmt(
                "users.total_users",
                &[&users.len().to_string()],
            ))
            .size(styles::FONT_SIZE_MD)
            .color(styles::TEXT_MUTED_COLOR),
        );
        ui.add_space(styles::SPACING_SM);

        let mut to_edit: Option<User> = None;
        let mut to_toggle: Option<(String, bool)> = None;
        let mut to_delete_confirm: Option<String> = None;

        styles::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            let available_width = ui.available_width();
            let table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(styles::table_column_percent(available_width, 0.15, 100.0))
                .column(styles::table_column_percent(available_width, 0.35, 200.0))
                .column(styles::table_column_percent(available_width, 0.15, 100.0))
                .column(styles::table_column_percent(available_width, 0.10, 80.0))
                .column(styles::table_column_remainder(180.0))
                .min_scrolled_height(0.0)
                .sense(egui::Sense::hover());

            table
                .header(styles::FONT_SIZE_MD, |mut header| {
                    header.col(|ui| {
                        ui.with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    RichText::new(i18n::t("users.username"))
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
                                    RichText::new(i18n::t("users.home_dir"))
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
                                    RichText::new(i18n::t("users.permissions"))
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
                                    RichText::new(i18n::t("users.status"))
                                        .strong()
                                        .color(styles::TEXT_PRIMARY_COLOR),
                                );
                            },
                        );
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new(i18n::t("users.actions"))
                                .strong()
                                .color(styles::TEXT_PRIMARY_COLOR),
                        );
                    });
                })
                .body(|mut body| {
                    for user in users.values() {
                        body.row(styles::FONT_SIZE_MD, |mut row| {
                            row.col(|ui| {
                                ui.with_layout(
                                    egui::Layout::centered_and_justified(
                                        egui::Direction::LeftToRight,
                                    ),
                                    |ui| {
                                        ui.label(
                                            RichText::new(&user.username)
                                                .size(styles::FONT_SIZE_MD)
                                                .strong()
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
                                        ui.label(
                                            RichText::new(&user.home_dir)
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
                                        let perm_text = if user.is_admin {
                                            i18n::t("users.admin")
                                        } else {
                                            i18n::t("users.normal")
                                        };
                                        let perm_color = if user.is_admin {
                                            styles::PRIMARY_COLOR
                                        } else {
                                            styles::TEXT_MUTED_COLOR
                                        };
                                        ui.label(
                                            RichText::new(perm_text)
                                                .size(styles::FONT_SIZE_MD)
                                                .strong()
                                                .color(perm_color),
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
                                        let (status_text, status_color) = if user.enabled {
                                            (i18n::t("users.enabled"), styles::SUCCESS_COLOR)
                                        } else {
                                            (i18n::t("users.disabled"), styles::DANGER_COLOR)
                                        };
                                        ui.label(
                                            RichText::new(status_text)
                                                .size(styles::FONT_SIZE_MD)
                                                .color(status_color),
                                        );
                                    },
                                );
                            });
                            row.col(|ui| {
                                ui.horizontal(|ui| {
                                    let edit_btn = egui::Button::new(
                                        RichText::new(i18n::t("users.edit"))
                                            .size(styles::FONT_SIZE_MD),
                                    )
                                    .fill(styles::BG_SECONDARY)
                                    .stroke(egui::Stroke::new(1.0, styles::BORDER_COLOR))
                                    .corner_radius(egui::CornerRadius::same(4));
                                    if ui.add(edit_btn).clicked() {
                                        to_edit = Some(user.clone());
                                    }

                                    let toggle_text = if user.enabled {
                                        i18n::t("users.disable")
                                    } else {
                                        i18n::t("users.enable")
                                    };
                                    let toggle_btn = egui::Button::new(
                                        RichText::new(&toggle_text).size(styles::FONT_SIZE_MD),
                                    )
                                    .fill(if user.enabled {
                                        styles::DANGER_LIGHT
                                    } else {
                                        styles::SUCCESS_LIGHT
                                    })
                                    .stroke(egui::Stroke::new(
                                        1.0,
                                        if user.enabled {
                                            styles::DANGER_COLOR
                                        } else {
                                            styles::SUCCESS_COLOR
                                        },
                                    ))
                                    .corner_radius(egui::CornerRadius::same(4));
                                    if ui.add(toggle_btn).clicked() {
                                        to_toggle = Some((user.username.clone(), !user.enabled));
                                    }

                                    let del = egui::Button::new(
                                        RichText::new(i18n::t("users.delete"))
                                            .size(styles::FONT_SIZE_MD)
                                            .color(Color32::WHITE),
                                    )
                                    .fill(styles::DANGER_DARK)
                                    .corner_radius(egui::CornerRadius::same(4));
                                    if ui.add(del).clicked() {
                                        to_delete_confirm = Some(user.username.clone());
                                    }
                                });
                            });
                        });
                        body.row(2.0, |mut row| {
                            let col_count = 5;
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

        if let Some(u) = to_edit {
            self.open_edit_modal(&u);
        }
        if let Some((name, enabled)) = to_toggle {
            match self.user_manager.set_user_enabled(&name, enabled) {
                Ok(_) => {
                    tracing::info!(
                        "User {} status changed to {}",
                        name,
                        if enabled { "enabled" } else { "disabled" }
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to change user {} status: {}", name, e);
                    self.status_message = Some((
                        i18n::t_fmt("users.status_change_failed", &[&e.to_string()]),
                        false,
                    ));
                }
            }
            self.save();
        }
        if let Some(name) = to_delete_confirm {
            self.modal = ModalMode::ConfirmDelete(name);
        }
    }
}
