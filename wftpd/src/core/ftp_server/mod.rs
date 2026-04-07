//! FTP 服务器核心模块
//!
//! 提供 FTP 服务器的启动、监听和会话管理功能

mod cert_gen;
mod commands;
mod passive;
pub mod session;
pub mod session_auth;
pub mod session_cmds;
pub mod session_dirs;
pub mod session_ip;
pub mod session_main;
pub mod session_site;
pub mod session_state;
pub mod session_xfer;
mod tls;
mod transfer;
mod upnp_manager;

mod ftps_listener;

use anyhow::Result;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use crate::core::config::{Config, get_program_data_path};
use crate::core::fail2ban::{Fail2BanConfig, Fail2BanManager};
use crate::core::ftp_server::tls::TlsConfig;
use crate::core::ftp_server::upnp_manager::UpnpManager;
use crate::core::quota::QuotaManager;
use crate::core::users::UserManager;

pub struct FtpServer {
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    fail2ban_manager: Arc<Fail2BanManager>,
    upnp_manager: Arc<UpnpManager>,
    running: Arc<Mutex<bool>>,
    shutdown_tx: Arc<TokioMutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    ftps_shutdown_tx: Arc<TokioMutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    tls_config: Arc<Mutex<Option<TlsConfig>>>,
}

impl FtpServer {
    pub fn new(config: Arc<Mutex<Config>>, user_manager: Arc<Mutex<UserManager>>) -> Self {
        let tls_config = {
            let cfg = config.lock();
            if cfg.ftp.ftps.enabled {
                let cert_path = cfg.ftp.ftps.cert_path.as_deref();
                let key_path = cfg.ftp.ftps.key_path.as_deref();
                Some(TlsConfig::new(
                    cert_path,
                    key_path,
                    cfg.ftp.ftps.require_ssl,
                ))
            } else {
                None
            }
        };

        let quota_manager = QuotaManager::new(&get_program_data_path());

        // 初始化 Fail2Ban 管理器
        let fail2ban_config_inner = {
            let cfg = config.lock();
            Fail2BanConfig {
                enabled: cfg.security.fail2ban_enabled,
                threshold: cfg.security.fail2ban_threshold,
                ban_time: cfg.security.fail2ban_ban_time,
                find_time: 600, // 10 分钟检测窗口
            }
        };
        let fail2ban_config = Arc::new(Mutex::new(fail2ban_config_inner));
        let fail2ban_manager = Arc::new(Fail2BanManager::new(fail2ban_config.clone()));

        // 初始化 UPnP 管理器
        let upnp_enabled = {
            let cfg = config.lock();
            cfg.ftp.upnp_enabled
        };
        let upnp_manager = Arc::new(UpnpManager::new(upnp_enabled));

        FtpServer {
            config,
            user_manager,
            quota_manager: Arc::new(quota_manager),
            fail2ban_manager,
            upnp_manager,
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
                cfg.ftp.port,
                warnings,
                cfg.ftp.ftps.enabled,
                cfg.ftp.ftps.implicit_ssl,
                cfg.ftp.ftps.implicit_ssl_port,
            )
        };

        if !warnings.is_empty() {
            for warning in &warnings {
                tracing::error!("配置验证失败：{}", warning);
            }
            return Err(anyhow::anyhow!("配置路径验证失败：{}", warnings.join("; ")));
        }

        // 根据配置的绑定地址确定监听方式
        let bind_addr = format!("{}:{}", bind_ip, ftp_port);

        tracing::info!("FTP server starting on {}", bind_addr);

