#![windows_subsystem = "windows"]

use eframe::{App, Frame};
use egui::{CentralPanel, RichText, Color32, IconData};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tracing_subscriber::layer::SubscriberExt;

use wftpg::core::config::Config;
use wftpg::core::server_manager::ServerManager;
use wftpg::core::config_manager::ConfigManager;
use wftpg::core::config_watcher::ConfigWatcher;
use wftpg::gui_egui::{about_tab, file_log_tab, log_tab, security_tab, server_tab, service_tab, styles, user_tab};

#[cfg(windows)]
mod admin {
    use windows::Win32::UI::Shell::IsUserAnAdmin;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::core::PCWSTR;

    pub fn is_running_as_admin() -> bool {
        unsafe {
            let result = IsUserAnAdmin();
            result.as_bool()
        }
    }

    pub fn request_admin_restart() -> Result<(), String> {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("无法获取当前程序路径: {}", e))?;

        let exe_path_str = exe_path.to_string_lossy().to_string();
        let mut wide_path: Vec<u16> = exe_path_str.encode_utf16().collect();
        wide_path.push(0);

        let verb: Vec<u16> = "runas\0".encode_utf16().collect();

        unsafe {
            let result = ShellExecuteW(
                None,
                PCWSTR(verb.as_ptr()),
                PCWSTR(wide_path.as_ptr()),
                PCWSTR::null(),
                None,
                windows::Win32::UI::WindowsAndMessaging::SW_SHOW,
            );

            let result_val = result.0 as i32;
            if result_val <= 32 {
                return Err(format!("请求管理员权限失败，错误代码: {}", result_val));
            }
        }

        Ok(())
    }

    pub fn ensure_admin_or_restart() -> bool {
        if is_running_as_admin() {
            return true;
        }

        match request_admin_restart() {
            Ok(()) => {
                std::process::exit(0);
            }
            Err(e) => {
                tracing::error!("请求管理员权限失败: {}", e);
                false
            }
        }
    }
}

const INIT_TIMEOUT_SECS: u64 = 10;
const SERVICE_INSTALL_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq)]
enum InitState {
    Loading,
    Ready,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
enum ServiceInstallStatus {
    None,
    Installing,
    Success(String),
    Failed(String),
}

struct InitResult {
    show_service_dialog: bool,
}

struct CachedStyles {
    loading_frame: egui::Frame,
    error_frame: egui::Frame,
    main_frame: egui::Frame,
    tab_frame: egui::Frame,
}

impl CachedStyles {
    fn new() -> Self {
        Self {
            loading_frame: egui::Frame::new().fill(styles::BG_PRIMARY),
            error_frame: egui::Frame::new().fill(styles::BG_PRIMARY),
            main_frame: egui::Frame::new()
                .fill(styles::BG_PRIMARY)
                .inner_margin(egui::Margin::same(16)),
            tab_frame: egui::Frame::new()
                .fill(styles::BG_CARD)
                .stroke(egui::Stroke::new(1.0, styles::BORDER_COLOR))
                .inner_margin(egui::Margin::symmetric(20, 12))
                .corner_radius(egui::CornerRadius::same(8)),
        }
    }
}

struct WftpgApp {
    current_tab:    usize,
    config_manager: ConfigManager,
    server_tab:     Option<server_tab::ServerTab>,
    user_tab:       Option<user_tab::UserTab>,
    security_tab:   Option<security_tab::SecurityTab>,
    service_tab:    Option<service_tab::ServiceTab>,
    log_tab:        Option<log_tab::LogTab>,
    file_log_tab:   Option<file_log_tab::FileLogTab>,
    about_tab:      Option<about_tab::AboutTab>,
    show_service_install_dialog: bool,
    service_install_status: ServiceInstallStatus,
    service_install_receiver: Option<mpsc::Receiver<Result<(), String>>>,
    service_install_start_time: Option<Instant>,
    init_state:     InitState,
    init_error:     Option<String>,
    init_receiver:  Option<mpsc::Receiver<Result<InitResult, String>>>,
    init_start_time: Instant,
    cached_styles:  CachedStyles,
    config_watcher: Option<ConfigWatcher>,
    pending_unset_topmost: bool,  // 标记是否需要在首次交互后取消置顶
}

impl WftpgApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_global_style(styles::get_custom_style());
        
        let config = Config::load(&Config::get_config_path()).unwrap_or_default();
        let config_manager = ConfigManager::new(config);
        
        let (init_tx, init_rx) = mpsc::channel();
        let ctx_clone = cc.egui_ctx.clone();
        
