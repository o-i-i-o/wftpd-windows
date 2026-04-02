use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::ffi::OsString;
use windows_service::service::ServiceAccess;

use crate::core::config::Config;
use crate::core::users::UserManager;
use crate::core::ftp_server::FtpServer;
use crate::core::sftp_server::SftpServer;

struct FtpState {
    server: Option<FtpServer>,
    runtime: Option<tokio::runtime::Runtime>,
}

struct SftpState {
    server: Option<SftpServer>,
    runtime: Option<tokio::runtime::Runtime>,
}

pub struct ServerManager {
    ftp_state: Arc<Mutex<FtpState>>,
    sftp_state: Arc<Mutex<SftpState>>,
    ftp_starting: Arc<AtomicBool>,
    sftp_starting: Arc<AtomicBool>,
}

impl ServerManager {
    pub fn new() -> Self {
        ServerManager {
            ftp_state: Arc::new(Mutex::new(FtpState {
                server: None,
                runtime: None,
            })),
            sftp_state: Arc::new(Mutex::new(SftpState {
                server: None,
                runtime: None,
            })),
            ftp_starting: Arc::new(AtomicBool::new(false)),
            sftp_starting: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start_ftp(
        &self,
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> anyhow::Result<()> {
        // 使用 CAS 操作防止并发启动竞态
        if self.ftp_starting.compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed).is_err() {
            tracing::debug!("FTP server is already starting or running");
            return Ok(());
        }

        {
            let state = self.ftp_state.lock();
            if state.server.is_some() {
                self.ftp_starting.store(false, Ordering::SeqCst);
                return Ok(());
            }
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;

        let server = FtpServer::new(
            config,
            user_manager,
        );

        runtime.block_on(server.start())?;

        {
            let mut state = self.ftp_state.lock();
            state.server = Some(server);
            state.runtime = Some(runtime);
        }

        Ok(())
    }

    pub fn stop_ftp(&self) {
        self.ftp_starting.store(false, Ordering::SeqCst);

        let (maybe_server, maybe_runtime) = {
            let mut state = self.ftp_state.lock();
            (state.server.take(), state.runtime.take())
        };

        if let (Some(server), Some(runtime)) = (maybe_server, maybe_runtime) {
            runtime.block_on(server.stop());
            runtime.shutdown_background();
        }

        tracing::info!("FTP server stopped");
    }

    pub fn is_ftp_running(&self) -> bool {
        let state = self.ftp_state.lock();
        state.server.as_ref().is_some_and(|s| s.is_running())
    }

    pub fn start_sftp(
        &self,
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> anyhow::Result<()> {
        // 使用 CAS 操作防止并发启动竞态
        if self.sftp_starting.compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed).is_err() {
            tracing::debug!("SFTP server is already starting or running");
            return Ok(());
        }

        {
            let state = self.sftp_state.lock();
            if state.server.is_some() {
                self.sftp_starting.store(false, Ordering::SeqCst);
                return Ok(());
            }
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;

        let server = SftpServer::new(
            config,
            user_manager,
        );

        runtime.block_on(server.start())?;

        {
            let mut state = self.sftp_state.lock();
            state.server = Some(server);
            state.runtime = Some(runtime);
        }

        Ok(())
    }

    pub fn stop_sftp(&self) {
        self.sftp_starting.store(false, Ordering::SeqCst);

        let (maybe_server, maybe_runtime) = {
            let mut state = self.sftp_state.lock();
            (state.server.take(), state.runtime.take())
        };

        if let (Some(server), Some(runtime)) = (maybe_server, maybe_runtime) {
            runtime.block_on(server.stop());
            runtime.shutdown_background();
        }

        tracing::info!("SFTP server stopped");
    }

    pub fn is_sftp_running(&self) -> bool {
        let state = self.sftp_state.lock();
        state.server.as_ref().is_some_and(|s| s.is_running())
    }

    pub fn is_service_installed(&self) -> bool {
        Self::with_service(ServiceAccess::QUERY_STATUS, |s| Ok(s.query_status().is_ok())).unwrap_or(false)
    }

    pub fn is_service_running(&self) -> bool {
        use windows_service::service::ServiceState;
        Self::with_service(ServiceAccess::QUERY_STATUS, |service| {
            Ok(service.query_status()
                .map(|s| s.current_state == ServiceState::Running)
                .unwrap_or(false))
        }).unwrap_or(false)
    }

    /// 公共辅助函数：获取 ServiceManager 并打开 wftpd 服务
    fn with_service<F, T>(access: ServiceAccess, f: F) -> anyhow::Result<T>
    where
        F: FnOnce(windows_service::service::Service) -> anyhow::Result<T>,
    {
        use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(|e| anyhow::anyhow!("连接服务管理器失败: {:?}", e))?;
        let service = manager.open_service("wftpd", access)
            .map_err(|e| anyhow::anyhow!("打开服务失败: {:?}", e))?;
        f(service)
    }

    pub fn install_service(&self) -> anyhow::Result<()> {
        use windows_service::{
            service::{
                ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType,
            },
            service_manager::{ServiceManager, ServiceManagerAccess},
        };

        let current_exe = std::env::current_exe()?;
        let exe_dir = current_exe
            .parent()
            .ok_or_else(|| anyhow::anyhow!("无法获取当前程序目录"))?;

        let wftpd_exe = exe_dir.join("wftpd.exe");
        if !wftpd_exe.exists() {
            return Err(anyhow::anyhow!(
                "在当前目录未找到 wftpd.exe，请确保 wftpd.exe 与 wftp-gui.exe 在同一目录"
            ));
        }

        let manager = ServiceManager::local_computer(
            None::<&str>,
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )
        .map_err(|e| anyhow::anyhow!("连接服务管理器失败: {:?}", e))?;

        let info = ServiceInfo {
            name: OsString::from("wftpd"),
            display_name: OsString::from("WFTPD SFTP/FTP Server"),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: wftpd_exe,
            launch_arguments: vec![],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };

        let service = manager
            .create_service(&info, ServiceAccess::CHANGE_CONFIG)
            .map_err(|e| anyhow::anyhow!("创建服务失败: {:?}", e))?;

        if let Err(e) = service.set_description("SFTP and FTP server daemon with GUI management") {
            tracing::warn!("设置服务描述失败（可忽略）: {:?}", e);
        }

        tracing::info!("服务安装成功");
        Ok(())
    }

    pub fn uninstall_service(&self) -> anyhow::Result<()> {
        use windows_service::service::{ServiceAccess, ServiceState};

        Self::with_service(ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE, |service| {
            match service.query_status() {
                Ok(status) => {
                    if status.current_state != ServiceState::Stopped {
                        tracing::info!("服务正在运行，尝试停止...");
                        if let Err(e) = Self::stop_service_internal(&service) {
                            tracing::warn!("停止服务失败（可能服务已停止）: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("查询服务状态失败: {:?}", e);
                }
            }

            service.delete()
                .map_err(|e| anyhow::anyhow!("删除服务失败: {:?}", e))?;

            Ok(())
        })
    }

    fn stop_service_internal(service: &windows_service::service::Service) -> anyhow::Result<()> {
        use windows_service::service::ServiceState;
        
        service.stop()
            .map_err(|e| anyhow::anyhow!("停止服务失败: {:?}", e))?;
        
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            match service.query_status() {
                Ok(s) => {
                    if s.current_state == ServiceState::Stopped {
                        return Ok(());
                    }
                }
                Err(_) => return Ok(()),
            }
        }
        Err(anyhow::anyhow!("等待服务停止超时"))
    }

    pub fn start_service(&self) -> anyhow::Result<()> {
        use windows_service::service::{ServiceAccess, ServiceState};

        Self::with_service(ServiceAccess::QUERY_STATUS | ServiceAccess::START, |service| {
            match service.query_status() {
                Ok(status) => {
                    if status.current_state == ServiceState::Running {
                        return Ok(());
                    }
                    if status.current_state == ServiceState::StartPending {
                        return Self::wait_service_starting(&service);
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("查询服务状态失败: {:?}", e));
                }
            }

            service.start(&[] as &[&std::ffi::OsStr])
                .map_err(|e| anyhow::anyhow!("启动服务失败: {:?}", e))?;

            Self::wait_service_starting(&service)
        })
    }

    fn wait_service_starting(service: &windows_service::service::Service) -> anyhow::Result<()> {
        use windows_service::service::ServiceState;
        
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            match service.query_status() {
                Ok(s) => {
                    if s.current_state == ServiceState::Running {
                        return Ok(());
                    }
                    if s.current_state == ServiceState::Stopped {
                        return Err(anyhow::anyhow!("服务启动后立即停止，请检查服务配置"));
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("查询服务状态失败: {:?}", e)),
            }
        }
        Err(anyhow::anyhow!("等待服务启动超时"))
    }

    pub fn stop_service(&self) -> anyhow::Result<()> {
        use windows_service::service::{ServiceAccess, ServiceState};

        Self::with_service(ServiceAccess::QUERY_STATUS | ServiceAccess::STOP, |service| {
            match service.query_status() {
                Ok(status) => {
                    if status.current_state == ServiceState::Stopped {
                        return Ok(());
                    }
                    if status.current_state == ServiceState::StopPending {
                        return Self::wait_service_stopping(&service);
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("查询服务状态失败: {:?}", e));
                }
            }

            service.stop()
                .map_err(|e| anyhow::anyhow!("停止服务失败: {:?}", e))?;

            Self::wait_service_stopping(&service)
        })
    }

    pub fn restart_service(&self) -> anyhow::Result<()> {
        use windows_service::service::{ServiceAccess, ServiceState};

        Self::with_service(ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::START, |service| {
            match service.query_status() {
                Ok(status) => {
                    if status.current_state == ServiceState::Running {
                        service.stop()
                            .map_err(|e| anyhow::anyhow!("停止服务失败: {:?}", e))?;
                        Self::wait_service_stopping(&service)?;
                    } else if status.current_state == ServiceState::StopPending {
                        Self::wait_service_stopping(&service)?;
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("查询服务状态失败: {:?}", e));
                }
            }

            service.start(&[] as &[&std::ffi::OsStr])
                .map_err(|e| anyhow::anyhow!("启动服务失败: {:?}", e))?;

            Self::wait_service_starting(&service)
        })
    }

    fn wait_service_stopping(service: &windows_service::service::Service) -> anyhow::Result<()> {
        use windows_service::service::ServiceState;
        
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            match service.query_status() {
                Ok(s) => {
                    if s.current_state == ServiceState::Stopped {
                        return Ok(());
                    }
                }
                Err(_) => return Ok(()),
            }
        }
        Err(anyhow::anyhow!("等待服务停止超时"))
    }
}

impl Default for ServerManager {
    fn default() -> Self {
        Self::new()
    }
}
