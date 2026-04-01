#![windows_subsystem = "windows"]

use eframe::{App, Frame};
use egui::{CentralPanel, RichText, Color32, IconData};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tracing_subscriber::layer::SubscriberExt;
use std::sync::Arc;
use parking_lot::RwLock;

use wftpg::core::config::Config;
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum InitState {
    Loading,
    Ready,
    Error,
}

struct InitResult;

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
    config:         Arc<RwLock<Config>>,
    server_tab:     Option<server_tab::ServerTab>,
    user_tab:       Option<user_tab::UserTab>,
    security_tab:   Option<security_tab::SecurityTab>,
    service_tab:    Option<service_tab::ServiceTab>,
    log_tab:        Option<log_tab::LogTab>,
    file_log_tab:   Option<file_log_tab::FileLogTab>,
    about_tab:      Option<about_tab::AboutTab>,
    init_state:     InitState,
    init_error:     Option<String>,
    init_receiver:  Option<mpsc::Receiver<Result<InitResult, String>>>,
    init_start_time: Instant,
    cached_styles:  CachedStyles,
}

impl WftpgApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_global_style(styles::get_custom_style());
        
        let config = Arc::new(RwLock::new(Config::load(&Config::get_config_path()).unwrap_or_default()));
        
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
            config:         config.clone(),
            server_tab:     None,
            user_tab:       None,
            security_tab:   None,
            service_tab:    None,
            log_tab:        None,
            file_log_tab:   None,
            about_tab:      None,
            init_state:     InitState::Loading,
            init_error:     None,
            init_receiver:  Some(init_rx),
            init_start_time: Instant::now(),
            cached_styles:  CachedStyles::new(),
        }
    }
    
    fn do_initialization() -> Result<InitResult, String> {
        Ok(InitResult)
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
                    Ok(_) => {
                        self.init_state = InitState::Ready;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        tracing::info!("应用初始化完成");
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
    
    fn ensure_tab_initialized(&mut self, tab_idx: usize) {
        match tab_idx {
            0 if self.server_tab.is_none() => {
                self.server_tab = Some(server_tab::ServerTab::with_config(self.config.clone()));
            }
            1 if self.user_tab.is_none() => {
                self.user_tab = Some(user_tab::UserTab::new());
            }
            2 if self.security_tab.is_none() => {
                self.security_tab = Some(security_tab::SecurityTab::with_config(self.config.clone()));
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



impl App for WftpgApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        let ctx = ui.ctx().clone();
        
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
    let mut fonts = egui::FontDefinitions::default();

    if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\simsun.ttc") {
        fonts.font_data.insert(
            "SimSun".into(),
            egui::FontData::from_owned(data).into(),
        );
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "SimSun".into());
        fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "SimSun".into());
        tracing::info!("成功加载中文字体 SimSun");
    } else {
        tracing::warn!("无法加载中文字体，将使用默认字体");
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
