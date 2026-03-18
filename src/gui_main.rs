#![windows_subsystem = "windows"]

use eframe::{App, Frame};
use egui::{CentralPanel, TopBottomPanel, RichText, Color32, IconData};
use std::time::{Duration, Instant};

use wftpg::core::ipc::IpcClient;
use wftpg::core::server_manager::ServerManager;
use wftpg::gui_egui::{server_tab, user_tab, security_tab, service_tab, log_tab, file_log_tab, styles};

#[cfg(windows)]
mod admin {
    use std::process::Command;

    pub fn is_admin() -> bool {
        let output = Command::new("net")
            .args(["session"])
            .output();
        
        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }

    pub fn request_admin() {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWDEFAULT;

        let exe_path = std::env::current_exe().unwrap_or_default();
        
        fn to_wide(s: &str) -> Vec<u16> {
            OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
        }

        unsafe {
            let operation = to_wide("runas");
            let file = to_wide(exe_path.to_string_lossy().as_ref());
            let params = to_wide("");

            let _ = ShellExecuteW(
                None,
                windows::core::PCWSTR(operation.as_ptr()),
                windows::core::PCWSTR(file.as_ptr()),
                windows::core::PCWSTR(params.as_ptr()),
                None,
                SW_SHOWDEFAULT,
            );
        }
    }
}

struct WftpgApp {
    current_tab:    usize,
    server_tab:     server_tab::ServerTab,
    user_tab:       user_tab::UserTab,
    security_tab:   security_tab::SecurityTab,
    service_tab:    service_tab::ServiceTab,
    log_tab:        log_tab::LogTab,
    file_log_tab:   file_log_tab::FileLogTab,
    ftp_running:    bool,
    sftp_running:   bool,
    server_running: bool,
    show_service_install_dialog: bool,
    service_install_status: Option<(String, bool)>,
    last_refresh:   Instant,
}

impl Default for WftpgApp {
    fn default() -> Self {
        Self {
            current_tab:    0,
            server_tab:     server_tab::ServerTab::new(),
            user_tab:       user_tab::UserTab::new(),
            security_tab:   security_tab::SecurityTab::new(),
            service_tab:    service_tab::ServiceTab::new(),
            log_tab:      log_tab::LogTab::new(),
            file_log_tab: file_log_tab::FileLogTab::new(),
            ftp_running:    false,
            sftp_running:   false,
            server_running: false,
            show_service_install_dialog: false,
            service_install_status: None,
            last_refresh:   Instant::now(),
        }
    }
}

impl WftpgApp {
    fn check_server_and_service(&mut self) {
        let manager = ServerManager::new();
        let is_service_installed = manager.is_service_installed();

        if !is_service_installed {
            let current_exe = std::env::current_exe().unwrap_or_default();
            let exe_dir = current_exe.parent().unwrap_or(std::path::Path::new("."));
            let wftpd_exe = exe_dir.join("wftpd.exe");

            if wftpd_exe.exists() {
                self.show_service_install_dialog = true;
            }
        }

        self.check_server_status();
    }

    fn check_server_status(&mut self) {
        if IpcClient::is_server_running() {
            if let Ok(resp) = IpcClient::get_status() {
                self.ftp_running    = resp.ftp_running;
                self.sftp_running   = resp.sftp_running;
                self.server_running = true;
                self.server_tab.update_status(resp.ftp_running, resp.sftp_running);
            }
        } else {
            self.ftp_running    = false;
            self.sftp_running   = false;
            self.server_running = false;
        }
        self.last_refresh = Instant::now();
    }

    fn auto_refresh(&mut self) {
        // 每 3 秒自动刷新一次状态
        if self.last_refresh.elapsed() >= Duration::from_secs(3) {
            self.check_server_status();
        }
    }

    fn install_service(&mut self) {
        let manager = ServerManager::new();
        match manager.install_service() {
            Ok(_) => {
                self.service_install_status = Some(("服务安装成功！".to_string(), true));
                if let Err(e) = manager.start_service() {
                    self.service_install_status = Some((
                        format!("服务已安装，但启动失败: {}。请手动启动服务。", e),
                        false
                    ));
                }
            }
            Err(e) => {
                self.service_install_status = Some((
                    format!("服务安装失败: {}。请以管理员身份运行程序。", e),
                    false
                ));
            }
        }
    }

