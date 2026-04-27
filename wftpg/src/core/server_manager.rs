//! Server Manager for Windows Service management
//!
//! This module provides Windows service management functionality
//! for the wftpd service backend using safe windows-service crate.

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::time::Duration;
use windows_service::service::{
    ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState, ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

const SERVICE_NAME: &str = "wftpd";
const SERVICE_WAIT_MAX_ATTEMPTS: u32 = 30;
const SERVICE_WAIT_INTERVAL_MS: u64 = 500;

pub struct ServerManager;

impl Default for ServerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerManager {
    pub fn new() -> Self {
        ServerManager
    }

    fn connect_manager(&self, access: ServiceManagerAccess) -> Result<ServiceManager> {
        ServiceManager::local_computer(None::<&str>, access)
            .context("Failed to connect to service control manager")
    }

    pub fn is_service_installed(&self) -> bool {
        let manager = match self.connect_manager(ServiceManagerAccess::CONNECT) {
            Ok(m) => m,
            Err(_) => return false,
        };

        manager
            .open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS)
            .is_ok()
    }

    pub fn is_service_running(&self) -> bool {
        let manager = match self.connect_manager(ServiceManagerAccess::CONNECT) {
            Ok(m) => m,
            Err(_) => return false,
        };

        let service = match manager.open_service(
            SERVICE_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::START,
        ) {
            Ok(s) => s,
            Err(_) => return false,
        };

        match service.query_status() {
            Ok(status) => status.current_state == ServiceState::Running,
            Err(_) => false,
        }
    }

    pub fn install_service(&self) -> Result<()> {
        let exe_path = std::env::current_exe().context("Failed to get current executable path")?;

        let exe_dir = exe_path
            .parent()
            .context("Failed to get executable directory")?;

        let service_exe = exe_dir.join("wftpd.exe");

        if !service_exe.exists() {
            anyhow::bail!(
                "Backend service executable not found: {}",
                service_exe.display()
            );
        }

        let manager = self.connect_manager(
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )?;

        let service_info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from("WFTPD Service"),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: service_exe,
            launch_arguments: vec![],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };

        manager
            .create_service(
                &service_info,
                ServiceAccess::CHANGE_CONFIG | ServiceAccess::START,
            )
            .context("Failed to create service")?;

        tracing::info!("Service installed successfully: {}", SERVICE_NAME);
        Ok(())
    }

    pub fn uninstall_service(&self) -> Result<()> {
        let manager = self.connect_manager(ServiceManagerAccess::CONNECT)?;

        let service = manager
            .open_service(
                SERVICE_NAME,
                ServiceAccess::DELETE | ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
            )
            .context("Failed to open service")?;

        service.delete().context("Failed to delete service")?;

        tracing::info!("Service uninstalled successfully: {}", SERVICE_NAME);
        Ok(())
    }

    pub fn start_service(&self) -> Result<()> {
        let manager = self.connect_manager(ServiceManagerAccess::CONNECT)?;

        let service = manager
            .open_service(
                SERVICE_NAME,
                ServiceAccess::START | ServiceAccess::QUERY_STATUS,
            )
            .context("Failed to open service")?;

        service
            .start(&[] as &[&std::ffi::OsStr])
            .context("Failed to start service")?;

        for _ in 0..SERVICE_WAIT_MAX_ATTEMPTS {
            let status = service.query_status()?;
            if status.current_state == ServiceState::Running {
                break;
            }
            if status.current_state == ServiceState::Stopped {
                anyhow::bail!(
                    "Service stopped immediately after start, check service configuration"
                );
            }
            std::thread::sleep(Duration::from_millis(SERVICE_WAIT_INTERVAL_MS));
        }

        tracing::info!("Service started successfully: {}", SERVICE_NAME);
        Ok(())
    }

    pub fn stop_service(&self) -> Result<()> {
        let manager = self.connect_manager(ServiceManagerAccess::CONNECT)?;

        let service = manager
            .open_service(
                SERVICE_NAME,
                ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
            )
            .context("Failed to open service")?;

        service.stop().context("Failed to stop service")?;

        for _ in 0..SERVICE_WAIT_MAX_ATTEMPTS {
            let status = service.query_status()?;
            if status.current_state == ServiceState::Stopped {
                break;
            }
            std::thread::sleep(Duration::from_millis(SERVICE_WAIT_INTERVAL_MS));
        }

        tracing::info!("Service stopped successfully: {}", SERVICE_NAME);
        Ok(())
    }

    pub fn restart_service(&self) -> Result<()> {
        self.stop_service()?;
        std::thread::sleep(Duration::from_secs(2));
        self.start_service()?;
        Ok(())
    }
}
