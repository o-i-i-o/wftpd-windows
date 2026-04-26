//! Server Manager for Windows Service management
//!
//! This module provides Windows service management functionality
//! for the wftpd service backend.

use anyhow::{Context, Result};
use windows::Win32::System::Services::*;
use windows::core::PCWSTR;

const SERVICE_NAME: &str = "wftpd";
const SERVICE_WAIT_MAX_ATTEMPTS: u32 = 30;
const SERVICE_WAIT_INTERVAL_MS: u64 = 500;

fn close_service_handle(handle: impl windows::Win32::Foundation::IntoParam<SC_HANDLE>) {
    if let Err(e) = CloseServiceHandle(handle) {
        tracing::debug!("CloseServiceHandle error: {:?}", e);
    }
}

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

    pub fn is_service_installed(&self) -> bool {
        unsafe {
            let manager_result = OpenSCManagerW(None, None, SC_MANAGER_CONNECT);

            if let Ok(manager) = manager_result {
                let service_name_wide: Vec<u16> = SERVICE_NAME
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let service_result = OpenServiceW(
                    manager,
                    PCWSTR(service_name_wide.as_ptr()),
                    SERVICE_QUERY_STATUS,
                );

                close_service_handle(manager);

                if let Ok(service) = service_result {
                    close_service_handle(SC_HANDLE(service.0));
                    return true;
                }
            }
        }
        false
    }

    pub fn is_service_running(&self) -> bool {
        unsafe {
            let manager = match OpenSCManagerW(None, None, SC_MANAGER_CONNECT) {
                Ok(m) => m,
                Err(_) => return false,
            };

            let service_name_wide: Vec<u16> = SERVICE_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let service = match OpenServiceW(
                manager,
                PCWSTR(service_name_wide.as_ptr()),
                SERVICE_QUERY_STATUS | SERVICE_START,
            ) {
                Ok(s) => s,
                Err(_) => {
                    close_service_handle(manager);
                    return false;
                }
            };

            close_service_handle(manager);

            let mut status = SERVICE_STATUS_PROCESS::default();
            let mut bytes_needed: u32 = 0;

            let query_result = QueryServiceStatusEx(
                service,
                SC_STATUS_PROCESS_INFO,
                Some(std::slice::from_raw_parts_mut(
                    &mut status as *mut _ as *mut u8,
                    std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
                )),
                &mut bytes_needed,
            );

            close_service_handle(service);

            if query_result.is_ok() {
                return status.dwCurrentState == SERVICE_RUNNING;
            }
        }
        false
    }

    pub fn install_service(&self) -> Result<()> {
        unsafe {
            let exe_path =
                std::env::current_exe().context("Failed to get current executable path")?;

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

            let exe_path_str = service_exe.to_string_lossy().to_string();
            let exe_path_wide: Vec<u16> = exe_path_str
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let service_name_wide: Vec<u16> = SERVICE_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let display_name_wide: Vec<u16> = "WFTPD Service"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let manager =
                OpenSCManagerW(None, None, SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)
                    .context("Failed to open service control manager")?;

            let service = CreateServiceW(
                manager,
                PCWSTR(service_name_wide.as_ptr()),
                PCWSTR(display_name_wide.as_ptr()),
                SERVICE_CHANGE_CONFIG | SERVICE_START,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_AUTO_START,
                SERVICE_ERROR_NORMAL,
                PCWSTR(exe_path_wide.as_ptr()),
                None,
                None,
                None,
                None,
                None,
            )
            .context("Failed to create service")?;

            close_service_handle(service);
            close_service_handle(manager);

            tracing::info!("Service installed successfully: {}", SERVICE_NAME);
            Ok(())
        }
    }

    pub fn uninstall_service(&self) -> Result<()> {
        unsafe {
            let manager = OpenSCManagerW(None, None, SC_MANAGER_CONNECT)
                .context("Failed to open service control manager")?;

            let service_name_wide: Vec<u16> = SERVICE_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let service = OpenServiceW(
                manager,
                PCWSTR(service_name_wide.as_ptr()),
                SERVICE_ALL_ACCESS,
            )
            .context("Failed to open service")?;

            DeleteService(service).context("Failed to delete service")?;

            close_service_handle(service);
            close_service_handle(manager);

            tracing::info!("Service uninstalled successfully: {}", SERVICE_NAME);
            Ok(())
        }
    }

    pub fn start_service(&self) -> Result<()> {
        unsafe {
            let manager = OpenSCManagerW(None, None, SC_MANAGER_CONNECT)
                .context("Failed to open service control manager")?;

            let service_name_wide: Vec<u16> = SERVICE_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let service = OpenServiceW(
                manager,
                PCWSTR(service_name_wide.as_ptr()),
                SERVICE_START | SERVICE_QUERY_STATUS,
            )
            .context("Failed to open service")?;

            close_service_handle(manager);

            StartServiceW(service, None).context("Failed to start service")?;

            let mut status = SERVICE_STATUS::default();
            for _ in 0..SERVICE_WAIT_MAX_ATTEMPTS {
                if QueryServiceStatus(service, &mut status).is_ok() {
                    if status.dwCurrentState == SERVICE_RUNNING {
                        break;
                    }
                    if status.dwCurrentState == SERVICE_STOPPED {
                        close_service_handle(service);
                        anyhow::bail!(
                            "Service stopped immediately after start, check service configuration"
                        );
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(SERVICE_WAIT_INTERVAL_MS));
            }

            close_service_handle(service);

            tracing::info!("Service started successfully: {}", SERVICE_NAME);
            Ok(())
        }
    }

    pub fn stop_service(&self) -> Result<()> {
        unsafe {
            let manager = OpenSCManagerW(None, None, SC_MANAGER_CONNECT)
                .context("Failed to open service control manager")?;

            let service_name_wide: Vec<u16> = SERVICE_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let service = OpenServiceW(
                manager,
                PCWSTR(service_name_wide.as_ptr()),
                SERVICE_STOP | SERVICE_QUERY_STATUS,
            )
            .context("Failed to open service")?;

            close_service_handle(manager);

            let mut status = SERVICE_STATUS::default();
            ControlService(service, SERVICE_CONTROL_STOP, &mut status)
                .context("Failed to stop service")?;

            for _ in 0..SERVICE_WAIT_MAX_ATTEMPTS {
                if QueryServiceStatus(service, &mut status).is_ok()
                    && status.dwCurrentState == SERVICE_STOPPED
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(SERVICE_WAIT_INTERVAL_MS));
            }

            close_service_handle(service);

            tracing::info!("Service stopped successfully: {}", SERVICE_NAME);
            Ok(())
        }
    }

    pub fn restart_service(&self) -> Result<()> {
        self.stop_service()?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        self.start_service()?;
        Ok(())
    }
}
