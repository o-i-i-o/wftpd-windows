use crate::core::config::Config;
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
                tracing::warn!("加载用户配置失败，使用默认配置: {}", e);
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
                tracing::info!("用户配置已保存");
                self.status_message = Some(("用户配置已保存".into(), true));
            }
            Err(e) => {
                tracing::error!("保存用户配置失败: {}", e);
                self.status_message = Some((format!("保存失败: {}", e), false));
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
            return Some("用户名不能为空".into());
        }
        if is_add && self.form_password.is_empty() {
            return Some("密码不能为空".into());
        }
        if !self.form_password.is_empty() && self.form_password != self.form_confirm_password {
            return Some("两次密码不一致".into());
        }
        if self.form_home_dir.trim().is_empty() {
            return Some("主目录不能为空".into());
        }
        None
    }

    fn pick_home_directory(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("选择用户主目录")
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

        // 使用引用避免不必要的 clone
        let is_add = matches!(&self.modal, ModalMode::AddUser);
        let is_confirm = matches!(&self.modal, ModalMode::ConfirmDelete(_));
        let title = match &self.modal {
            ModalMode::AddUser => "添加用户",
            ModalMode::EditUser(_) => "编辑用户",
            ModalMode::ConfirmDelete(_) => "确认删除",
            ModalMode::None => "",
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

        egui::Window::new(title)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(center)
            .fixed_size([mw, 0.0])
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                if is_confirm {
                    // 使用引用获取用户名，避免 clone
                    if let ModalMode::ConfirmDelete(ref name) = self.modal {
                        ui.vertical_centered(|ui| {
                            ui.add_space(styles::SPACING_SM);
                            ui.label(
                                RichText::new(format!("确定要删除用户 \"{}\" 吗？", name))
                                    .size(styles::FONT_SIZE_MD),
                            );
                            ui.label(
                                RichText::new("此操作不可撤销。")
                                    .color(styles::DANGER_COLOR)
                                    .size(styles::FONT_SIZE_MD),
                            );
                            ui.add_space(styles::SPACING_MD);
                        });
                        ui.horizontal(|ui| {
                            let w = (mw - 32.0) / 2.0;
                            if ui.add_sized([w, 32.0], egui::Button::new("取消")).clicked() {
                                close_modal = true;
                            }
                            let del = egui::Button::new(
                                RichText::new("确认删除")
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
                                        RichText::new("用户名:")
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                    ),
                                );
                                if is_add {
                                    styles::input_frame().show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.form_username)
                                                .desired_width(input_width)
                                                .hint_text("请输入用户名")
                                                .font(egui::FontId::new(
                                                    styles::FONT_SIZE_MD,
                                                    egui::FontFamily::Proportional,
                                                )),
                                        );
                                    });
                                } else {
                                    // 使用引用，避免不必要的 clone
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
                                        RichText::new(if is_add {
                                            "密码:"
                                        } else {
                                            "新密码:"
                                        })
                                        .size(styles::FONT_SIZE_MD)
                                        .color(styles::TEXT_SECONDARY_COLOR),
                                    ),
                                );
                                styles::input_frame().show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.form_password)
                                            .password(true)
                                            .desired_width(input_width)
                                            .hint_text(if is_add {
                                                "请输入密码"
                                            } else {
                                                "留空则不修改"
                                            })
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
                                        RichText::new("确认密码:")
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                    ),
                                );
                                styles::input_frame().show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.form_confirm_password)
                                            .password(true)
                                            .desired_width(input_width)
                                            .hint_text("再次输入密码")
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
                                        RichText::new("主目录:")
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                    ),
                                );
                                styles::input_frame().show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.form_home_dir)
                                            .desired_width(ui.available_width() - 80.0)
                                            .hint_text("如: C:\\Users\\ftp")
                                            .font(egui::FontId::new(
                                                styles::FONT_SIZE_MD,
                                                egui::FontFamily::Proportional,
                                            )),
                                    );
                                });
                                if ui.button("浏览...").clicked() {
                                    self.pick_home_directory();
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    [label_width, 24.0],
                                    egui::Label::new(
                                        RichText::new("权限:")
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                    ),
                                );
                                ui.checkbox(&mut self.form_is_admin, "赋予管理员权限");
                            });

                            ui.add_space(styles::SPACING_XS);

                            // 高级权限设置
                            ui.horizontal_wrapped(|ui| {
                                ui.checkbox(&mut self.form_permissions.can_read, "读取");
                                ui.checkbox(&mut self.form_permissions.can_write, "写入");
                                ui.checkbox(&mut self.form_permissions.can_delete, "删除");
                                ui.checkbox(&mut self.form_permissions.can_list, "列表");
                                ui.checkbox(&mut self.form_permissions.can_mkdir, "创建目录");
                                ui.checkbox(&mut self.form_permissions.can_rmdir, "删除目录");
                                ui.checkbox(&mut self.form_permissions.can_rename, "重命名");
                                ui.checkbox(&mut self.form_permissions.can_append, "追加");
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
                        if ui.add_sized([w, 30.0], egui::Button::new("取消")).clicked() {
                            close_modal = true;
                        }
                        let ok = egui::Button::new(
                            RichText::new(if is_add { "添加" } else { "保存" })
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
                                tracing::info!("用户 {} 权限更新成功", username);
                            }
                            Err(e) => {
                                tracing::warn!("用户 {} 权限更新失败: {}", username, e);
                                self.status_message =
                                    Some((format!("用户已添加，但权限设置失败: {}", e), false));
                            }
                        }
                        self.save();
                        self.modal = ModalMode::None;
                    }
                    Err(e) => {
                        tracing::error!("添加用户 {} 失败: {}", self.form_username.trim(), e);
                        self.form_error = Some(format!("添加失败: {}", e));
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
                        tracing::info!("用户 {} 主目录更新成功", uname);
                    }
                    Err(e) => {
                        tracing::warn!("用户 {} 主目录更新失败: {}", uname, e);
                        error_messages.push(format!("主目录更新失败: {}", e));
                        has_error = true;
                    }
                }

                if !self.form_password.is_empty() {
                    match self
                        .user_manager
                        .update_password(uname, &self.form_password)
                    {
                        Ok(_) => {
                            tracing::info!("用户 {} 密码更新成功", uname);
                        }
                        Err(e) => {
                            tracing::warn!("用户 {} 密码更新失败: {}", uname, e);
                            error_messages.push(format!("密码更新失败: {}", e));
                            has_error = true;
                        }
                    }
                }

                match self
                    .user_manager
                    .update_permissions(uname, self.form_permissions)
                {
                    Ok(_) => {
                        tracing::info!("用户 {} 权限更新成功", uname);
                    }
                    Err(e) => {
                        tracing::warn!("用户 {} 权限更新失败: {}", uname, e);
                        error_messages.push(format!("权限更新失败: {}", e));
                        has_error = true;
                    }
                }

                match self.user_manager.set_user_admin(uname, self.form_is_admin) {
                    Ok(_) => {
                        tracing::info!("用户 {} 管理员状态更新成功", uname);
                    }
                    Err(e) => {
                        tracing::warn!("用户 {} 管理员状态更新失败: {}", uname, e);
                        error_messages.push(format!("管理员状态更新失败: {}", e));
                        has_error = true;
                    }
                }

                if has_error {
                    self.status_message = Some((
                        format!("部分更新失败: {}", error_messages.join("; ")),
                        false,
                    ));
                }
                self.save();
                self.modal = ModalMode::None;
            }
        }

        if let Some(name) = delete_target {
            match self.user_manager.remove_user(&name) {
                Ok(_) => {
                    tracing::info!("用户 {} 已删除", name);
                    self.save();
                    // 删除成功后关闭模态框
                    self.modal = ModalMode::None;
                }
                Err(e) => {
                    tracing::error!("删除用户 {} 失败：{}", name, e);
                    self.status_message = Some((format!("删除用户失败：{}", e), false));
                }
            }
        }

        // 分开处理提交和关闭逻辑，避免状态冲突
        if do_submit {
            // 提交操作已在上方完成，模态框已关闭
            self.form_error = None;
        } else if close_modal {
            // 取消操作直接关闭模态框
            self.modal = ModalMode::None;
            self.form_error = None;
        }
        // 删除操作的模态框关闭已在删除逻辑中处理
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();

        self.show_modal(&ctx);

        ui.horizontal(|ui| {
            styles::page_header(ui, "👥", "用户管理");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 直接使用引用，避免不必要的 clone
                if let Some((msg, ok)) = &self.status_message {
                    styles::status_message(ui, msg, *ok);
                }
            });
        });

        ui.horizontal(|ui| {
            let add_btn = egui::Button::new(
                RichText::new("➕ 添加用户")
                    .color(Color32::WHITE)
                    .size(styles::FONT_SIZE_MD),
            )
            .fill(styles::PRIMARY_COLOR)
            .corner_radius(egui::CornerRadius::same(6));
            if ui.add(add_btn).clicked() {
                self.open_add_modal();
            }
            ui.add_space(styles::SPACING_SM);
            if ui.button("🔄 刷新").clicked() {
                match UserManager::load(&Config::get_users_path()) {
                    Ok(um) => {
                        self.user_manager = um;
                        tracing::info!("用户列表已刷新");
                        self.status_message = Some(("用户列表已刷新".into(), true));
                    }
                    Err(e) => {
                        tracing::error!("刷新用户列表失败: {}", e);
                        self.status_message = Some((format!("刷新失败: {}", e), false));
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 使用 user_count() 避免不必要的 clone
                let count = self.user_manager.user_count();
                ui.label(
                    RichText::new(format!("共 {} 个用户", count))
                        .size(styles::FONT_SIZE_MD)
                        .color(styles::TEXT_MUTED_COLOR),
                );
            });
        });

        ui.add_space(styles::SPACING_MD);

        // 使用 iter_users() 返回引用，避免 clone 所有用户
        let users: Vec<&User> = self.user_manager.iter_users().collect();
        let mut to_toggle: Option<(String, bool)> = None;
        let mut to_edit: Option<User> = None;
        let mut to_delete_confirm: Option<String> = None;

        if users.is_empty() {
            styles::empty_state(ui, "📭", "暂无用户", "点击 \"➕ 添加用户\" 创建第一个用户");
        } else {
            styles::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                let available_width = ui.available_width();

                let table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(styles::table_column_percent(available_width, 0.15, 100.0))
                    .column(styles::table_column_percent(available_width, 0.30, 180.0))
                    .column(styles::table_column_percent(available_width, 0.12, 80.0))
                    .column(styles::table_column_percent(available_width, 0.10, 70.0))
                    .column(styles::table_column_remainder(150.0))
                    .min_scrolled_height(0.0)
                    .sense(egui::Sense::hover());

                table
                    .header(styles::FONT_SIZE_MD, |mut header| {
                        header.col(|ui| {
                            ui.label(
                                RichText::new("用户名")
                                    .strong()
                                    .color(styles::TEXT_PRIMARY_COLOR),
                            );
                        });
                        header.col(|ui| {
                            ui.label(
                                RichText::new("主目录")
                                    .strong()
                                    .color(styles::TEXT_PRIMARY_COLOR),
                            );
                        });
                        header.col(|ui| {
                            ui.label(
                                RichText::new("权限")
                                    .strong()
                                    .color(styles::TEXT_PRIMARY_COLOR),
                            );
                        });
                        header.col(|ui| {
                            ui.label(
                                RichText::new("状态")
                                    .strong()
                                    .color(styles::TEXT_PRIMARY_COLOR),
                            );
                        });
                        header.col(|ui| {
                            ui.label(
                                RichText::new("操作")
                                    .strong()
                                    .color(styles::TEXT_PRIMARY_COLOR),
                            );
                        });
                    })
                    .body(|mut body| {
                        for &user in &users {
                            // user 现在是 User（通过解构引用）
                            body.row(styles::FONT_SIZE_MD, |mut row| {
                                row.col(|ui| {
                                    // 直接使用引用访问字段
                                    ui.label(
                                        RichText::new(&user.username)
                                            .size(styles::FONT_SIZE_MD)
                                            .strong()
                                            .color(styles::TEXT_PRIMARY_COLOR),
                                    );
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(&user.home_dir)
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_SECONDARY_COLOR),
                                    );
                                });
                                row.col(|ui| {
                                    let admin_rt = if user.is_admin {
                                        RichText::new("👑 管理员")
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::PRIMARY_COLOR)
                                    } else {
                                        RichText::new("👤 普通")
                                            .size(styles::FONT_SIZE_MD)
                                            .color(styles::TEXT_LABEL_COLOR)
                                    };
                                    ui.label(admin_rt);
                                });
                                row.col(|ui| {
                                    let st_col = if user.enabled {
                                        styles::SUCCESS_DARK
                                    } else {
                                        styles::DANGER_DARK
                                    };
                                    let st_icon = if user.enabled { "●" } else { "○" };
                                    ui.label(
                                        RichText::new(format!("{} 启用", st_icon))
                                            .size(styles::FONT_SIZE_MD)
                                            .color(st_col),
                                    );
                                });
                                row.col(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;

                                        let edit_btn = egui::Button::new(
                                            RichText::new("编辑").size(styles::FONT_SIZE_MD),
                                        )
                                        .fill(styles::BG_SECONDARY)
                                        .stroke(egui::Stroke::new(1.0, styles::BORDER_COLOR))
                                        .corner_radius(egui::CornerRadius::same(4));
                                        if ui.add(edit_btn).clicked() {
                                            // 在需要时 clone
                                            to_edit = Some(user.clone());
                                        }

                                        let toggle_btn = egui::Button::new(
                                            RichText::new(if user.enabled {
                                                "禁用"
                                            } else {
                                                "启用"
                                            })
                                            .size(styles::FONT_SIZE_MD),
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
                                            // 在需要时 clone
                                            to_toggle =
                                                Some((user.username.clone(), !user.enabled));
                                        }

                                        let del = egui::Button::new(
                                            RichText::new("删除")
                                                .size(styles::FONT_SIZE_MD)
                                                .color(Color32::WHITE),
                                        )
                                        .fill(styles::DANGER_DARK)
                                        .corner_radius(egui::CornerRadius::same(4));
                                        if ui.add(del).clicked() {
                                            // 在需要时 clone
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
        }

        if let Some(u) = to_edit {
            self.open_edit_modal(&u);
        }
        if let Some((name, enabled)) = to_toggle {
            match self.user_manager.set_user_enabled(&name, enabled) {
                Ok(_) => {
                    tracing::info!(
                        "用户 {} 状态已更改为 {}",
                        name,
                        if enabled { "启用" } else { "禁用" }
                    );
                }
                Err(e) => {
                    tracing::error!("更改用户 {} 状态失败: {}", name, e);
                    self.status_message = Some((format!("更改用户状态失败: {}", e), false));
                }
            }
            self.save();
        }
        if let Some(name) = to_delete_confirm {
            self.modal = ModalMode::ConfirmDelete(name);
        }
    }
}