        std::thread::spawn(move || {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::do_initialization()
            }));
            
            let init_result = match res {
                Ok(Ok(result)) => Ok(result),
                Ok(Err(e)) => Err(e),
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_info.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "未知错误".to_string()
                    };
                    Err(format!("初始化时发生 panic: {}", msg))
                }
            };
            
            let _ = init_tx.send(init_result);
            ctx_clone.request_repaint();
        });
        
        Self {
            current_tab:    0,
            config_manager: config_manager.clone(),
            server_tab:     None,
            user_tab:       None,
            security_tab:   None,
            service_tab:    None,
            log_tab:        None,
            file_log_tab:   None,
            about_tab:      None,
            show_service_install_dialog: false,
            service_install_status: ServiceInstallStatus::None,
            service_install_receiver: None,
            service_install_start_time: None,
            init_state:     InitState::Loading,
            init_error:     None,
            init_receiver:  Some(init_rx),
            init_start_time: Instant::now(),
            cached_styles:  CachedStyles::new(),
            config_watcher: None,
            pending_unset_topmost: false,  // 初始化为 false
        }
    }
    
    fn do_initialization() -> Result<InitResult, String> {
        let manager = ServerManager::new();
        let is_service_installed = manager.is_service_installed();
        let mut show_service_dialog = false;

        if !is_service_installed
            && let Ok(current_exe) = std::env::current_exe()
            && let Some(exe_dir) = current_exe.parent() {
                let wftpd_exe = exe_dir.join("wftpd.exe");
                if wftpd_exe.exists() {
                    show_service_dialog = true;
                }
            }

        Ok(InitResult {
            show_service_dialog,
        })
    }
    
    /// 初始化配置文件监听器
    fn init_config_watcher(&mut self) {
        let config_path = Config::get_config_path();
        self.config_watcher = Some(
            ConfigWatcher::new(
                &config_path,
                self.config_manager.clone(),
            )
        );
        tracing::info!("Configuration watcher initialized");
    }
    
    fn check_init_result(&mut self, ctx: &egui::Context) {
        if self.init_start_time.elapsed() >= Duration::from_secs(INIT_TIMEOUT_SECS) {
            self.init_receiver = None;
            self.init_error = Some(format!(
                "初始化超时（{}秒），请检查程序权限或查看日志。",
                INIT_TIMEOUT_SECS
            ));
            self.init_state = InitState::Error;
            tracing::error!("应用初始化超时");
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            return;
        }

        if let Some(rx) = &self.init_receiver
            && let Ok(result) = rx.try_recv() {
                self.init_receiver = None;

                match result {
                    Ok(init_result) => {
                        self.show_service_install_dialog = init_result.show_service_dialog;
                        self.init_state = InitState::Ready;
                        // 初始化配置监听器
                        self.init_config_watcher();
                        
                        // 显示窗口并设置启动时置顶（可被其他窗口挤占）
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        // 方案 1: 使用 AlwaysOnTop + 后续手动降级（推荐，兼容性最好）
                        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                            egui::WindowLevel::AlwaysOnTop
                        ));
                        // 标记需要在首次交互后取消置顶
                        self.pending_unset_topmost = true;
                        
                        tracing::info!("应用初始化完成，配置监听器已启动，窗口已置顶");
                    }
                    Err(e) => {
                        self.init_error = Some(e);
                        self.init_state = InitState::Error;
                        tracing::error!("应用初始化失败");
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    }
                }
            }
    }
    
    fn check_service_install_result(&mut self) {
        if let Some(start_time) = self.service_install_start_time
            && start_time.elapsed() >= Duration::from_secs(SERVICE_INSTALL_TIMEOUT_SECS)
        {
            self.service_install_receiver = None;
            self.service_install_start_time = None;
            self.service_install_status = ServiceInstallStatus::Failed(
                format!("服务安装超时（{}秒），请检查服务状态或手动安装。", SERVICE_INSTALL_TIMEOUT_SECS)
            );
            return;
        }
        
        if let Some(rx) = &self.service_install_receiver
            && let Ok(result) = rx.try_recv() {
                self.service_install_receiver = None;
                self.service_install_start_time = None;
                
                match result {
                    Ok(_) => {
                        self.service_install_status = ServiceInstallStatus::Success(
                            "服务安装并启动成功！".to_string()
                        );
                    }
                    Err(e) => {
                        self.service_install_status = ServiceInstallStatus::Failed(e);
                    }
                }
            }
    }
    
    fn ensure_tab_initialized(&mut self, tab_idx: usize) {
        match tab_idx {
            0 if self.server_tab.is_none() => {
                self.server_tab = Some(server_tab::ServerTab::new(self.config_manager.clone()));
            }
            1 if self.user_tab.is_none() => {
                self.user_tab = Some(user_tab::UserTab::new());
            }
            2 if self.security_tab.is_none() => {
                self.security_tab = Some(security_tab::SecurityTab::new(self.config_manager.clone()));
            }
            3 if self.service_tab.is_none() => {
                self.service_tab = Some(service_tab::ServiceTab::new());
            }
            4 if self.log_tab.is_none() => {
                self.log_tab = Some(log_tab::LogTab::new());
            }
            5 if self.file_log_tab.is_none() => {
                self.file_log_tab = Some(file_log_tab::FileLogTab::new());
            }
            6 if self.about_tab.is_none() => {
                self.about_tab = Some(about_tab::AboutTab::new());
            }
            _ => {}
        }
    }
}

