use parking_lot::Mutex;
use std::sync::Arc;
use std::ffi::OsString;
use std::sync::mpsc;

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
        }
    }

    pub fn start_ftp(
        &self,
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> anyhow::Result<()> {
        {
            let state = self.ftp_state.lock();
            if state.server.is_some() {
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

        tracing::info!("FTP server started successfully");

        Ok(())
    }

    pub fn start_ftp_async(
        &self,
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> mpsc::Receiver<Result<(), String>> {
        let (tx, rx) = mpsc::channel();
        let ftp_state = Arc::clone(&self.ftp_state);

        std::thread::spawn(move || {
            {
                let state = ftp_state.lock();
                if state.server.is_some() {
                    let _ = tx.send(Ok(()));
                    return;
                }
            }

            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(format!("创建Tokio运行时失败: {}", e)));
                    return;
                }
            };

            let server = FtpServer::new(
                config,
                user_manager,
            );

            if let Err(e) = runtime.block_on(server.start()) {
                runtime.shutdown_background();
                let _ = tx.send(Err(format!("启动FTP服务器失败: {}", e)));
                return;
            }

            {
                let mut state = ftp_state.lock();
                state.server = Some(server);
                state.runtime = Some(runtime);
            }

            tracing::info!("FTP server started successfully");

            let _ = tx.send(Ok(()));
        });

        rx
    }

    pub fn stop_ftp(&self) {
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

    pub fn stop_ftp_async(&self) -> mpsc::Receiver<Result<(), String>> {
        let (tx, rx) = mpsc::channel();
        let ftp_state = Arc::clone(&self.ftp_state);

        std::thread::spawn(move || {
            let (maybe_server, maybe_runtime) = {
                let mut state = ftp_state.lock();
                (state.server.take(), state.runtime.take())
            };

            if let (Some(server), Some(runtime)) = (maybe_server, maybe_runtime) {
                runtime.block_on(server.stop());
                runtime.shutdown_background();
            }

            tracing::info!("FTP server stopped");

            let _ = tx.send(Ok(()));
        });

        rx
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
        {
            let state = self.sftp_state.lock();
            if state.server.is_some() {
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

        tracing::info!("SFTP server started successfully");

        Ok(())
    }

    pub fn start_sftp_async(
        &self,
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> mpsc::Receiver<Result<(), String>> {
        let (tx, rx) = mpsc::channel();
        let sftp_state = Arc::clone(&self.sftp_state);

        std::thread::spawn(move || {
            {
                let state = sftp_state.lock();
                if state.server.is_some() {
                    let _ = tx.send(Ok(()));
                    return;
                }
            }

            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(format!("创建Tokio运行时失败: {}", e)));
                    return;
                }
            };

            let server = SftpServer::new(
                config,
                user_manager,
            );

            if let Err(e) = runtime.block_on(server.start()) {
                runtime.shutdown_background();
                let _ = tx.send(Err(format!("启动SFTP服务器失败: {}", e)));
                return;
            }

            {
                let mut state = sftp_state.lock();
                state.server = Some(server);
                state.runtime = Some(runtime);
            }

            tracing::info!("SFTP server started successfully");

            let _ = tx.send(Ok(()));
        });

        rx
    }

    pub fn stop_sftp(&self) {
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

    pub fn stop_sftp_async(&self) -> mpsc::Receiver<Result<(), String>> {
        let (tx, rx) = mpsc::channel();
        let sftp_state = Arc::clone(&self.sftp_state);

        std::thread::spawn(move || {
            let (maybe_server, maybe_runtime) = {
                let mut state = sftp_state.lock();
                (state.server.take(), state.runtime.take())
            };

            if let (Some(server), Some(runtime)) = (maybe_server, maybe_runtime) {
                runtime.block_on(server.stop());
                runtime.shutdown_background();
            }

            tracing::info!("SFTP server stopped");

            let _ = tx.send(Ok(()));
        });

        rx
    }

    pub fn is_sftp_running(&self) -> bool {
        let state = self.sftp_state.lock();
        state.server.as_ref().is_some_and(|s| s.is_running())
    }

    pub fn is_service_installed(&self) -> bool {
        use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
        use windows_service::service::ServiceAccess;
        let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        else {
            return false;
        };
        manager
            .open_service("wftpd", ServiceAccess::QUERY_STATUS)
            .is_ok()
    }

    pub fn is_service_running(&self) -> bool {
        use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
        use windows_service::service::{ServiceAccess, ServiceState};
        let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        else {
            return false;
        };
        let Ok(service) = manager.open_service("wftpd", ServiceAccess::QUERY_STATUS) else {
            return false;
        };
        match service.query_status() {
            Ok(status) => status.current_state == ServiceState::Running,
            Err(_) => false,
        }
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

        Ok(())
    }

    pub fn uninstall_service(&self) -> anyhow::Result<()> {
        use windows_service::{
            service::{ServiceAccess, ServiceState},
            service_manager::{ServiceManager, ServiceManagerAccess},
        };

        let manager = ServiceManager::local_computer(
            None::<&str>, 
            ServiceManagerAccess::CONNECT
        ).map_err(|e| anyhow::anyhow!("连接服务管理器失败: {:?}", e))?;
        
        let service = manager.open_service(
            "wftpd", 
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE
        ).map_err(|e| anyhow::anyhow!("打开服务失败: {:?}", e))?;
        
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
        use windows_service::{
            service::{ServiceAccess, ServiceState},
            service_manager::{ServiceManager, ServiceManagerAccess},
        };

        let manager = ServiceManager::local_computer(
            None::<&str>, 
            ServiceManagerAccess::CONNECT
        ).map_err(|e| anyhow::anyhow!("连接服务管理器失败: {:?}", e))?;
        
        let service = manager.open_service(
            "wftpd", 
            ServiceAccess::QUERY_STATUS | ServiceAccess::START
        ).map_err(|e| anyhow::anyhow!("打开服务失败: {:?}", e))?;
        
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
        use windows_service::{
            service::{ServiceAccess, ServiceState},
            service_manager::{ServiceManager, ServiceManagerAccess},
        };

        let manager = ServiceManager::local_computer(
            None::<&str>, 
            ServiceManagerAccess::CONNECT
        ).map_err(|e| anyhow::anyhow!("连接服务管理器失败: {:?}", e))?;
        
        let service = manager.open_service(
            "wftpd", 
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP
        ).map_err(|e| anyhow::anyhow!("打开服务失败: {:?}", e))?;
        
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
    }

    pub fn restart_service(&self) -> anyhow::Result<()> {
        use windows_service::{
            service::{ServiceAccess, ServiceState},
            service_manager::{ServiceManager, ServiceManagerAccess},
        };

        let manager = ServiceManager::local_computer(
            None::<&str>, 
            ServiceManagerAccess::CONNECT
        ).map_err(|e| anyhow::anyhow!("连接服务管理器失败: {:?}", e))?;
        
        let service = manager.open_service(
            "wftpd", 
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::START
        ).map_err(|e| anyhow::anyhow!("打开服务失败: {:?}", e))?;
        
        // 检查当前状态
        match service.query_status() {
            Ok(status) => {
                if status.current_state == ServiceState::Running {
                    // 服务正在运行，先停止
                    service.stop()
                        .map_err(|e| anyhow::anyhow!("停止服务失败: {:?}", e))?;
                    // 等待服务停止
                    Self::wait_service_stopping(&service)?;
                } else if status.current_state == ServiceState::StopPending {
                    // 服务正在停止，等待完成
                    Self::wait_service_stopping(&service)?;
                }
                // 服务已停止，现在启动
            }
            Err(e) => {
                return Err(anyhow::anyhow!("查询服务状态失败: {:?}", e));
            }
        }
        
        // 启动服务
        service.start(&[] as &[&std::ffi::OsStr])
            .map_err(|e| anyhow::anyhow!("启动服务失败: {:?}", e))?;
        
        Self::wait_service_starting(&service)
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
