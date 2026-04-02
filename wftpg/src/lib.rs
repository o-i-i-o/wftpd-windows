//! WFTPG - SFTP/FTP Management Frontend
//!
//! This library provides the core functionality for the WFTPG management frontend.
//!
//! # Architecture
//! 
//! - Configuration management with hot-reload support
//! - User management with Argon2 password hashing
//! - Comprehensive logging with tracing
//! - Windows service integration

pub mod core;
pub mod gui_egui;

use parking_lot::Mutex;
use std::sync::Arc;
use std::path::PathBuf;

use core::config::Config;
use core::users::UserManager;
use core::logger::TracingLogger;
use core::config_manager::ConfigManager;

pub struct AppState {
    pub config_manager: ConfigManager,
    pub user_manager: Arc<Mutex<UserManager>>,
    pub logger: TracingLogger,
    pub config_path: PathBuf,
    pub users_path: PathBuf,
}

impl AppState {
    pub fn new() -> anyhow::Result<Self> {
        let config_path = Config::get_config_path();
        let users_path = Config::get_users_path();
        
        let config = Config::load(&config_path)?;
        let user_manager = UserManager::load(&users_path)?;
        
        let log_dir = config.logging.log_dir.clone();
        let log_level = config.logging.log_level.clone();
        let max_log_size = config.logging.max_log_size;
        let max_log_files = config.logging.max_log_files;
        
        let logger = TracingLogger::init(&log_dir, max_log_size, max_log_files, &log_level)
            .map_err(|e| anyhow::anyhow!("Failed to initialize logger: {}", e))?;
        
        // 创建 ConfigManager 替代直接的 Arc<Mutex<Config>>
        let config_manager = ConfigManager::new(config);
        
        Ok(AppState {
            config_manager,
            user_manager: Arc::new(Mutex::new(user_manager)),
            logger,
            config_path,
            users_path,
        })
    }
    
    pub fn reload_config(&self) -> anyhow::Result<()> {
        self.config_manager.reload_from_file(&self.config_path)
            .map_err(|e| anyhow::anyhow!("Failed to reload config: {}", e))
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
