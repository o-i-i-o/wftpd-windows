mod session;
mod commands;
mod passive;
mod transfer;
mod tls;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::core::config::Config;
use crate::core::logger::Logger;
use crate::core::users::UserManager;
use crate::core::file_logger::FileLogger;

pub struct FtpServer {
    config: Arc<std::sync::Mutex<Config>>,
    user_manager: Arc<std::sync::Mutex<UserManager>>,
    logger: Arc<std::sync::Mutex<Logger>>,
    file_logger: Arc<std::sync::Mutex<FileLogger>>,
    running: Arc<std::sync::Mutex<bool>>,
    shutdown_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl FtpServer {
    pub fn new(
        config: Arc<std::sync::Mutex<Config>>,
        user_manager: Arc<std::sync::Mutex<UserManager>>,
        logger: Arc<std::sync::Mutex<Logger>>,
        file_logger: Arc<std::sync::Mutex<FileLogger>>,
    ) -> Self {
        FtpServer {
            config,
            user_manager,
            logger,
            file_logger,
            running: Arc::new(std::sync::Mutex::new(false)),
            shutdown_tx: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let (bind_ip, ftp_port, warnings) = {
            let cfg = match self.config.lock() {
                Ok(guard) => guard,
                Err(e) => return Err(anyhow::anyhow!("获取配置锁失败: {}", e)),
            };
            let warnings = cfg.validate_paths();
            (cfg.ftp.bind_ip.clone(), cfg.server.ftp_port, warnings)
        };

        if !warnings.is_empty() {
            for warning in &warnings {
                log::error!("配置验证失败: {}", warning);
            }
            return Err(anyhow::anyhow!("配置路径验证失败: {}", warnings.join("; ")));
        }

        let bind_addr = format!("{}:{}", bind_ip, ftp_port);
        
        let listener = {
            use socket2::{Domain, Protocol, Socket, Type, SockAddr};
            let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
            socket.set_reuse_address(true)?;
            socket.set_nonblocking(true)?;
            let addr: std::net::SocketAddr = bind_addr.parse()
                .map_err(|e| anyhow::anyhow!("Invalid bind address: {}", e))?;
            socket.bind(&SockAddr::from(addr))?;
            socket.listen(128)?;
            tokio::net::TcpListener::from_std(socket.into())
                .map_err(|e| anyhow::anyhow!("Failed to create tokio listener: {}", e))?
        };

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        {
            let mut tx = self.shutdown_tx.lock().await;
            *tx = Some(shutdown_tx);
        }

        {
            let mut running = match self.running.lock() {
                Ok(guard) => guard,
                Err(e) => return Err(anyhow::anyhow!("获取运行状态锁失败: {}", e)),
            };
            *running = true;
        }

        let config = Arc::clone(&self.config);
        let user_manager = Arc::clone(&self.user_manager);
        let logger = Arc::clone(&self.logger);
        let file_logger = Arc::clone(&self.file_logger);
        let running_clone = Arc::clone(&self.running);

        if let Ok(mut log) = self.logger.lock() {
            log.info("FTP", &format!("FTP server started on {}", bind_addr));
        }

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((socket, peer_addr)) => {
                                let config = Arc::clone(&config);
                                let user_manager = Arc::clone(&user_manager);
                                let logger = Arc::clone(&logger);
                                let file_logger = Arc::clone(&file_logger);
                                let client_ip = peer_addr.ip().to_string();

                                if let Ok(mut log) = logger.lock() {
                                    log.client_action(
                                        "FTP",
                                        &format!("Client connected from {}", client_ip),
                                        &client_ip,
                                        None,
                                        "CONNECT",
                                    );
                                }

                                tokio::spawn(async move {
                                    if let Err(e) = session::handle_session(
                                        socket,
                                        config,
                                        user_manager,
                                        logger,
                                        file_logger,
                                        client_ip,
                                    ).await {
                                        log::debug!("FTP session error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                log::warn!("Failed to accept FTP connection: {}", e);
                            }
                        }
                    }
                }
            }

            if let Ok(mut running) = running_clone.lock() {
                *running = false;
            }
        });

        Ok(())
    }

    pub async fn stop(&self) {
        {
            let mut tx = self.shutdown_tx.lock().await;
            if let Some(sender) = tx.take() {
                let _ = sender.send(());
            }
        }
        {
            let mut running = match self.running.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    log::error!("获取运行状态锁失败: {}", e);
                    return;
                }
            };
            *running = false;
        }
        if let Ok(mut logger) = self.logger.lock() {
            logger.info("FTP", "FTP server stopped");
        }
    }

    pub fn is_running(&self) -> bool {
        match self.running.lock() {
            Ok(guard) => *guard,
            Err(_) => false,
        }
    }
}
