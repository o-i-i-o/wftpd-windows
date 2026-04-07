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

        tracing::info!("=== WFTPD Starting ===");
        tracing::info!("Config path: {}", config_path.display());
        tracing::info!("Users path: {}", users_path.display());

        // 先初始化基本日志系统，确保配置加载失败时也能记录日志
        let default_log_dir = Config::get_default_log_dir();
        if let Err(e) = std::fs::create_dir_all(&default_log_dir) {
            eprintln!(
                "Warning: Failed to create log directory {}: {}",
                default_log_dir, e
            );
            tracing::warn!("Failed to create default log directory: {}", e);
        }

        // 使用默认配置初始化日志系统（稍后会用配置文件中的设置重新初始化）
        match TracingLogger::init(&default_log_dir, 10 * 1024 * 1024, 10, "info") {
            Ok(logger) => logger,
            Err(e) => {
                eprintln!("CRITICAL: Failed to initialize logger: {}", e);
                return Err(anyhow::anyhow!("Failed to initialize logger: {}", e));
            }
        };

        tracing::info!("Loading configuration from {}", config_path.display());

        let config = match Config::load(&config_path) {
            Ok(c) => {
                tracing::info!("Configuration loaded successfully");

                // 验证配置
                if let Err(e) = c.validate() {
                    tracing::error!("Configuration validation failed: {}", e);
                    return Err(anyhow::anyhow!(
                        "Config validation error: {}. Please fix the configuration file.",
                        e
                    ));
                }

                tracing::debug!(
                    "Config details: FTP={}, SFTP={}",
                    c.ftp.enabled,
                    c.sftp.enabled
                );
                c
            }
            Err(e) => {
                tracing::error!("Failed to load configuration: {}", e);
                tracing::error!(
                    "Please check the configuration file at: {}",
                    config_path.display()
                );
                tracing::error!("You can restore from backup or regenerate the config file");
                return Err(anyhow::anyhow!(
                    "Configuration load failed: {}. Check logs for details.",
                    e
                ));
            }
        };

        // 使用配置文件中的日志设置重新初始化日志系统
        let log_dir = config.logging.log_dir.clone();
        let log_level = config.logging.log_level.clone();
        let max_log_size = config.logging.max_log_size;
        let max_log_files = config.logging.max_log_files;

        tracing::info!("Reinitializing logger with config settings...");
        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            eprintln!("Warning: Failed to create log directory {}: {}", log_dir, e);
            tracing::warn!("Failed to create configured log directory: {}", e);
        }

        match TracingLogger::init(&log_dir, max_log_size, max_log_files, &log_level) {
            Ok(logger) => logger,
            Err(e) => {
                tracing::error!("Failed to reinitialize logger: {}", e);
                return Err(anyhow::anyhow!("Logger reinitialization failed: {}", e));
            }
        };

        tracing::info!(
            "Logger reinitialized: level={}, dir={}, max_size={}MB, max_files={}",
            log_level,
            log_dir,
            max_log_size / (1024 * 1024),
            max_log_files
        );

        tracing::info!("Loading users from {}", users_path.display());
        let user_manager = match UserManager::load(&users_path) {
            Ok(u) => {
                tracing::info!("Users loaded successfully ({} users)", u.user_count());
                u
            }
            Err(e) => {
                tracing::error!("Failed to load users: {}", e);
                tracing::error!("Please check the users file at: {}", users_path.display());
                return Err(anyhow::anyhow!(
                    "Users load failed: {}. Check logs for details.",
                    e
                ));
            }
        };

        tracing::info!("AppState initialized successfully");
        Ok(AppState {
            config: Arc::new(Mutex::new(config)),
            user_manager: Arc::new(Mutex::new(user_manager)),
            logger: TracingLogger::init(&log_dir, max_log_size, max_log_files, &log_level)
                .map_err(|e| anyhow::anyhow!("Final logger init failed: {}", e))?,
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
