use std::sync::{Arc, Mutex};
use std::ffi::OsString;

use crate::core::config::Config;
use crate::core::users::UserManager;
use crate::core::logger::Logger;
use crate::core::file_logger::FileLogger;
use crate::core::ftp_server::FtpServer;
use crate::core::sftp_server::SftpServer;

pub struct ServerManager {
    ftp_server: Arc<Mutex<Option<FtpServer>>>,
    sftp_server: Arc<Mutex<Option<SftpServer>>>,
    sftp_runtime: Arc<Mutex<Option<tokio::runtime::Runtime>>>,
}

impl ServerManager {
    pub fn new() -> Self {
        ServerManager {
            ftp_server: Arc::new(Mutex::new(None)),
            sftp_server: Arc::new(Mutex::new(None)),
            sftp_runtime: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start_ftp(
        &self,
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
        logger: Arc<Mutex<Logger>>,
        file_logger: Arc<Mutex<FileLogger>>,
    ) -> anyhow::Result<()> {
        let mut ftp_server = match self.ftp_server.lock() {
            Ok(guard) => guard,
            Err(e) => return Err(anyhow::anyhow!("获取FTP服务器锁失败: {}", e)),
        };
        if ftp_server.is_some() {
            return Ok(());
        }

        let server = FtpServer::new(config, user_manager, logger, file_logger);
        server.start()?;
        *ftp_server = Some(server);
        Ok(())
    }

    pub fn stop_ftp(&self, logger: &Arc<Mutex<Logger>>) {
        let mut ftp_server = match self.ftp_server.lock() {
            Ok(guard) => guard,
            Err(e) => {
                log::error!("获取FTP服务器锁失败: {}", e);
                return;
            }
        };
        if let Some(server) = ftp_server.take() {
            server.stop();
            if let Ok(mut log) = logger.lock() {
                log.info("FTP", "FTP server stopped");
            }
        }
    }

    pub fn is_ftp_running(&self) -> bool {
        let ftp_server = match self.ftp_server.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        ftp_server.as_ref().is_some_and(|s| s.is_running())
    }

    pub fn start_sftp(
        &self,
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
        logger: Arc<Mutex<Logger>>,
        file_logger: Arc<Mutex<FileLogger>>,
    ) -> anyhow::Result<()> {
        {
            let sftp_server = match self.sftp_server.lock() {
                Ok(guard) => guard,
                Err(e) => return Err(anyhow::anyhow!("获取SFTP服务器锁失败: {}", e)),
            };
            if sftp_server.is_some() {
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
            Arc::clone(&logger),
            file_logger,
        );

        runtime.block_on(server.start())?;

        {
            let mut rt = match self.sftp_runtime.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    runtime.shutdown_background();
                    return Err(anyhow::anyhow!("获取SFTP运行时锁失败: {}", e));
                }
            };
            *rt = Some(runtime);
        }
        {
            let mut srv = match self.sftp_server.lock() {
                Ok(guard) => guard,
                Err(e) => return Err(anyhow::anyhow!("获取SFTP服务器锁失败: {}", e)),
            };
            *srv = Some(server);
        }

        if let Ok(mut log) = logger.lock() {
            log.info("SFTP", "SFTP server started successfully");
        }

        Ok(())
    }

    /// Stop the SFTP server.
    /// We first signal shutdown via the server's stop() method (which sends the
    /// oneshot), then drop the runtime so all spawned tasks are cancelled cleanly.
    /// Taking both the server and the runtime inside the same locked scope avoids
    /// the TOCTOU race that existed before.
    pub fn stop_sftp(&self, logger: &Arc<Mutex<Logger>>) {
        // Take both atomically to avoid racing between server.stop() and
        // runtime.shutdown_background().
        let (maybe_server, maybe_runtime) = {
            let mut srv = match self.sftp_server.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    log::error!("获取SFTP服务器锁失败: {}", e);
                    return;
                }
            };
            let mut rt = match self.sftp_runtime.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    log::error!("获取SFTP运行时锁失败: {}", e);
                    return;
                }
            };
            (srv.take(), rt.take())
        };

        if let (Some(server), Some(runtime)) = (maybe_server, maybe_runtime) {
            // Send the shutdown signal and wait for it inside the runtime.
            runtime.block_on(server.stop());
            // Now shut down the runtime; all remaining tasks are dropped.
            runtime.shutdown_background();
        }

        if let Ok(mut log) = logger.lock() {
            log.info("SFTP", "SFTP server stopped");
        }
    }

    pub fn is_sftp_running(&self) -> bool {
        let sftp_server = match self.sftp_server.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        sftp_server.as_ref().is_some_and(|s| s.is_running())
    }

    // ----------------------------------------------------------------
    // Windows Service management helpers (used by the GUI service tab)
    // ----------------------------------------------------------------

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

        // 查找同目录下的 wftpd.exe，直接使用当前路径
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
            log::warn!("设置服务描述失败（可忽略）: {:?}", e);
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
        
        // 检查服务状态，如果正在运行则先停止
        match service.query_status() {
            Ok(status) => {
                if status.current_state != ServiceState::Stopped {
                    log::info!("服务正在运行，尝试停止...");
                    match service.stop() {
                        Ok(_) => {
                            // 等待服务停止
                            let mut attempts = 0;
                            loop {
                                std::thread::sleep(std::time::Duration::from_millis(500));
                                match service.query_status() {
                                    Ok(s) => {
                                        if s.current_state == ServiceState::Stopped {
                                            break;
                                        }
                                    }
                                    Err(_) => break,
                                }
                                attempts += 1;
                                if attempts > 20 {
                                    return Err(anyhow::anyhow!("等待服务停止超时"));
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("停止服务失败（可能服务已停止）: {:?}", e);
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("查询服务状态失败: {:?}", e);
            }
        }
        
        service.delete()
            .map_err(|e| anyhow::anyhow!("删除服务失败: {:?}", e))?;
        
        Ok(())
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
        
        // 检查服务是否已经在运行
        match service.query_status() {
            Ok(status) => {
                if status.current_state == ServiceState::Running {
                    return Ok(()); // 服务已经在运行
                }
                if status.current_state == ServiceState::StartPending {
                    // 等待服务启动完成
                    let mut attempts = 0;
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        match service.query_status() {
                            Ok(s) => {
                                if s.current_state == ServiceState::Running {
                                    return Ok(());
                                }
                                if s.current_state != ServiceState::StartPending {
                                    return Err(anyhow::anyhow!("服务启动失败，当前状态: {:?}", s.current_state));
                                }
                            }
                            Err(e) => return Err(anyhow::anyhow!("查询服务状态失败: {:?}", e)),
                        }
                        attempts += 1;
                        if attempts > 60 {
                            return Err(anyhow::anyhow!("等待服务启动超时"));
                        }
                    }
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!("查询服务状态失败: {:?}", e));
            }
        }
        
        // 启动服务
        service.start(&[] as &[&std::ffi::OsStr])
            .map_err(|e| anyhow::anyhow!("启动服务失败: {:?}", e))?;
        
        // 等待服务启动完成
        let mut attempts = 0;
        loop {
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
            attempts += 1;
            if attempts > 60 {
                return Err(anyhow::anyhow!("等待服务启动超时"));
            }
        }
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
        
        // 检查服务状态
        match service.query_status() {
            Ok(status) => {
                if status.current_state == ServiceState::Stopped {
                    return Ok(()); // 服务已经停止
                }
                if status.current_state == ServiceState::StopPending {
                    // 等待服务停止完成
                    let mut attempts = 0;
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        match service.query_status() {
                            Ok(s) => {
                                if s.current_state == ServiceState::Stopped {
                                    return Ok(());
                                }
                            }
                            Err(_) => return Ok(()),
                        }
                        attempts += 1;
                        if attempts > 60 {
                            return Err(anyhow::anyhow!("等待服务停止超时"));
                        }
                    }
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!("查询服务状态失败: {:?}", e));
            }
        }
        
        // 发送停止命令
        service.stop()
            .map_err(|e| anyhow::anyhow!("停止服务失败: {:?}", e))?;
        
        // 等待服务停止
        let mut attempts = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            match service.query_status() {
                Ok(s) => {
                    if s.current_state == ServiceState::Stopped {
                        return Ok(());
                    }
                }
                Err(_) => return Ok(()),
            }
            attempts += 1;
            if attempts > 60 {
                return Err(anyhow::anyhow!("等待服务停止超时"));
            }
        }
    }
}

impl Default for ServerManager {
    fn default() -> Self {
        Self::new()
    }
}
