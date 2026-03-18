use egui::{RichText, Ui, Color32, Frame};
use crate::core::users::{User, UserManager, Permissions};
use crate::core::config::Config;
use crate::gui_egui::styles;
use egui_file_dialog::FileDialog;

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
    show_permissions: bool,
    file_dialog: FileDialog,
}

impl Default for UserTab {
    fn default() -> Self {
        let user_manager = UserManager::load(&Config::get_users_path()).unwrap_or_default();
        Self {
            user_manager, modal: ModalMode::None,
            form_username: String::new(), form_password: String::new(),
            form_confirm_password: String::new(), form_home_dir: String::new(),
            form_is_admin: false, form_permissions: Permissions::full(),
            form_error: None, status_message: None,
            show_permissions: false,
            file_dialog: FileDialog::new().title("选择用户主目录"),
        }
    }
}

impl UserTab {
    pub fn new() -> Self { Self::default() }

    fn save(&mut self) {
        match self.user_manager.save(&Config::get_users_path()) {
            Ok(_) => self.status_message = Some(("✓ 用户配置已保存".into(), true)),
            Err(e) => self.status_message = Some((format!("✗ 保存失败: {}", e), false)),
        }
    }

    fn open_add_modal(&mut self) {
        self.form_username.clear(); self.form_password.clear();
        self.form_confirm_password.clear(); self.form_home_dir.clear();
        self.form_is_admin = false; self.form_permissions = Permissions::full();
        self.form_error = None; self.show_permissions = false;
        self.modal = ModalMode::AddUser;
    }

    fn open_edit_modal(&mut self, user: &User) {
        self.form_username = user.username.clone();
        self.form_password.clear(); self.form_confirm_password.clear();
        self.form_home_dir = user.home_dir.clone();
        self.form_is_admin = user.is_admin; self.form_permissions = user.permissions;
        self.form_error = None; self.show_permissions = false;
        self.modal = ModalMode::EditUser(user.username.clone());
    }

    fn validate_form(&self, is_add: bool) -> Option<String> {
        if is_add && self.form_username.trim().is_empty() { return Some("用户名不能为空".into()); }
        if is_add && self.form_password.is_empty() { return Some("密码不能为空".into()); }
        if !self.form_password.is_empty() && self.form_password != self.form_confirm_password {
            return Some("两次密码不一致".into());
        }
        if self.form_home_dir.trim().is_empty() { return Some("主目录不能为空".into()); }
        None
    }

