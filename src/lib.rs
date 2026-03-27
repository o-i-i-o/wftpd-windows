//! WFTPG - SFTP/FTP Server Library
//!
//! This library provides the core functionality for the WFTPG SFTP/FTP server.

pub mod core;
pub mod gui_egui;

use parking_lot::Mutex;
use std::sync::Arc;
use std::path::PathBuf;

use core::config::Config;
use core::users::UserManager;
use core::logger::TracingLogger;
use core::server_manager::ServerManager;

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
        
        let config = Config::load(&config_path)?;
        let user_manager = UserManager::load(&users_path)?;
        
        let log_dir = config.logging.log_dir.clone();
        let log_level = config.logging.log_level.clone();
        let max_log_size = config.logging.max_log_size;
        let max_log_files = config.logging.max_log_files;
        
        let logger = TracingLogger::init(&log_dir, max_log_size, max_log_files, &log_level)
            .map_err(|e| anyhow::anyhow!("Failed to initialize logger: {}", e))?;
        
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
        self.server_manager.start_ftp(
            Arc::clone(&self.config),
            Arc::clone(&self.user_manager),
            self.logger.clone(),
        )
    }
    
    pub fn stop_ftp(&self) {
        self.server_manager.stop_ftp();
    }
    
    pub fn is_ftp_running(&self) -> bool {
        self.server_manager.is_ftp_running()
    }
    
    pub fn start_sftp(&self) -> anyhow::Result<()> {
        self.server_manager.start_sftp(
            Arc::clone(&self.config),
            Arc::clone(&self.user_manager),
            self.logger.clone(),
        )
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
    
    pub fn shutdown(&self) {
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new().expect("Failed to create default AppState")
    }
}