    fn show_service_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_service_install_dialog {
            return;
        }

        egui::Window::new("安装后台服务")
            .collapsible(false)
            .resizable(false)
            .fixed_size([480.0, 0.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(styles::SPACING_LG);
                    ui.label(RichText::new("🔔 检测到 wftpd.exe").size(styles::FONT_SIZE_LG).strong());
                    ui.add_space(styles::SPACING_SM);
                    ui.label(RichText::new("WFTPG 后台服务尚未安装。").size(styles::FONT_SIZE_MD));
                    ui.label(RichText::new("是否将 wftpd.exe 注册为 Windows 服务并启动？").size(styles::FONT_SIZE_MD));
                    ui.add_space(styles::SPACING_LG);

                    if let Some((msg, success)) = &self.service_install_status {
                        let color = if *success { styles::SUCCESS_COLOR } else { styles::DANGER_COLOR };
                        ui.label(RichText::new(msg).color(color).size(styles::FONT_SIZE_MD));
                        ui.add_space(styles::SPACING_LG);

                        if ui.add(styles::secondary_button("关闭")).clicked() {
                            self.show_service_install_dialog = false;
                            self.service_install_status = None;
                        }
                    } else {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = styles::SPACING_MD;

                            if ui.add(styles::secondary_button("稍后手动安装")).clicked() {
                                self.show_service_install_dialog = false;
                            }

                            if ui.add(styles::primary_button("安装并启动服务")).clicked() {
                                self.install_service();
                            }
                        });
                    }
                    ui.add_space(styles::SPACING_MD);
                });
            });
    }
}

