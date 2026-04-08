//! Server Manager for Windows Service management
//!
//! This module provides Windows service management functionality
//! for the wftpd service backend.

use anyhow::{Context, Result};
use windows::Win32::System::Services::*;
use windows::core::PCWSTR;

const SERVICE_NAME: &str = "wftpd";
/// 服务启动/停止最大等待次数
const SERVICE_WAIT_MAX_ATTEMPTS: u32 = 30;
/// 服务启动/停止每次等待间隔（毫秒）
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

    /// 检查服务是否已安装
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

                let _ = CloseServiceHandle(manager);

                if let Ok(_service) = service_result {
                    let _ = CloseServiceHandle(SC_HANDLE(service_result.unwrap().0));
                    return true;
                }
            }
        }
        false
    }

    /// 检查服务是否正在运行
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
                    let _ = CloseServiceHandle(manager);
                    return false;
                }
            };

            let _ = CloseServiceHandle(manager);

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

            let _ = CloseServiceHandle(service);

            if query_result.is_ok() {
                return status.dwCurrentState == SERVICE_RUNNING;
            }
        }
        false
    }

    /// 安装服务
    pub fn install_service(&self) -> Result<()> {
        unsafe {
            let exe_path = std::env::current_exe().context("无法获取当前程序路径")?;

            let exe_dir = exe_path.parent().context("无法获取程序目录")?;

            let service_exe = exe_dir.join("wftpd.exe");

            if !service_exe.exists() {
                anyhow::bail!("找不到后端服务程序: {}", service_exe.display());
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
                    .context("无法打开服务控制管理器")?;

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
            .context("无法创建服务")?;

            let _ = CloseServiceHandle(service);
            let _ = CloseServiceHandle(manager);

            tracing::info!("服务安装成功：{}", SERVICE_NAME);
            Ok(())
        }
    }

    /// 卸载服务
    pub fn uninstall_service(&self) -> Result<()> {
        unsafe {
            let manager =
                OpenSCManagerW(None, None, SC_MANAGER_CONNECT).context("无法打开服务控制管理器")?;

            let service_name_wide: Vec<u16> = SERVICE_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let service = OpenServiceW(
                manager,
                PCWSTR(service_name_wide.as_ptr()),
                SERVICE_ALL_ACCESS,
            )
            .context("无法打开服务")?;

            DeleteService(service).context("无法删除服务")?;

            let _ = CloseServiceHandle(service);
            let _ = CloseServiceHandle(manager);

            tracing::info!("服务卸载成功：{}", SERVICE_NAME);
            Ok(())
        }
    }

    /// 启动服务
    pub fn start_service(&self) -> Result<()> {
        unsafe {
            let manager =
                OpenSCManagerW(None, None, SC_MANAGER_CONNECT).context("无法打开服务控制管理器")?;

            let service_name_wide: Vec<u16> = SERVICE_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let service = OpenServiceW(
                manager,
                PCWSTR(service_name_wide.as_ptr()),
                SERVICE_START | SERVICE_QUERY_STATUS,
            )
            .context("无法打开服务")?;

            let _ = CloseServiceHandle(manager);

            StartServiceW(service, None).context("无法启动服务")?;

            let mut status = SERVICE_STATUS::default();
            for _ in 0..SERVICE_WAIT_MAX_ATTEMPTS {
                if QueryServiceStatus(service, &mut status).is_ok() {
                    if status.dwCurrentState == SERVICE_RUNNING {
                        break;
                    }
                    if status.dwCurrentState == SERVICE_STOPPED {
                        let _ = CloseServiceHandle(service);
                        anyhow::bail!("服务启动后立即停止，请检查服务配置");
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(SERVICE_WAIT_INTERVAL_MS));
            }

            let _ = CloseServiceHandle(service);

            tracing::info!("服务启动成功：{}", SERVICE_NAME);
            Ok(())
        }
    }

    /// 停止服务
    pub fn stop_service(&self) -> Result<()> {
        unsafe {
            let manager =
                OpenSCManagerW(None, None, SC_MANAGER_CONNECT).context("无法打开服务控制管理器")?;

            let service_name_wide: Vec<u16> = SERVICE_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let service = OpenServiceW(
                manager,
                PCWSTR(service_name_wide.as_ptr()),
                SERVICE_STOP | SERVICE_QUERY_STATUS,
            )
            .context("无法打开服务")?;

            let _ = CloseServiceHandle(manager);

            let mut status = SERVICE_STATUS::default();
            ControlService(service, SERVICE_CONTROL_STOP, &mut status).context("无法停止服务")?;

            for _ in 0..SERVICE_WAIT_MAX_ATTEMPTS {
                if QueryServiceStatus(service, &mut status).is_ok()
                    && status.dwCurrentState == SERVICE_STOPPED
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(SERVICE_WAIT_INTERVAL_MS));
            }

            let _ = CloseServiceHandle(service);

            tracing::info!("服务停止成功：{}", SERVICE_NAME);
            Ok(())
        }
    }

    /// 重启服务
    pub fn restart_service(&self) -> Result<()> {
        self.stop_service()?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        self.start_service()?;
        Ok(())
    }
}
