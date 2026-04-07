//! FTP 会话主处理流程
//!
//! 处理 FTP 控制连接和命令分发的核心逻辑

use std::net::IpAddr;
use std::sync::Arc;
use parking_lot::Mutex;

use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::core::config::Config;
use crate::core::quota::QuotaManager;
use crate::core::users::UserManager;
use crate::core::fail2ban::Fail2BanManager;

use super::commands::FtpCommand;

use super::session_state::{ControlStream, SessionState};
use super::session_auth::{CommandContext, handle_auth_command};
use super::session_cmds::{handle_basic_command, handle_help_command, handle_stat_command};
use super::session_dirs::handle_directory_command;
use super::session_xfer::{handle_transfer_command, handle_list_command, handle_retrieve_command, handle_store_command, handle_fileinfo_command};
use super::session_site::handle_site_command;
use super::upnp_manager::UpnpManager;

const MAX_COMMAND_LENGTH: usize = 8192;

pub async fn handle_session(
    mut socket: TcpStream,
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    fail2ban_manager: Arc<Fail2BanManager>,
    upnp_manager: Option<Arc<UpnpManager>>,
    client_ip: String,
) -> Result<()> {
    let local_addr = socket.local_addr()?;
    let server_local_ip = {
        let local_ip = local_addr.ip();
        if local_ip.is_unspecified() {
            super::session_ip::get_local_ip_for_client(&client_ip)
        } else {
            match local_ip {
                IpAddr::V4(ipv4) => ipv4.to_string(),
                IpAddr::V6(ipv6) => {
                    if let Some(ipv4) = ipv6.to_ipv4_mapped() {
                        ipv4.to_string()
                    } else {
                        tracing::warn!("Server has pure IPv6 address {}, using fallback", ipv6);
                        super::session_ip::get_local_ip_for_client(&client_ip)
                    }
                }
            }
        }
    };

    tracing::debug!(
        "FTP session: client_ip={}, server_local_ip={}, socket_local_addr={}",
        client_ip, server_local_ip, local_addr
    );

    let session_config = {
        let cfg = config.lock();
        super::session_state::SessionConfig::from_config(&cfg, &client_ip)
    };

    let passive_timeout = session_config.idle_timeout;

    if !session_config.ip_allowed {
        tracing::warn!("Connection rejected from {} by IP filter", client_ip);
        if let Err(e) = socket
            .write_all(b"530 Connection denied by IP filter\r\n")
            .await
        {
            tracing::debug!("Failed to send IP filter rejection to {}: {}", client_ip, e);
        }
        return Ok(());
    }

    let mut control_stream = ControlStream::Plain(Some(socket));
    control_stream
        .write_response(
            format!("220 {}\r\n", session_config.welcome_msg).as_bytes(),
            "welcome message",
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
                        .write_response(b"500 Command too long\r\n", "command too long")
                        .await;
                    cmd_buffer.clear();
                    continue;
                }

                while let Some(crlf_pos) = cmd_buffer.windows(2).position(|w| w == b"\r\n") {
                    let command_bytes: Vec<u8> = cmd_buffer.drain(..crlf_pos + 2).collect();
                    let command = String::from_utf8_lossy(
                        &command_bytes[..command_bytes.len().saturating_sub(2)],
                    )
                    .trim()
                    .to_string();

                    let parts: Vec<&str> = command.splitn(2, ' ').collect();
                    let cmd = parts[0].to_uppercase();
                    let arg = parts.get(1).map(|s| s.trim());

                    let ftp_cmd = FtpCommand::parse(&cmd, arg);

                    let ctx = CommandContext {
                        config: &config,
                        user_manager: &user_manager,
                        quota_manager: &quota_manager,
                        fail2ban_manager: &fail2ban_manager,
                        client_ip: &client_ip,
                        allow_anonymous: &session_config.allow_anonymous,
                        anonymous_home: &session_config.anonymous_home,
                        tls_config: &session_config.tls_config,
                        require_ssl: session_config.require_ssl,
                    };

                    if !dispatch_command(&mut control_stream, &ftp_cmd, &mut state, &ctx).await? {
                        return Ok(());
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::debug!("读取错误: {}", e);
                break;
            }
            Err(_) => {
                control_stream
                    .write_response(b"421 Connection timed out\r\n", "connection timeout")
                    .await;
                break;
            }
        }
    }

    Ok(())
}



pub async fn dispatch_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    ctx: &CommandContext<'_>,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    match cmd {
        AUTH(_) | PBSZ(_) | PROT(_) | CCC | ADAT(_) | MIC(_) | CONF(_) | ENC(_) | USER(_) | PASS(_) => {
            return handle_auth_command(control_stream, cmd, state, ctx).await;
        }
        QUIT | SYST | FEAT | NOOP | OPTS(_, _) | TYPE(_) | MODE(_) | STRU(_) | ALLO | REST(_) | ACCT | REIN | ABOR => {
            return handle_basic_command(control_stream, cmd, state, ctx).await;
        }
        HELP(_) => {
            return handle_help_command(control_stream, cmd).await;
        }
        STAT => {
            return handle_stat_command(control_stream, cmd, state, ctx).await;
        }
        PWD | XPWD | CWD(_) | CDUP | XCUP | MKD(_) | RMD(_) | RNFR(_) | RNTO(_) | DELE(_) => {
            return handle_directory_command(control_stream, cmd, state, ctx).await;
        }
        PASV | EPSV | PORT(_) | EPRT(_) => {
            return handle_transfer_command(control_stream, cmd, state, ctx).await;
        }
        LIST(_) | NLST(_) | MLSD(_) | MLST(_) => {
            return handle_list_command(control_stream, cmd, state, ctx).await;
        }
        RETR(_) => {
            return handle_retrieve_command(control_stream, cmd, state, ctx).await;
        }
        STOR(_) | APPE(_) | STOU => {
            return handle_store_command(control_stream, cmd, state, ctx).await;
        }
        SIZE(_) | MDTM(_) => {
            return handle_fileinfo_command(control_stream, cmd, state, ctx).await;
        }
        SITE(_) => {
            return handle_site_command(control_stream, cmd, state, ctx).await;
        }
        Unknown(_) => {
            control_stream
                .write_response(
                    format!("202 Command not implemented: {}\r\n", "").as_bytes(),
                    "FTP response",
                )
                .await;
            Ok(true)
        }
    }
}