    fn show_modal(&mut self, ctx: &egui::Context) {
        if self.modal == ModalMode::None { return; }
        let screen = ctx.available_rect();
        if screen.width() <= 0.0 || screen.height() <= 0.0 { return; }
        egui::Area::new(egui::Id::new("modal_backdrop"))
            .fixed_pos(egui::pos2(0.0, 0.0))
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                let screen = ctx.available_rect();
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(140));
            });
        let is_add = self.modal == ModalMode::AddUser;
        let is_confirm = matches!(&self.modal, ModalMode::ConfirmDelete(_));
        let title = match &self.modal {
            ModalMode::AddUser => "添加用户",
            ModalMode::EditUser(_) => "编辑用户",
            ModalMode::ConfirmDelete(_) => "确认删除",
            ModalMode::None => "",
        };
        let mw: f32 = if is_confirm { 320.0 } else { 420.0 };
        let screen = ctx.available_rect();
        if screen.width() <= 0.0 || screen.height() <= 0.0 { return; }
        let center = egui::pos2(screen.center().x, screen.center().y);
        let mut close_modal = false;
        let mut do_submit = false;
        let mut delete_target: Option<String> = None;

        egui::Window::new(title)
            .pivot(egui::Align2::CENTER_CENTER).fixed_pos(center)
            .fixed_size([mw, 0.0]).collapsible(false).resizable(false)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                if is_confirm {
                    if let ModalMode::ConfirmDelete(ref name) = self.modal.clone() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(8.0);
                            ui.label(RichText::new(format!("确定要删除用户 \"{}\" 吗？", name)).size(14.0));
                            ui.label(RichText::new("此操作不可撤销。").color(Color32::from_rgb(192,57,43)).size(12.0));
                            ui.add_space(12.0);
                        });
                        ui.horizontal(|ui| {
                            let w = (mw - 32.0) / 2.0;
                            if ui.add_sized([w,32.0], egui::Button::new("取消")).clicked() { close_modal = true; }
                            let del = egui::Button::new(RichText::new("确认删除").color(Color32::WHITE))
                                .fill(Color32::from_rgb(192,57,43));
                            if ui.add_sized([w,32.0], del).clicked() {
                                delete_target = Some(name.clone()); close_modal = true;
                            }
                        });
                    }
                } else {
                    Frame::new()
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(220, 225, 230)))
                        .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 })
                        .show(ui, |ui| {
                            egui::Grid::new("user_form_grid").num_columns(2).spacing([8.0,8.0]).min_col_width(80.0)
                                .show(ui, |ui| {
                                    ui.label("用户名:");
                                    if is_add {
                                        ui.add(egui::TextEdit::singleline(&mut self.form_username)
                                            .desired_width(260.0).hint_text("请输入用户名"));
                                    } else {
                                        ui.label(RichText::new(self.form_username.clone()).strong().size(14.0));
                                    }
                                    ui.end_row();
                                    ui.label(if is_add {"密码:"} else {"新密码:"});
                                    ui.add(egui::TextEdit::singleline(&mut self.form_password).password(true)
                                        .desired_width(260.0).hint_text(if is_add {"请输入密码"} else {"留空则不修改"}));
                                    ui.end_row();
                                    ui.label("确认密码:");
                                    ui.add(egui::TextEdit::singleline(&mut self.form_confirm_password).password(true)
                                        .desired_width(260.0).hint_text("再次输入密码"));
                                    ui.end_row();
                                    ui.label("主目录:");
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut self.form_home_dir)
                                            .desired_width(220.0).hint_text("如: C:\\Users\\ftp"));
                                        if ui.button("浏览...").clicked() {
                                            self.file_dialog.pick_directory();
                                        }
                                    });
                                    ui.end_row();
                                    ui.label("管理员:");
                                    ui.checkbox(&mut self.form_is_admin, "赋予管理员权限");
                                    ui.end_row();
                                    ui.label("权限:");
                                    ui.horizontal(|ui| {
                                        if ui.button(if self.show_permissions {"收起权限"} else {"高级权限"}).clicked() {
                                            self.show_permissions = !self.show_permissions;
                                        }
                                    });
                                    ui.end_row();
                                });
                        });
                    
                    if self.show_permissions {
                        Frame::new()
                            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(220, 225, 230)))
                            .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 })
                            .show(ui, |ui| {
                                egui::Grid::new("permissions_grid").num_columns(2).spacing([16.0,8.0]).min_col_width(100.0)
                                    .show(ui, |ui| {
                                        ui.checkbox(&mut self.form_permissions.can_read, "读取");
                                        ui.checkbox(&mut self.form_permissions.can_write, "写入");
                                        ui.end_row();
                                        ui.checkbox(&mut self.form_permissions.can_delete, "删除");
                                        ui.checkbox(&mut self.form_permissions.can_list, "列表");
                                        ui.end_row();
                                        ui.checkbox(&mut self.form_permissions.can_mkdir, "创建目录");
                                        ui.checkbox(&mut self.form_permissions.can_rmdir, "删除目录");
                                        ui.end_row();
                                        ui.checkbox(&mut self.form_permissions.can_rename, "重命名");
                                        ui.checkbox(&mut self.form_permissions.can_append, "追加");
                                        ui.end_row();
                                    });
                            });
                    }
                    
                    if let Some(ref err) = self.form_error.clone() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(err).color(Color32::from_rgb(192,57,43)).size(12.0));
                    }
                    ui.add_space(8.0); ui.separator(); ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let w = (mw - 32.0) / 2.0;
                        if ui.add_sized([w,30.0], egui::Button::new("取消")).clicked() { close_modal = true; }
                        let ok = egui::Button::new(RichText::new(if is_add {"添加"} else {"保存"}).color(Color32::WHITE))
                            .fill(Color32::from_rgb(41,128,185));
                        if ui.add_sized([w,30.0], ok).clicked() { do_submit = true; }
                    });
                }
            });

        if do_submit {
            if let Some(e) = self.validate_form(is_add) {
                self.form_error = Some(e);
            } else if is_add {
                match self.user_manager.add_user(
                    self.form_username.trim(), &self.form_password,
                    self.form_home_dir.trim(), self.form_is_admin,
                ) {
                    Ok(_) => {
                        // 添加成功后更新权限
                        let _ = self.user_manager.update_permissions(self.form_username.trim(), self.form_permissions);
                        self.save(); self.modal = ModalMode::None;
                    }
                    Err(e) => self.form_error = Some(format!("添加失败: {}", e)),
                }
            } else if let ModalMode::EditUser(ref uname) = self.modal.clone() {
                let _ = self.user_manager.update_home_dir(uname, self.form_home_dir.trim());
                if !self.form_password.is_empty() {
                    let _ = self.user_manager.update_password(uname, &self.form_password);
                }
                let _ = self.user_manager.update_permissions(uname, self.form_permissions);
                self.save(); self.modal = ModalMode::None;
            }
        }
        if let Some(name) = delete_target { let _ = self.user_manager.remove_user(&name); self.save(); }
        if close_modal && !do_submit { self.modal = ModalMode::None; self.form_error = None; }

        self.file_dialog.update(ctx);
        if let Some(path) = self.file_dialog.take_picked() {
            self.form_home_dir = path.to_string_lossy().to_string();
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.show_modal(&ctx);

        ui.heading(RichText::new("👥 用户管理").color(styles::TEXT_PRIMARY_COLOR));
        ui.separator();

        if let Some((msg, ok)) = &self.status_message.clone() {
            let (bg_color, text_color, icon) = if *ok {
                (Color32::from_rgb(220, 252, 231), Color32::from_rgb(16, 124, 16), "✓")
            } else {
                (Color32::from_rgb(253, 230, 230), Color32::from_rgb(185, 28, 28), "✗")
            };
            egui::Frame::new()
                .fill(bg_color)
                .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 })
                .corner_radius(egui::CornerRadius::same(6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon).size(16.0).color(text_color));
                        ui.label(RichText::new(msg).color(text_color));
                    });
                });
            ui.add_space(8.0);
        }

        ui.horizontal(|ui| {
            let add_btn = egui::Button::new(RichText::new("➕ 添加用户").color(Color32::WHITE).size(13.0))
                .fill(Color32::from_rgb(108, 92, 231))
                .corner_radius(egui::CornerRadius::same(6));
            if ui.add(add_btn).clicked() { self.open_add_modal(); }
            ui.add_space(8.0);
            if ui.button("🔄 刷新").clicked() {
                self.user_manager = UserManager::load(&Config::get_users_path()).unwrap_or_default();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let count = self.user_manager.get_all_users().len();
                ui.label(RichText::new(format!("共 {} 个用户", count))
                    .size(12.0).color(Color32::from_rgb(100, 100, 100)));
            });
        });

        ui.add_space(10.0);

        let users: Vec<User> = self.user_manager.get_all_users();
        let mut to_toggle: Option<(String, bool)> = None;
        let mut to_edit: Option<User> = None;
        let mut to_delete_confirm: Option<String> = None;

        if users.is_empty() {
            egui::Frame::new()
                .fill(Color32::from_rgb(248, 249, 250))
                .inner_margin(egui::Margin { left: 16, right: 16, top: 40, bottom: 40 })
                .corner_radius(egui::CornerRadius::same(8))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("📭 暂无用户")
                            .size(18.0).color(Color32::from_rgb(120, 120, 120)));
                        ui.add_space(12.0);
                        ui.label(RichText::new("点击 \"➕ 添加用户\" 创建第一个用户")
                            .size(13.0).color(Color32::from_rgb(150, 150, 150)));
                    });
                });
        } else {
            egui::Frame::new()
                    .fill(Color32::from_rgb(248, 249, 250))
                    .inner_margin(egui::Margin { left: 8, right: 8, top: 6, bottom: 6 })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_sized([180.0, 16.0], egui::Label::new(RichText::new("用户名").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([280.0, 16.0], egui::Label::new(RichText::new("主目录").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([80.0, 16.0], egui::Label::new(RichText::new("权限").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.add_sized([80.0, 16.0], egui::Label::new(RichText::new("状态").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR)));
                            ui.label(RichText::new("操作").strong().size(12.0).color(styles::TEXT_PRIMARY_COLOR));
                        });
                    });

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for user in &users {
                    let row_fill = if user.enabled { 
                        Color32::WHITE 
                    } else { 
                        Color32::from_rgb(255, 250, 250)
                    };
                    
                    egui::Frame::new()
                        .fill(row_fill)
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(235, 238, 242)))
                        .inner_margin(egui::Margin { left: 8, right: 8, top: 7, bottom: 7 })
                        .corner_radius(egui::CornerRadius::same(6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_sized([180.0, 24.0], egui::Label::new(
                                    RichText::new(&user.username).size(14.0).strong().color(Color32::from_rgb(45, 55, 72))));
                                ui.add_sized([280.0, 24.0], egui::Label::new(
                                    RichText::new(&user.home_dir).size(12.0).color(Color32::from_rgb(90, 90, 90))));
                                let admin_rt = if user.is_admin {
                                    RichText::new("👑 管理员").size(12.0).color(Color32::from_rgb(108, 92, 231))
                                } else {
                                    RichText::new("👤 普通").size(12.0).color(Color32::from_rgb(120, 120, 120))
                                };
                                ui.add_sized([80.0, 24.0], egui::Label::new(admin_rt));
                                let st_col = if user.enabled { 
                                    Color32::from_rgb(16, 124, 16) 
                                } else { 
                                    Color32::from_rgb(185, 28, 28)
                                };
                                let st_icon = if user.enabled { "●" } else { "○" };
                                ui.add_sized([80.0, 24.0], egui::Label::new(
                                    RichText::new(format!("{} 启用", st_icon))
                                        .size(12.0).color(st_col)));
                                
                                ui.add_space(8.0);
                                let edit_btn = egui::Button::new(RichText::new("✏ 编辑").size(12.0))
                                    .fill(Color32::from_rgb(243, 244, 246))
                                    .corner_radius(egui::CornerRadius::same(4));
                                if ui.add(edit_btn).clicked() { to_edit = Some(user.clone()); }
                                
                                let toggle_btn = egui::Button::new(
                                    RichText::new(if user.enabled {"禁用"} else {"启用"}).size(12.0))
                                    .fill(if user.enabled { 
                                        Color32::from_rgb(254, 226, 226) 
                                    } else { 
                                        Color32::from_rgb(220, 252, 231) 
                                    })
                                    .corner_radius(egui::CornerRadius::same(4));
                                if ui.add(toggle_btn).clicked() {
                                    to_toggle = Some((user.username.clone(), !user.enabled));
                                }
                                
                                let del = egui::Button::new(RichText::new("🗑 删除").size(12.0).color(Color32::from_rgb(185, 28, 28)))
                                    .fill(Color32::from_rgb(254, 226, 226))
                                    .corner_radius(egui::CornerRadius::same(4));
                                if ui.add(del).clicked() { to_delete_confirm = Some(user.username.clone()); }
                            });
                        });
                }
            });
        }

        if let Some(u) = to_edit { self.open_edit_modal(&u); }
        if let Some((name, enabled)) = to_toggle {
            let _ = self.user_manager.set_user_enabled(&name, enabled);
            self.save();
        }
        if let Some(name) = to_delete_confirm { self.modal = ModalMode::ConfirmDelete(name); }
    }
}
