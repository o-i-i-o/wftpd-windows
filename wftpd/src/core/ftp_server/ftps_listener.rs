use anyhow::Result;
use parking_lot::Mutex;
use std::sync::Arc;

use crate::core::config::Config;
use crate::core::fail2ban::Fail2BanManager;
use crate::core::ftp_server::tls::TlsConfig;
use crate::core::quota::QuotaManager;
use crate::core::users::UserManager;

pub async fn start_ftps_implicit_server(
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    fail2ban_manager: Arc<Fail2BanManager>,
    tls_config: TlsConfig,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    let (bind_ip, ftps_port) = {
        let cfg = config.lock();
        (cfg.ftp.bind_ip.clone(), cfg.ftp.ftps.implicit_ssl_port)
    };

    let bind_addr = format!("{}:{}", bind_ip, ftps_port);

    let listener = {
        use socket2::{Domain, Protocol, SockAddr, Socket, Type};
        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_reuse_address(true)?;
        socket.set_nonblocking(true)?;
        let addr: std::net::SocketAddr = bind_addr
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid bind address: {}", e))?;
        socket.bind(&SockAddr::from(addr))?;
        socket.listen(128)?;
        tokio::net::TcpListener::from_std(socket.into())
            .map_err(|e| anyhow::anyhow!("Failed to create tokio listener: {}", e))?
    };

    tracing::info!("FTPS server (implicit SSL) started on {}", bind_addr);

    let tls_acceptor = match &tls_config.acceptor {
        Some(acceptor) => acceptor.clone(),
        None => {
            return Err(anyhow::anyhow!("TLS acceptor not available"));
        }
    };

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                break;
            }
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((socket, peer_addr)) => {
                        let client_ip = peer_addr.ip().to_string();

                        // 原子化检查+注册连接（与普通 FTP 服务器一致）
                        let ip_allowed = {
                            let cfg = config.lock();
                            cfg.try_register_connection(&client_ip)
                        };

                        if !ip_allowed {
                            tracing::warn!(
                                "FTPS Connection rejected from {}: connection limit exceeded",
                                client_ip
                            );
                            continue;
                        }

                        tracing::info!(
                            client_ip = %client_ip,
                            action = "CONNECT",
                            protocol = "FTPS",
                            "FTPS client connected from {}", client_ip
                        );

                        let tls_stream = match tls_acceptor.accept(socket).await {
                            Ok(stream) => stream,
                            Err(e) => {
                                tracing::error!("FTPS TLS handshake failed: {}", e);
                                continue;
                            }
                        };

                        let config_for_session = Arc::clone(&config);
                        let config_for_cleanup = Arc::clone(&config);
                        let user_manager = Arc::clone(&user_manager);
                        let quota_manager = Arc::clone(&quota_manager);
                        let fail2ban_manager = Arc::clone(&fail2ban_manager);
                        let client_ip_clone = client_ip.clone();

                        tokio::spawn(async move {
                            if let Err(e) = crate::core::ftp_server::session::handle_session_tls(
                                tls_stream,
                                config_for_session,
                                user_manager,
                                quota_manager,
                                fail2ban_manager,
                                client_ip,
                            ).await {
                                tracing::debug!("FTPS session error: {}", e);
                            }

                            // 连接结束时注销
                            {
                                let cfg = config_for_cleanup.lock();
                                cfg.unregister_connection(&client_ip_clone);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Failed to accept FTPS connection: {}", e);
                    }
                }
            }
        }
    }

    tracing::info!("FTPS server stopped");

    Ok(())
}