        let listener = {
            use socket2::{Domain, Protocol, SockAddr, Socket, Type};
            // 根据配置的地址类型选择 IPv4 或 IPv6
            let domain = if bind_ip == "::" || (bind_ip.starts_with('[') && bind_ip.ends_with(']'))
            {
                Domain::IPV6
            } else {
                Domain::IPV4
            };

            let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;

            // 仅在配置为 [::] 时启用 IPv6 双栈支持
            if domain == Domain::IPV6 {
                socket.set_only_v6(false)?; // 允许 IPv4 映射到 IPv6
            }

            socket.set_reuse_address(true)?;
            socket.set_nonblocking(true)?;
            let addr: std::net::SocketAddr = bind_addr
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid bind address '{}': {}", bind_addr, e))?;
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

        let config_spawn = Arc::clone(&self.config);
        let user_manager_spawn = Arc::clone(&self.user_manager);
        let quota_manager_spawn = Arc::clone(&self.quota_manager);
        let fail2ban_spawn = Arc::clone(&self.fail2ban_manager);
        let upnp_spawn = Arc::clone(&self.upnp_manager);
        let running_clone = Arc::clone(&self.running);

        // 启动 Fail2Ban 后台清理任务
        Arc::clone(&fail2ban_spawn).start_cleanup_task();

        // 初始化 UPnP 端口映射（在 runtime 上下文中）
        let upnp_init = Arc::clone(&upnp_spawn);
        tokio::spawn(async move {
            if let Err(e) = upnp_init.initialize().await {
                tracing::warn!("UPnP 初始化失败: {}", e);
            }
        });

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

                let config_clone = Arc::clone(&config_spawn);
                let user_manager_clone = Arc::clone(&user_manager_spawn);
                let quota_manager_clone = Arc::clone(&quota_manager_spawn);
                let fail2ban_clone = Arc::clone(&fail2ban_spawn);
                let upnp_clone = Arc::clone(&upnp_spawn);

                tokio::spawn(async move {
                    if let Err(e) = ftps_listener::start_ftps_implicit_server(
                        config_clone,
                        user_manager_clone,
                        quota_manager_clone,
                        fail2ban_clone,
                        Some(upnp_clone),
                        tls_cfg,
                        ftps_shutdown_rx,
                    )
                    .await
                    {
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
                                let client_ip = peer_addr.ip().to_string();
                                let config_arc = Arc::clone(&config_spawn);

                                // 优化的并发控制：在单个锁保护下完成所有检查
                                let checks_result = {
                                    let cfg = config_arc.lock();
                                    let ip_allowed = cfg.is_ip_allowed(&client_ip);
                                    let connection_allowed = if ip_allowed {
                                        cfg.try_register_connection(&client_ip)
                                    } else {
                                        false
                                    };
                                    (ip_allowed, connection_allowed)
                                };

                                let (ip_allowed, connection_allowed) = checks_result;

                                // 检查 IP 是否被封禁 (Fail2Ban) - 独立检查，不使用锁
                                if fail2ban_spawn.is_banned(&client_ip).await {
                                    tracing::warn!(
                                        "Connection rejected from {}: IP is banned by Fail2Ban",
                                        client_ip
                                    );
                                    use tokio::io::AsyncWriteExt;
                                    let mut socket = socket;
                                    let _ = socket.write_all(
                                        b"421 Your IP has been banned - connection refused\r\n"
                                    ).await;
                                    continue;
                                }

                                if !ip_allowed {
                                    tracing::warn!(
                                        "Connection rejected from {}: IP not allowed by blacklist/whitelist",
                                        client_ip
                                    );
                                    use tokio::io::AsyncWriteExt;
                                    let mut socket = socket;
                                    let _ = socket.write_all(
                                        b"530 Connection denied by IP filter\r\n"
                                    ).await;
                                    continue;
                                }

                                if !connection_allowed {
                                    tracing::warn!(
                                        "Connection rejected from {}: connection limit exceeded",
                                        client_ip
                                    );
                                    use tokio::io::AsyncWriteExt;
                                    let mut socket = socket;
                                    let _ = socket.write_all(
                                        b"421 Too many connections - please try again later\r\n"
                                    ).await;
                                    continue;
                                }

                                let _user_manager = Arc::clone(&user_manager_spawn);
                                let _quota_manager = Arc::clone(&quota_manager_spawn);
                                let client_ip_clone = client_ip.clone();

                                tracing::info!(
                                    client_ip = %client_ip,
                                    action = "CONNECT",
                                    protocol = "FTP",
                                    "Client connected from {}", client_ip
                                );

                                let config_for_spawn = Arc::clone(&config_spawn);
                                let user_manager_for_spawn = Arc::clone(&user_manager_spawn);
                                let quota_manager_for_spawn = Arc::clone(&quota_manager_spawn);
                                let fail2ban_for_spawn = Arc::clone(&fail2ban_spawn);
                                let upnp_for_spawn = Arc::clone(&upnp_spawn);
                                tokio::spawn(async move {
                                    if let Err(e) = session::handle_session(
                                        socket,
                                        config_for_spawn,
                                        user_manager_for_spawn,
                                        quota_manager_for_spawn,
                                        fail2ban_for_spawn,
                                        Some(upnp_for_spawn),
                                        client_ip,
                                    ).await {
                                        tracing::debug!("FTP session error: {}", e);
                                    }

                                    // 连接结束时注销
                                    {
                                        let cfg = config_arc.lock();
                                        cfg.unregister_connection(&client_ip_clone);
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
