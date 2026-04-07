//! FTPS 隐式 TLS 监听器
//!
//! 处理 FTPS (FTP over TLS) 隐式加密连接的监听器

use anyhow::Result;
use parking_lot::Mutex;
use std::sync::Arc;

use crate::core::config::Config;
use crate::core::fail2ban::Fail2BanManager;
use crate::core::ftp_server::session::dispatch_command;
use crate::core::ftp_server::session_state::{ControlStream, SessionState, SessionConfig};
use crate::core::ftp_server::session_auth::CommandContext;
use crate::core::ftp_server::commands::FtpCommand;
use crate::core::ftp_server::tls::TlsConfig;
use crate::core::ftp_server::upnp_manager::UpnpManager;
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
        let domain = if bind_ip == "::" { Domain::IPV6 } else { Domain::IPV4 };
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

                        // 检查 IP 是否被封禁 (Fail2Ban)
                        if fail2ban_manager.is_banned(&client_ip).await {
                            tracing::warn!(
                                "FTPS Connection rejected from {}: IP is banned by Fail2Ban",
                                client_ip
                            );
                            continue;
                        }

                        // 检查 IP 黑白名单
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

                        // 原子化检查+注册连接
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

pub async fn handle_session_tls(
    socket: super::tls::AsyncTlsTcpStream,
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    fail2ban_manager: Arc<Fail2BanManager>,
    upnp_manager: Option<Arc<UpnpManager>>,
    client_ip: String,
) -> Result<()> {
    use std::net::IpAddr;
    use crate::core::ftp_server::session_ip::get_local_ip_for_client;

    const MAX_COMMAND_LENGTH: usize = 8192;

    let local_addr = socket.get_ref().0.local_addr()?;
    let server_local_ip = {
        let local_ip = local_addr.ip();
        if local_ip.is_unspecified() {
            get_local_ip_for_client(&client_ip)
        } else {
            match local_ip {
                IpAddr::V4(ipv4) => ipv4.to_string(),
                IpAddr::V6(ipv6) => {
                    if let Some(ipv4) = ipv6.to_ipv4_mapped() {
                        ipv4.to_string()
                    } else {
                        tracing::warn!("Server has pure IPv6 address {}, using fallback", ipv6);
                        get_local_ip_for_client(&client_ip)
                    }
                }
            }
        }
    };

    tracing::debug!(
        "FTP TLS session: client_ip={}, server_local_ip={}, socket_local_addr={}",
        client_ip, server_local_ip, local_addr
    );

    let session_config = {
        let cfg = config.lock();
        SessionConfig::from_config(&cfg, &client_ip)
    };

    let passive_timeout = session_config.idle_timeout;

    if !session_config.ip_allowed {
        tracing::warn!("Connection rejected from {} by IP filter", client_ip);
        return Ok(());
    }

    let mut control_stream = ControlStream::Tls(Box::new(socket));
    control_stream
        .write_response(
            format!("220 {}\r\n", session_config.welcome_msg).as_bytes(),
            "welcome message (TLS)",
        )
        .await;

    let allow_symlinks = {
        let cfg = config.lock();
        cfg.security.allow_symlinks
    };
    let mut state = SessionState::new(&client_ip, &server_local_ip, allow_symlinks, upnp_manager);
    state.transfer_mode = session_config.default_transfer_mode;
    state.passive_mode = session_config.default_passive_mode;
    state.encoding = session_config.encoding;
    state.tls_enabled = true;

    let (allow_anonymous, anonymous_home, tls_config, require_ssl) = {
        let cfg = config.lock();
        (
            cfg.ftp.allow_anonymous,
            cfg.ftp.anonymous_home.clone(),
            TlsConfig::new(
                cfg.ftp.ftps.cert_path.as_deref(),
                cfg.ftp.ftps.key_path.as_deref(),
                cfg.ftp.ftps.require_ssl,
            ),
            cfg.ftp.ftps.enabled && cfg.ftp.ftps.require_ssl,
        )
    };

    let mut cmd_buffer: Vec<u8> = Vec::with_capacity(MAX_COMMAND_LENGTH);
    let mut read_buffer = [0u8; 4096];
    let mut last_activity = std::time::Instant::now();

    loop {
        let (conn_timeout, idle_timeout) = {
            let cfg = config.lock();
            (cfg.ftp.connection_timeout, cfg.ftp.idle_timeout)
        };

        if idle_timeout > 0 {
            let idle_duration = last_activity.elapsed();
            if idle_duration > std::time::Duration::from_secs(idle_timeout) {
                control_stream
                    .write_response(b"421 Idle timeout - closing connection\r\n", "idle timeout")
                    .await;
                break;
            }
        }

        state.passive_manager.cleanup_expired(passive_timeout);

        let timeout_result = tokio::time::timeout(
            std::time::Duration::from_secs(conn_timeout),
            control_stream.read(&mut read_buffer),
        )
        .await;

        match timeout_result {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                last_activity = std::time::Instant::now();
                cmd_buffer.extend_from_slice(&read_buffer[..n]);

                if cmd_buffer.len() > MAX_COMMAND_LENGTH {
                    control_stream
                        .write_response(b"500 Command too long\r\n", "command too long (TLS)")
                        .await;
                    cmd_buffer.clear();
                    continue;
                }

                while let Some(pos) = cmd_buffer.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = cmd_buffer.drain(..=pos).collect();
                    let line_str = String::from_utf8_lossy(&line);
                    let line_str = line_str.trim_end_matches('\r').trim_end_matches('\n');

                    if line_str.is_empty() {
                        continue;
                    }

                    let cmd = {
                        let parts: Vec<&str> = line_str.splitn(2, ' ').collect();
                        let cmd_str = parts[0].trim().to_uppercase();
                        let arg = parts.get(1).map(|s| s.trim());
                        FtpCommand::parse(&cmd_str, arg)
                    };

                    let ctx = CommandContext {
                        config: &config,
                        user_manager: &user_manager,
                        quota_manager: &quota_manager,
                        fail2ban_manager: &fail2ban_manager,
                        client_ip: &client_ip,
                        allow_anonymous: &allow_anonymous,
                        anonymous_home: &anonymous_home,
                        tls_config: &tls_config,
                        require_ssl,
                    };

                    if !dispatch_command(&mut control_stream, &cmd, &mut state, &ctx).await? {
                        return Ok(());
                    }
                }
            }
            Ok(Err(_)) | Err(_) => {
                control_stream
                    .write_response(b"421 Connection timed out\r\n", "connection timeout (TLS)")
                    .await;
                break;
            }
        }
    }

    Ok(())
}
