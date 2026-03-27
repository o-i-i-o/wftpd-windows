mod session;
mod commands;
mod passive;
mod transfer;
mod tls;

mod ftps_listener;

use anyhow::Result;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use crate::core::config::{Config, get_program_data_path};
use crate::core::users::UserManager;
use crate::core::quota::QuotaManager;

use crate::core::ftp_server::tls::TlsConfig;

pub struct FtpServer {
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    running: Arc<Mutex<bool>>,
    shutdown_tx: Arc<TokioMutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    ftps_shutdown_tx: Arc<TokioMutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    tls_config: Arc<Mutex<Option<TlsConfig>>>,
}

impl FtpServer {
    pub fn new(
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> Self {
        let tls_config = {
            let cfg = config.lock();
            if cfg.ftp.ftps.enabled {
                let cert_path = cfg.ftp.ftps.cert_path.as_deref();
                let key_path = cfg.ftp.ftps.key_path.as_deref();
                Some(TlsConfig::new(cert_path, key_path, cfg.ftp.ftps.require_ssl))
            } else {
                None
            }
        };

        let quota_manager = QuotaManager::new(&get_program_data_path());

        FtpServer {
            config,
            user_manager,
            quota_manager: Arc::new(quota_manager),
            running: Arc::new(Mutex::new(false)),
            shutdown_tx: Arc::new(TokioMutex::new(None)),
            ftps_shutdown_tx: Arc::new(TokioMutex::new(None)),
            tls_config: Arc::new(Mutex::new(tls_config)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let (bind_ip, ftp_port, warnings, ftps_enabled, ftps_implicit, ftps_port) = {
            let cfg = self.config.lock();
            let warnings = cfg.validate_paths();
            (
                cfg.ftp.bind_ip.clone(),
                cfg.server.ftp_port,
                warnings,
                cfg.ftp.ftps.enabled,
                cfg.ftp.ftps.implicit_ssl,
                cfg.ftp.ftps.implicit_ssl_port,
            )
        };

        if !warnings.is_empty() {
            for warning in &warnings {
                tracing::error!("配置验证失败: {}", warning);
            }
            return Err(anyhow::anyhow!("配置路径验证失败: {}", warnings.join("; ")));
        }

        let bind_addr = format!("{}:{}", bind_ip, ftp_port);
        tracing::info!("FTP server starting on {}", bind_addr);
        
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
            let mut running = self.running.lock();
            *running = true;
        }

        let config = Arc::clone(&self.config);
        let user_manager = Arc::clone(&self.user_manager);
        let quota_manager = Arc::clone(&self.quota_manager);
        let running_clone = Arc::clone(&self.running);

        tracing::info!("FTP server started on {}", bind_addr);

        if ftps_enabled && ftps_implicit {
            let tls_cfg_opt = self.tls_config.lock().clone();
            if let Some(tls_cfg) = tls_cfg_opt {
                let (ftps_shutdown_tx, ftps_shutdown_rx) = tokio::sync::oneshot::channel();
                {
                    let mut tx = self.ftps_shutdown_tx.lock().await;
                    *tx = Some(ftps_shutdown_tx);
                }
                
                let ftps_bind_addr = format!("{}:{}", bind_ip, ftps_port);
                tracing::info!("FTPS implicit SSL server starting on {}", ftps_bind_addr);
                
                let config_clone = Arc::clone(&config);
                let user_manager_clone = Arc::clone(&user_manager);
                let quota_manager_clone = Arc::clone(&quota_manager);
                
                tokio::spawn(async move {
                    if let Err(e) = ftps_listener::start_ftps_implicit_server(
                        config_clone,
                        user_manager_clone,
                        quota_manager_clone,
                        tls_cfg,
                        ftps_shutdown_rx,
                    ).await {
                        tracing::error!("FTPS implicit SSL server error: {}", e);
                    }
                });
            }
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
                                let quota_manager = Arc::clone(&quota_manager);
                                let client_ip = peer_addr.ip().to_string();

                                tracing::info!(
                                    client_ip = %client_ip,
                                    action = "CONNECT",
                                    "Client connected from {}", client_ip
                                );

                                tokio::spawn(async move {
                                    if let Err(e) = session::handle_session(
                                        socket,
                                        config,
                                        user_manager,
                                        quota_manager,
                                        client_ip,
                                    ).await {
                                        tracing::debug!("FTP session error: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::warn!("Failed to accept FTP connection: {}", e);
                            }
                        }
                    }
                }
            }

            let mut running = running_clone.lock();
            *running = false;
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
            let mut running = self.running.lock();
            *running = false;
        }
        tracing::info!("FTP server stopped");
    }

    pub fn is_running(&self) -> bool {
        *self.running.lock()
    }
}
