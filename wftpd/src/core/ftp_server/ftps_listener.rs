//! FTPS implicit TLS listener
//!
//! Listener for handling FTPS (FTP over TLS) implicit encrypted connections

use anyhow::Result;
use parking_lot::Mutex;
use std::sync::Arc;

use super::session_main::handle_session_tls;
use super::tls::TlsConfig;
use super::upnp_manager::UpnpManager;
use crate::core::config::Config;
use crate::core::fail2ban::Fail2BanManager;
use crate::core::quota::QuotaManager;
use crate::core::users::UserManager;

pub async fn start_ftps_implicit_server(
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    fail2ban_manager: Arc<Fail2BanManager>,
    upnp_manager: Option<Arc<UpnpManager>>,
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
        let domain = if bind_ip == "::" {
            Domain::IPV6
        } else {
            Domain::IPV4
        };
        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;

        if domain == Domain::IPV6 {
            socket.set_only_v6(false)?;
        }

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

                        // Check if IP is banned (Fail2Ban)
                        if fail2ban_manager.is_banned(&client_ip).await {
                            tracing::warn!(
                                "FTPS Connection rejected from {}: IP is banned by Fail2Ban",
                                client_ip
                            );
                            continue;
                        }

                        // Check IP whitelist/blacklist
                        let ip_allowed = {
                            let cfg = config.lock();
                            cfg.is_ip_allowed(&client_ip)
                        };

                        if !ip_allowed {
                            tracing::warn!(
                                "FTPS Connection rejected from {}: IP not allowed by blacklist/whitelist",
                                client_ip
                            );
                            continue;
                        }

                        // Atomically check and register connection
                        let connection_allowed = {
                            let cfg = config.lock();
                            cfg.try_register_connection(&client_ip)
                        };

                        if !connection_allowed {
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
                        let user_manager_clone = Arc::clone(&user_manager);
                        let quota_manager_clone = Arc::clone(&quota_manager);
                        let fail2ban_manager_clone = Arc::clone(&fail2ban_manager);
                        let client_ip_clone = client_ip.clone();

                        let upnp_manager_clone = upnp_manager.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_session_tls(
                                tls_stream,
                                config_for_session,
                                user_manager_clone,
                                quota_manager_clone,
                                fail2ban_manager_clone,
                                upnp_manager_clone,
                                client_ip,
                            ).await {
                                tracing::debug!("FTPS session error: {}", e);
                            }

                            // Unregister when connection ends
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
