//! WFTPG - FTP/SFTP 服务器库
//!
//! 提供 FTP 和 SFTP 服务器的核心功能，包括配置管理、用户管理、服务器生命周期管理

pub mod core;

use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;

use core::config::Config;
use core::logger::TracingLogger;
use core::server_manager::ServerManager;
use core::users::UserManager;

pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub user_manager: Arc<Mutex<UserManager>>,
    pub logger: TracingLogger,
    server_manager: ServerManager,
    pub config_path: PathBuf,
    pub users_path: PathBuf,
}

impl AppState {
    pub fn new() -> anyhow::Result<Self> {
        let config_path = Config::get_config_path();
        let users_path = Config::get_users_path();

        // 先初始化基本日志系统，确保配置加载失败时也能记录日志
        let default_log_dir = Config::get_default_log_dir();
        if let Err(e) = std::fs::create_dir_all(&default_log_dir) {
            eprintln!("Warning: Failed to create log directory {}: {}", default_log_dir, e);
        }
        
        // 使用默认配置初始化日志系统
        let logger = TracingLogger::init(&default_log_dir, 10 * 1024 * 1024, 10, "info")
            .map_err(|e| anyhow::anyhow!("Failed to initialize logger: {}", e))?;

        tracing::info!("Loading configuration from {}", config_path.display());

        let config = match Config::load(&config_path) {
            Ok(c) => {
                tracing::info!("Configuration loaded successfully");
                c
            }
            Err(e) => {
                tracing::error!("Failed to load configuration: {}", e);
                return Err(e);
            }
        };

        tracing::info!("Loading users from {}", users_path.display());
        let user_manager = match UserManager::load(&users_path) {
            Ok(u) => {
                tracing::info!("Users loaded successfully ({} users)", u.user_count());
                u
            }
            Err(e) => {
                tracing::error!("Failed to load users: {}", e);
                return Err(e);
            }
        };

        Ok(AppState {
            config: Arc::new(Mutex::new(config)),
            user_manager: Arc::new(Mutex::new(user_manager)),
            logger,
            server_manager: ServerManager::new(),
            config_path,
            users_path,
        })
    }

    pub fn start_ftp(&self) -> anyhow::Result<()> {
        self.server_manager
            .start_ftp(Arc::clone(&self.config), Arc::clone(&self.user_manager))
    }

    pub fn stop_ftp(&self) {
        self.server_manager.stop_ftp();
    }

    pub fn is_ftp_running(&self) -> bool {
        self.server_manager.is_ftp_running()
    }

    pub fn start_sftp(&self) -> anyhow::Result<()> {
        self.server_manager
            .start_sftp(Arc::clone(&self.config), Arc::clone(&self.user_manager))
    }

    pub fn stop_sftp(&self) {
        self.server_manager.stop_sftp();
    }

    pub fn is_sftp_running(&self) -> bool {
        self.server_manager.is_sftp_running()
    }

    pub fn start_all(&self) -> anyhow::Result<()> {
        let (ftp_enabled, sftp_enabled) = {
            let config = self.config.lock();
            (config.ftp.enabled, config.sftp.enabled)
        };

        if ftp_enabled {
            self.start_ftp()?;
        }
        if sftp_enabled {
            self.start_sftp()?;
        }

        Ok(())
    }

    pub fn stop_all(&self) {
        self.stop_ftp();
        self.stop_sftp();
    }

    pub fn reload_config(&self) -> anyhow::Result<()> {
        let config = crate::core::config::Config::load(&self.config_path)?;
        let mut current_config = self.config.lock();
        *current_config = config;
        Ok(())
    }

    pub fn reload_users(&self) -> anyhow::Result<()> {
        let users = crate::core::users::UserManager::load(&self.users_path)?;
        let mut current_users = self.user_manager.lock();
        *current_users = users;
        Ok(())
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new().expect("Failed to create default AppState")
    }
}