impl WftpgApp {
    fn install_service(&mut self, ctx: &egui::Context) {
        self.service_install_status = ServiceInstallStatus::Installing;
        self.service_install_start_time = Some(Instant::now());
        
        let (tx, rx) = mpsc::channel();
        self.service_install_receiver = Some(rx);
        
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let manager = ServerManager::new();
                manager.install_service()
                    .and_then(|_| manager.start_service())
            }));
            
            let final_result = match result {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(e)) => Err(format!("服务安装失败：{}。请以管理员身份运行程序。", e)),
                Err(panic_info) => {
                    let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_info.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "未知错误".to_string()
                    };
                    Err(format!("服务安装时发生 panic: {}", msg))
                }
            };
            
            let _ = tx.send(final_result);
            ctx_clone.request_repaint();
        });
    }

    fn show_service_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_service_install_dialog {
            return;
        }

        egui::Window::new("安装后台服务")
            .collapsible(false)
            .resizable(false)
            .fixed_size([520.0, 0.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(styles::SPACING_LG);
                    ui.label(RichText::new("🔔 检测到 wftpd.exe").size(styles::FONT_SIZE_LG).strong());
                    ui.add_space(styles::SPACING_SM);
                    ui.label(RichText::new("WFTPG 后台服务尚未安装。").size(styles::FONT_SIZE_MD));
                    ui.label(RichText::new("是否将 wftpd.exe 注册为 Windows 服务并启动？").size(styles::FONT_SIZE_MD));
                    ui.add_space(styles::SPACING_LG);

                    match &self.service_install_status {
                        ServiceInstallStatus::Installing => {
                            ui.vertical_centered(|ui| {
                                ui.spinner();
                                ui.add_space(styles::SPACING_MD);
                                ui.label(RichText::new("正在安装服务...").size(styles::FONT_SIZE_MD));
                            });
                            ui.add_space(styles::SPACING_SM);
                            ui.label(RichText::new("这可能需要几秒钟，请稍候...").size(styles::FONT_SIZE_SM).color(styles::TEXT_MUTED_COLOR));
                        }
                        ServiceInstallStatus::Success(msg) => {
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(msg).color(styles::SUCCESS_COLOR).size(styles::FONT_SIZE_MD));
                            });
                            ui.add_space(styles::SPACING_LG);
                            ui.vertical_centered(|ui| {
                                if ui.add(styles::secondary_button("关闭")).clicked() {
                                    self.show_service_install_dialog = false;
                                    self.service_install_status = ServiceInstallStatus::None;
                                }
                            });
                        }
                        ServiceInstallStatus::Failed(msg) => {
                            ui.label(RichText::new(msg).color(styles::DANGER_COLOR).size(styles::FONT_SIZE_MD));
                            ui.add_space(styles::SPACING_SM);
                            
                            egui::Frame::new()
                                .fill(styles::BG_SECONDARY)
                                .inner_margin(egui::Margin::same(8))
                                .corner_radius(egui::CornerRadius::same(4))
                                .show(ui, |ui| {
                                    ui.label(RichText::new("手动安装命令:").size(styles::FONT_SIZE_SM).strong());
                                    ui.label(RichText::new("sc create wftpd binPath= \"<安装目录>\\wftpd.exe\" start= auto").size(styles::FONT_SIZE_SM).color(styles::TEXT_MUTED_COLOR));
                                    ui.add_space(styles::SPACING_XS);
                                    ui.label(RichText::new("sc start wftpd").size(styles::FONT_SIZE_SM).color(styles::TEXT_MUTED_COLOR));
                                });
                            
                            ui.add_space(styles::SPACING_LG);

                            ui.horizontal_centered(|ui| {
                                if ui.add(styles::secondary_button("关闭")).clicked() {
                                    self.show_service_install_dialog = false;
                                    self.service_install_status = ServiceInstallStatus::None;
                                }
                                ui.add_space(styles::SPACING_MD);
                                if ui.add(styles::primary_button("重试")).clicked() {
                                    self.install_service(ctx);
                                }
                            });
                        }
                        ServiceInstallStatus::None => {
                            ui.vertical_centered(|ui| {
                                if ui.add(styles::secondary_button("稍后手动安装")).clicked() {
                                    self.show_service_install_dialog = false;
                                }
                                ui.add_space(styles::SPACING_MD);
                                if ui.add(styles::primary_button("安装并启动服务")).clicked() {
                                    self.install_service(ctx);
                                }
                            });
                        }
                    }
                    ui.add_space(styles::SPACING_MD);
                });
            });
    }
}