impl App for WftpgApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        // 自动刷新状态
        self.auto_refresh();
        
        self.show_service_dialog(ctx);
        ctx.set_style(styles::get_custom_style());

        // 顶部标题栏 - 移除刷新按钮
        TopBottomPanel::top("header")
            .frame(egui::Frame::new()
                .fill(styles::BG_HEADER)
                .inner_margin(egui::Margin::symmetric(20, 12)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("◉").size(22.0).color(styles::PRIMARY_COLOR));
                    ui.add_space(8.0);
                    ui.label(RichText::new("WFTPG").size(22.0).strong().color(Color32::WHITE));
                    ui.add_space(12.0);
                    ui.label(RichText::new("SFTP/FTP 管理工具").size(14.0)
                        .color(styles::TEXT_MUTED_COLOR));
                });
            });

        // 底部状态栏 - 优化布局
        TopBottomPanel::bottom("status_bar")
            .frame(egui::Frame::new()
                .fill(styles::BG_SECONDARY)
                .stroke(egui::Stroke::new(1.0, styles::BORDER_COLOR))
                .inner_margin(egui::Margin::symmetric(20, 10)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let ftp_col  = if self.ftp_running  { styles::SUCCESS_COLOR } else { styles::DANGER_COLOR };
                    let sftp_col = if self.sftp_running { styles::SUCCESS_COLOR } else { styles::DANGER_COLOR };
                    let srv_col  = if self.server_running { styles::SUCCESS_COLOR } else { styles::TEXT_SECONDARY_COLOR };

                    // FTP 状态
                    ui.label(RichText::new("●").size(14.0).color(ftp_col));
                    ui.label(RichText::new("FTP").size(14.0).strong().color(styles::TEXT_PRIMARY_COLOR));
                    ui.label(RichText::new(if self.ftp_running { "运行中" } else { "已停止" })
                        .size(12.0).color(ftp_col));
                    
                    ui.add_space(20.0);
                    ui.label(RichText::new("|").color(styles::BORDER_COLOR));
                    ui.add_space(20.0);
                    
                    // SFTP 状态
                    ui.label(RichText::new("●").size(14.0).color(sftp_col));
                    ui.label(RichText::new("SFTP").size(14.0).strong().color(styles::TEXT_PRIMARY_COLOR));
                    ui.label(RichText::new(if self.sftp_running { "运行中" } else { "已停止" })
                        .size(12.0).color(sftp_col));
                    
                    ui.add_space(20.0);
                    ui.label(RichText::new("|").color(styles::BORDER_COLOR));
                    ui.add_space(20.0);
                    
                    // 服务状态
                    ui.label(RichText::new("●").size(14.0).color(srv_col));
                    ui.label(RichText::new("后台服务").size(14.0).strong().color(styles::TEXT_PRIMARY_COLOR));
                    ui.label(RichText::new(if self.server_running { "在线" } else { "离线" })
                        .size(12.0).color(srv_col));
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("WFTPG v3.0.0").size(12.0).color(styles::TEXT_SECONDARY_COLOR));
                    });
                });
            });

        // 中央面板
        CentralPanel::default()
            .frame(egui::Frame::new().fill(styles::BG_PRIMARY))
            .show(ctx, |ui| {
                ui.add_space(12.0);
                
                // Tab 导航栏 - 优化布局
                styles::card_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let tabs = [
                            ("⚙", "服务器",   0usize),
                            ("👤", "用户管理", 1),
                            ("🔒", "安全设置", 2),
                            ("🖥", "系统服务", 3),
                            ("📋", "运行日志", 4),
                            ("📁", "文件日志", 5),
                        ];
                        
                        for (icon, label, idx) in &tabs {
                            let selected = self.current_tab == *idx;
                            let text = RichText::new(format!("{} {}", icon, label))
                                .size(14.0)
                                .strong()
                                .color(if selected { styles::PRIMARY_COLOR } else { styles::TEXT_SECONDARY_COLOR });
                            
                            let btn = egui::Button::new(text)
                                .fill(if selected { 
                                    Color32::from_rgb(243, 232, 255) 
                                } else { 
                                    Color32::TRANSPARENT 
                                })
                                .stroke(if selected {
                                    egui::Stroke::new(1.5, styles::PRIMARY_COLOR)
                                } else {
                                    egui::Stroke::NONE
                                })
                                .corner_radius(egui::CornerRadius::same(6))
                                .min_size(egui::vec2(100.0, 40.0));
                            
                            let resp = ui.add(btn);
                            if resp.clicked() { self.current_tab = *idx; }
                        }
                    });
                });

                ui.add_space(12.0);

                // Tab 内容
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        styles::card_frame().show(ui, |ui| {
                            match self.current_tab {
                                0 => self.server_tab.ui(ui),
                                1 => self.user_tab.ui(ui),
                                2 => self.security_tab.ui(ui),
                                3 => self.service_tab.ui(ui),
                                4 => self.log_tab.ui(ui),
                                5 => self.file_log_tab.ui(ui),
                                _ => self.server_tab.ui(ui),
                            }
                        });
                    });
                    
                ui.add_space(12.0);
            });
    }
}

fn setup_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    let candidates = [
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyhbd.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
    ];
    for path in &candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert("chinese".into(), FontData::from_owned(data).into());
            fonts.families.get_mut(&FontFamily::Proportional).unwrap().push("chinese".into());
            fonts.families.get_mut(&FontFamily::Monospace).unwrap().push("chinese".into());
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn load_icon() -> Option<IconData> {
    let exe_dir = std::env::current_exe().ok()?
        .parent()?
        .to_path_buf();
    
    let icon_path = exe_dir.join("ui/wftpg.ico");
    if !icon_path.exists() {
        return None;
    }
    
    let data = std::fs::read(&icon_path).ok()?;
    let icon = ico::IconDir::read(std::io::Cursor::new(&data)).ok()?;
    for entry in icon.entries() {
        if let Ok(image) = entry.decode() {
            let rgba = image.rgba_data().to_vec();
            let width = entry.width();
            let height = entry.height();
            return Some(IconData { rgba, width, height });
        }
    }
    None
}

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    {
        if !admin::is_admin() {
            admin::request_admin();
            std::process::exit(0);
        }
    }

    let icon = load_icon();
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([900.0, 650.0])
            .with_resizable(true)
            .with_icon(icon.unwrap_or_else(|| {
                IconData {
                    rgba: vec![0; 32 * 32 * 4],
                    width: 32,
                    height: 32,
                }
            })),
        ..Default::default()
    };
    
    eframe::run_native(
        "WFTPG - SFTP/FTP 管理工具",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            let mut app = WftpgApp::default();
            app.check_server_and_service();
            Ok(Box::new(app))
        }),
    )
}