impl App for WftpgApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        let ctx = ui.ctx().clone();
        
        // 检查配置文件变更并自动重载
        if let Some(watcher) = &mut self.config_watcher
            && watcher.check_and_reload() {
            tracing::info!("Configuration auto-reloaded, refreshing UI...");
        }
        
        // 如果标记了需要取消置顶，在检测到用户交互时执行
        if self.pending_unset_topmost {
            // 检测是否有输入事件（鼠标或键盘）
            let has_input = ctx.input(|i| !i.events.is_empty());
            if has_input {
                // 有事件发生，取消置顶
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::Normal
                ));
                self.pending_unset_topmost = false;
                tracing::debug!("窗口已降级为普通窗口（用户交互后）");
            }
        }
        
        match self.init_state {
            InitState::Loading => {
                self.check_init_result(&ctx);
                
                CentralPanel::default()
                    .frame(self.cached_styles.loading_frame)
                    .show_inside(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(ui.available_height() / 2.0 - 50.0);
                            ui.spinner();
                            ui.add_space(styles::SPACING_MD);
                            ui.label(RichText::new("正在初始化...").size(styles::FONT_SIZE_LG).color(styles::TEXT_SECONDARY_COLOR));
                        });
                    });
                return;
            }
            InitState::Error => {
                CentralPanel::default()
                    .frame(self.cached_styles.error_frame)
                    .show_inside(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(ui.available_height() / 2.0 - 80.0);
                            ui.label(RichText::new("⚠ 初始化失败").size(styles::FONT_SIZE_LG).strong().color(styles::DANGER_COLOR));
                            ui.add_space(styles::SPACING_MD);
                            if let Some(error) = &self.init_error {
                                ui.label(RichText::new(error).size(styles::FONT_SIZE_MD).color(styles::TEXT_SECONDARY_COLOR));
                            }
                            ui.add_space(styles::SPACING_LG);
                            if ui.add(styles::primary_button("退出程序")).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    });
                return;
            }
            InitState::Ready => {}
        }
        
        self.check_service_install_result();
        self.show_service_dialog(&ctx);

        CentralPanel::default()
            .frame(self.cached_styles.main_frame)
            .show_inside(ui, |ui| {
                ui.add_space(12.0);
                
                self.cached_styles.tab_frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        
                        let tabs = [
                            ("⚙", "服务器",   0usize),
                            ("👤", "用户管理", 1),
                            ("🔒", "安全设置", 2),
                            ("🖥", "系统服务", 3),
                            ("📋", "运行日志", 4),
                            ("📁", "文件日志", 5),
                            ("ℹ", "关于",     6),
                        ];
                        
                        for (icon, label, idx) in &tabs {
                            let selected = self.current_tab == *idx;
                            
                            let bg_color = if selected {
                                Color32::from_rgb(243, 232, 255)
                            } else {
                                Color32::TRANSPARENT
                            };
                            
                            let text_color = if selected {
                                styles::PRIMARY_COLOR
                            } else {
                                styles::TEXT_SECONDARY_COLOR
                            };
                            
                            let text = RichText::new(format!("{icon}  {label}"))
                                .size(14.0)
                                .strong()
                                .color(text_color);
                            
                            let btn = egui::Button::new(text)
                                .fill(bg_color)
                                .stroke(if selected {
                                    egui::Stroke::new(1.5, styles::PRIMARY_COLOR)
                                } else {
                                    egui::Stroke::new(1.0, styles::BORDER_COLOR)
                                })
                                .corner_radius(egui::CornerRadius::same(6))
                                .min_size(egui::vec2(110.0, 42.0));
                            
                            let resp = ui.add(btn);
                            if resp.clicked() { 
                                self.current_tab = *idx; 
                            }
                        }
                    });
                });

                ui.add_space(12.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.ensure_tab_initialized(self.current_tab);
                        match self.current_tab {
                            0 => self.server_tab.as_mut().unwrap().ui(ui),
                            1 => self.user_tab.as_mut().unwrap().ui(ui),
                            2 => self.security_tab.as_mut().unwrap().ui(ui),
                            3 => self.service_tab.as_mut().unwrap().ui(ui),
                            4 => self.log_tab.as_mut().unwrap().ui(ui),
                            5 => self.file_log_tab.as_mut().unwrap().ui(ui),
                            6 => self.about_tab.as_mut().unwrap().ui(ui),
                            _ => self.server_tab.as_mut().unwrap().ui(ui),
                        }
                    });
                    
                ui.add_space(12.0);
            });
    }
    
    /// 在应用程序退出前调用，用于清理 egui 资源
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        tracing::info!("GUI 应用程序即将关闭，正在清理 egui 资源...");
    }
}

fn setup_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();

    let candidates = [
        ("C:\\Windows\\Fonts\\seguisym.ttf", "Segoe UI Symbol"),
        ("C:\\Windows\\Fonts\\msyh.ttc", "Microsoft YaHei"),
        ("C:\\Windows\\Fonts\\msyhbd.ttc", "Microsoft YaHei Bold"),
        ("C:\\Windows\\Fonts\\simsun.ttc", "SimSun"),
        ("C:\\Windows\\Fonts\\simhei.ttf", "SimHei"),
    ];

    for (path, name) in &candidates {
        match std::fs::read(path) {
            Ok(data) => {
                fonts.font_data.insert((*name).into(), FontData::from_owned(data).into());
                if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                    family.push((*name).into());
                }
                if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
                    family.push((*name).into());
                }
                tracing::info!("成功加载字体：{}", name);
            }
            Err(e) => {
                tracing::warn!("加载字体 {} 失败：{}", path, e);
            }
        }
    }

    ctx.set_fonts(fonts);
}

fn load_icon() -> IconData {
    const ICON_BYTES: &[u8] = include_bytes!("../ui/wftpg.ico");

    match ico::IconDir::read(std::io::Cursor::new(ICON_BYTES)) {
        Ok(icon) => {
            for entry in icon.entries() {
                if let Ok(image) = entry.decode() {
                    let rgba = image.rgba_data().to_vec();
                    let width = entry.width();
                    let height = entry.height();
                    tracing::info!("成功加载内嵌图标: {}x{}", width, height);
                    return IconData { rgba, width, height };
                }
            }
            tracing::warn!("内嵌图标中没有可解码的图像");
            create_default_icon()
        }
        Err(e) => {
            tracing::error!("解析内嵌图标文件失败: {}", e);
            create_default_icon()
        }
    }
}

fn create_default_icon() -> IconData {
    let size = 32u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    
    for y in 0..size {
        for x in 0..size {
            let dx = (x as i32 - 16).abs();
            let dy = (y as i32 - 16).abs();
            let dist = ((dx * dx + dy * dy) as f64).sqrt();
            
            let (r, g, b, a) = if dist < 14.0 {
                (108, 92, 231, 255)
            } else if dist < 16.0 {
                (80, 70, 200, 255)
            } else {
                (0, 0, 0, 0)
            };
            
            rgba.push(r);
            rgba.push(g);
            rgba.push(b);
            rgba.push(a);
        }
    }
    
    IconData {
        rgba,
        width: size,
        height: size,
    }
}

fn main() -> eframe::Result<()> {
    init_tracing_for_gui();

    #[cfg(windows)]
    {
        if !admin::ensure_admin_or_restart() {
            tracing::error!("程序需要管理员权限才能运行");
            std::process::exit(1);
        }
    }

    let icon = load_icon();

    let persistence_path = std::path::PathBuf::from("C:\\ProgramData\\wftpg\\gui_state");
    if let Some(parent) = persistence_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size([900.0, 650.0])
            .with_resizable(true)
            .with_visible(false)
            .with_active(true)
            .with_icon(icon),
        persist_window: true,
        persistence_path: Some(persistence_path),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "WFTPG - SFTP/FTP 管理工具",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(WftpgApp::new(cc)))
        }),
    )
}

fn init_tracing_for_gui() {
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(false)
            .with_thread_ids(false)
            .with_thread_names(false)
            .compact());
    
    let _ = tracing::subscriber::set_global_default(subscriber);
}
