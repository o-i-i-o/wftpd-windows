//! FTP file transfer command handler
//!
//! Handles RETR, STOR, LIST, NLST and other file transfer commands

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::core::path_utils::{path_starts_with_ignore_case, to_ftp_path};
use crate::core::rate_limiter::RateLimiter;

use super::commands::FtpCommand;
use super::session_state::{ControlStream, SessionState};
use super::transfer;

pub async fn handle_transfer_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    ctx: &super::session_auth::CommandContext<'_>,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    if !state.authenticated {
        control_stream
            .write_response(b"530 Not logged in\r\n", "FTP response")
            .await;
        return Ok(true);
    }

    match cmd {
        PASV => {
            let pasv_config = {
                let cfg = ctx.config.lock();
                super::passive::PasvConfig {
                    client_ip: ctx.client_ip.to_string(),
                    server_local_ip: state.server_local_ip.clone(),
                    bind_ip: cfg.ftp.bind_ip.clone(),
                    port_range: cfg.ftp.passive_ports,
                    masquerade_address: cfg.ftp.masquerade_address.clone(),
                    passive_ip_override: cfg.ftp.passive_ip_override.clone(),
                    masquerade_map: cfg.ftp.masquerade_map.clone(),
                    listener_timeout_secs: cfg.ftp.idle_timeout,
                }
            };

            match state.passive_manager.handle_pasv(&pasv_config).await {
                Ok((passive_port, response_ip)) => {
                    state.passive_mode = true;
                    state.data_port = Some(passive_port);

                    // Validate response IP address format
                    let ip_parts: Vec<&str> = response_ip.split('.').collect();
                    if ip_parts.len() != 4 {
                        tracing::error!(
                            "PASV: Invalid IPv4 address format '{}' returned from handle_pasv. \
                             Please check masquerade_address and passive_ip_override configuration.",
                            response_ip
                        );
                        control_stream
                            .write_response(
                                b"425 Cannot determine valid passive mode IP - configuration error\r\n",
                                "PASV response",
                            )
                            .await;
                        return Ok(true);
                    }

                    let p1 = passive_port >> 8;
                    let p2 = passive_port & 0xFF;

                    control_stream
                        .write_response(
                            format!(
                                "227 Entering Passive Mode ({},{},{},{},{},{}).\r\n",
                                ip_parts[0], ip_parts[1], ip_parts[2], ip_parts[3], p1, p2
                            )
                            .as_bytes(),
                            "PASV response",
                        )
                        .await;

                    tracing::info!(
                        client_ip = %ctx.client_ip,
                        username = ?state.current_user.as_deref(),
                        action = "PASV",
                        protocol = "FTP",
                        "PASV mode: port {} on IP {}", passive_port, response_ip
                    );
                }
                Err(e) => {
                    control_stream
                        .write_response(
                            format!("425 Could not enter passive mode: {}\r\n", e).as_bytes(),
                            "FTP response",
                        )
                        .await;
                }
            }
        }

        EPSV => {
            let pasv_config = {
                let cfg = ctx.config.lock();
                super::passive::PasvConfig {
                    client_ip: ctx.client_ip.to_string(),
                    server_local_ip: state.server_local_ip.clone(),
                    bind_ip: cfg.ftp.bind_ip.clone(),
                    port_range: cfg.ftp.passive_ports,
                    masquerade_address: cfg.ftp.masquerade_address.clone(),
                    passive_ip_override: cfg.ftp.passive_ip_override.clone(),
                    masquerade_map: cfg.ftp.masquerade_map.clone(),
                    listener_timeout_secs: cfg.ftp.idle_timeout,
                }
            };

            match state.passive_manager.handle_epsv(&pasv_config).await {
                Ok(passive_port) => {
                    state.passive_mode = true;
                    state.data_port = Some(passive_port);

                    control_stream
                        .write_response(
                            format!(
                                "229 Entering Extended Passive Mode (|||{}|)\r\n",
                                passive_port
                            )
                            .as_bytes(),
                            "EPSV response",
                        )
                        .await;

                    tracing::info!(
                        client_ip = %ctx.client_ip,
                        username = ?state.current_user.as_deref(),
                        action = "EPSV",
                        protocol = "FTP",
                        "EPSV mode: port {}", passive_port
                    );
                }
                Err(e) => {
                    control_stream
                        .write_response(
                            format!("425 Could not enter extended passive mode: {}\r\n", e)
                                .as_bytes(),
                            "FTP response",
                        )
                        .await;
                }
            }
        }

        PORT(data) => {
            if let Some(data) = data {
                let parts: Vec<u16> = data.split(',').filter_map(|s| s.parse().ok()).collect();
                if parts.len() == 6 {
                    if !state.validate_port_ip(data) {
                        control_stream.write_response(b"500 PORT command rejected: IP address must match control connection\r\n", "FTP response").await;
                        return Ok(true);
                    }

                    let port = parts[4] * 256 + parts[5];
                    let addr = format!(
                        "{}.{}.{}.{}:{}",
                        parts[0], parts[1], parts[2], parts[3], port
                    );
                    state.data_port = Some(port);
                    state.data_addr = Some(addr);
                    state.passive_mode = false;
                    control_stream
                        .write_response(b"200 PORT command successful\r\n", "FTP response")
                        .await;
                } else {
                    control_stream
                        .write_response(
                            b"501 Syntax error in parameters or arguments\r\n",
                            "FTP response",
                        )
                        .await;
                }
            } else {
                control_stream
                    .write_response(
                        b"501 Syntax error: PORT requires parameters\r\n",
                        "FTP response",
                    )
                    .await;
            }
        }

        EPRT(data) => {
            if let Some(data) = data {
                let parts: Vec<&str> = data.split('|').collect();
                if parts.len() >= 4 {
                    let net_proto = parts[1];
                    let net_addr = parts[2];
                    let tcp_port = parts[3];

                    match net_proto {
                        "1" => {
                            if let Ok(port) = tcp_port.parse::<u16>() {
                                if !state.validate_eprt_ip(net_addr) {
                                    control_stream.write_response(b"500 EPRT command rejected: IP address must match control connection\r\n", "FTP response").await;
                                    return Ok(true);
                                }
                                state.data_port = Some(port);
                                state.data_addr = Some(format!("{}:{}", net_addr, port));
                                state.passive_mode = false;
                                control_stream
                                    .write_response(
                                        b"200 EPRT command successful\r\n",
                                        "FTP response",
                                    )
                                    .await;
                            } else {
                                control_stream
                                    .write_response(b"501 Invalid port number\r\n", "FTP response")
                                    .await;
                            }
                        }
                        "2" => {
                            if let Ok(port) = tcp_port.parse::<u16>() {
                                if !state.validate_eprt_ip(net_addr) {
                                    control_stream.write_response(b"500 EPRT command rejected: IP address must match control connection\r\n", "FTP response").await;
                                    return Ok(true);
                                }
                                state.data_port = Some(port);
                                state.data_addr = Some(format!("[{}]:{}", net_addr, port));
                                state.passive_mode = false;
                                control_stream
                                    .write_response(
                                        b"200 EPRT command successful (IPv6)\r\n",
                                        "FTP response",
                                    )
                                    .await;
                            } else {
                                control_stream
                                    .write_response(b"501 Invalid port number\r\n", "FTP response")
                                    .await;
                            }
                        }
                        _ => {
                            control_stream
                                .write_response(
                                    b"522 Protocol not supported, use (1,2)\r\n",
                                    "FTP response",
                                )
                                .await;
                        }
                    }
                } else {
                    control_stream
                        .write_response(b"501 Syntax error in EPRT parameters\r\n", "FTP response")
                        .await;
                }
            } else {
                control_stream
                    .write_response(
                        b"501 Syntax error: EPRT requires parameters\r\n",
                        "FTP response",
                    )
                    .await;
            }
        }

        _ => return Ok(true),
    }

    Ok(true)
}

pub async fn handle_list_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    ctx: &super::session_auth::CommandContext<'_>,
) -> Result<bool> {
    use super::commands::FtpCommand::*;
    use crate::core::path_utils::resolve_directory_path;

    match cmd {
        LIST(path) | NLST(path) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let can_list = {
                let users = ctx.user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_none_or(|u| u.permissions.can_list)
            };

            if !can_list {
                control_stream
                    .write_response(b"550 Permission denied\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let list_path = if let Some(path_arg) = path {
                match resolve_directory_path(&state.cwd, &state.home_dir, path_arg) {
                    Ok(path) => path,
                    Err(crate::core::path_utils::PathResolveError::PathEscape) => {
                        control_stream
                            .write_response(b"550 Permission denied\r\n", "FTP response")
                            .await;
                        return Ok(true);
                    }
                    Err(crate::core::path_utils::PathResolveError::NotADirectory) => {
                        control_stream
                            .write_response(b"550 Not a directory\r\n", "FTP response")
                            .await;
                        return Ok(true);
                    }
                    Err(crate::core::path_utils::PathResolveError::NotFound) => {
                        control_stream
                            .write_response(b"550 Directory not found\r\n", "FTP response")
                            .await;
                        return Ok(true);
                    }
                    Err(_) => {
                        control_stream
                            .write_response(b"550 Failed to resolve path\r\n", "FTP response")
                            .await;
                        return Ok(true);
                    }
                }
            } else {
                PathBuf::from(&state.cwd)
            };

            control_stream
                .write_response(b"150 Here comes the directory listing\r\n", "FTP response")
                .await;

            let current_username = state
                .current_user
                .clone()
                .unwrap_or_else(|| "anonymous".to_string());
            let is_ascii = state.transfer_mode == "ascii";
            let mut transfer_ok = false;

            if let Ok(mut data_stream) = transfer::get_data_connection(
                state.passive_mode,
                state.data_port,
                &state.data_addr,
                ctx.client_ip,
                &mut state.passive_manager,
                state.data_protection,
                ctx.tls_config.acceptor.as_deref(),
            )
            .await
            {
                let is_nlst = matches!(cmd, NLST(_));
                match transfer::send_directory_listing(
                    &mut data_stream,
                    &list_path,
                    &current_username,
                    is_nlst,
                    is_ascii,
                )
                .await
                {
                    Ok(()) => transfer_ok = true,
                    Err(e) => tracing::warn!("LIST/NLST transfer error: {}", e),
                }
            }

            if state.passive_mode
                && let Some(port) = state.data_port
            {
                state.passive_manager.remove_listener(port);
            }
            state.data_port = None;
            state.data_addr = None;

            if transfer_ok {
                control_stream
                    .write_response(b"226 Transfer complete\r\n", "FTP response")
                    .await;
            } else {
                control_stream
                    .write_response(b"426 Transfer aborted\r\n", "FTP response")
                    .await;
            }
        }

        MLSD(path) | MLST(path) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let can_list = {
                let users = ctx.user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_none_or(|u| u.permissions.can_list)
            };

            if !can_list {
                control_stream
                    .write_response(b"550 Permission denied\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let target_path = if let Some(path_arg) = path {
                match resolve_directory_path(&state.cwd, &state.home_dir, path_arg) {
                    Ok(path) => path,
                    Err(_) => {
                        control_stream
                            .write_response(b"550 Failed to resolve path\r\n", "FTP response")
                            .await;
                        return Ok(true);
                    }
                }
            } else {
                PathBuf::from(&state.cwd)
            };

            if matches!(cmd, MLST(_)) {
                if target_path.exists()
                    && path_starts_with_ignore_case(&target_path, &state.home_dir)
                {
                    if let Ok(metadata) = tokio::fs::metadata(&target_path).await {
                        let owner = state.current_user.as_deref().unwrap_or("anonymous");
                        let facts = transfer::build_mlst_facts(&metadata, owner);
                        match to_ftp_path(&target_path, std::path::Path::new(&state.home_dir)) {
                            Ok(ftp_path) => {
                                let name = target_path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| target_path.to_string_lossy().to_string());
                                control_stream
                                    .write_response(
                                        format!(
                                            "250-Listing {}\r\n {} {}\r\n250 End\r\n",
                                            ftp_path, facts, name
                                        )
                                        .as_bytes(),
                                        "FTP response",
                                    )
                                    .await;
                            }
                            Err(e) => {
                                tracing::error!("MLST failed: {}", e);
                                control_stream
                                    .write_response(
                                        b"550 Failed to get file path\r\n",
                                        "FTP response",
                                    )
                                    .await;
                            }
                        }
                    } else {
                        control_stream
                            .write_response(b"550 Failed to get file info\r\n", "FTP response")
                            .await;
                    }
                } else {
                    control_stream
                        .write_response(b"550 File not found\r\n", "FTP response")
                        .await;
                }
            } else {
                control_stream
                    .write_response(b"150 Here comes the directory listing\r\n", "FTP response")
                    .await;

                let mlst_owner = state
                    .current_user
                    .clone()
                    .unwrap_or_else(|| "anonymous".to_string());
                let mut transfer_ok = false;

                if let Ok(mut data_stream) = transfer::get_data_connection(
                    state.passive_mode,
                    state.data_port,
                    &state.data_addr,
                    ctx.client_ip,
                    &mut state.passive_manager,
                    state.data_protection,
                    ctx.tls_config.acceptor.as_deref(),
                )
                .await
                {
                    match transfer::send_mlsd_listing(&mut data_stream, &target_path, &mlst_owner)
                        .await
                    {
                        Ok(()) => transfer_ok = true,
                        Err(e) => tracing::warn!("MLSD transfer error: {}", e),
                    }
                }

                if state.passive_mode
                    && let Some(port) = state.data_port
                {
                    state.passive_manager.remove_listener(port);
                }
                state.data_port = None;
                state.data_addr = None;

                if transfer_ok {
                    control_stream
                        .write_response(b"226 Transfer complete\r\n", "FTP response")
                        .await;
                } else {
                    control_stream
                        .write_response(b"426 Transfer aborted\r\n", "FTP response")
                        .await;
                }
            }
        }

        _ => return Ok(true),
    }

    Ok(true)
}

pub async fn handle_retrieve_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    ctx: &super::session_auth::CommandContext<'_>,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    if let RETR(filename) = cmd {
        if !state.authenticated {
            control_stream
                .write_response(b"530 Not logged in\r\n", "FTP response")
                .await;
            return Ok(true);
        }

        if let Some(filename) = filename {
            let file_path = match state.resolve_path(filename) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("RETR failed for '{}': {}", filename, e);
                    control_stream
                        .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                        .await;
                    return Ok(true);
                }
            };

            if !path_starts_with_ignore_case(&file_path, &state.home_dir) {
                tracing::warn!(
                    "RETR denied: path='{}', home='{}', exists={}, is_file={}",
                    file_path.display(),
                    state.home_dir,
                    file_path.exists(),
                    file_path.is_file()
                );
                control_stream
                    .write_response(b"550 File not found\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let (can_read, speed_limit_kbps) = {
                let users = ctx.user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                let global_speed_limit = {
                    let cfg = ctx.config.lock();
                    if cfg.ftp.max_speed_kbps > 0 {
                        Some(cfg.ftp.max_speed_kbps)
                    } else {
                        None
                    }
                };
                (
                    user.is_none_or(|u| u.permissions.can_read),
                    user.and_then(|u| u.permissions.speed_limit_kbps)
                        .or(global_speed_limit),
                )
            };

            if !can_read {
                control_stream
                    .write_response(b"550 Permission denied\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let file_metadata = match tokio::fs::metadata(&file_path).await {
                Ok(m) => m,
                Err(e) => {
                    control_stream
                        .write_response(
                            format!("450 File unavailable: {}\r\n", e).as_bytes(),
                            "FTP response",
                        )
                        .await;
                    return Ok(true);
                }
            };

            let file_size = file_metadata.len();
            let remaining = if state.rest_offset > 0 && state.rest_offset < file_size {
                file_size - state.rest_offset
            } else {
                file_size
            };

            if state.rest_offset > 0 {
                control_stream
                    .write_response(
                        format!("110 Restart marker at {}\r\n", state.rest_offset).as_bytes(),
                        "FTP response",
                    )
                    .await;
            }

            control_stream
                .write_response(
                    format!(
                        "150 Opening BINARY mode data connection ({} bytes)\r\n",
                        remaining
                    )
                    .as_bytes(),
                    "RETR opening",
                )
                .await;

            let is_ascii = state.transfer_mode == "ascii";
            let rate_limiter: Option<std::sync::Arc<RateLimiter>> =
                speed_limit_kbps.map(|limit| std::sync::Arc::new(RateLimiter::new(limit)));
            let mut transfer_ok = false;

            if let Ok(mut data_stream) = transfer::get_data_connection(
                state.passive_mode,
                state.data_port,
                &state.data_addr,
                ctx.client_ip,
                &mut state.passive_manager,
                state.data_protection,
                ctx.tls_config.acceptor.as_deref(),
            )
            .await
            {
                let abort = Arc::clone(&state.abort_flag);
                match transfer::send_file_with_limits(
                    &mut data_stream,
                    &file_path,
                    state.rest_offset,
                    abort,
                    is_ascii,
                    rate_limiter.as_deref(),
                )
                .await
                {
                    Ok(()) => transfer_ok = true,
                    Err(e) => tracing::warn!("RETR transfer error: {}", e),
                }
            }

            if state.passive_mode
                && let Some(port) = state.data_port
            {
                state.passive_manager.remove_listener(port);
            }
            state.data_port = None;
            state.data_addr = None;

            if transfer_ok {
                control_stream
                    .write_response(b"226 Transfer complete\r\n", "FTP response")
                    .await;

                let final_size = tokio::fs::metadata(&file_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(remaining);
                crate::file_op_log!(
                    download,
                    state.current_user.as_deref().unwrap_or("anonymous"),
                    ctx.client_ip,
                    &file_path.to_string_lossy(),
                    final_size,
                    "FTP"
                );
            } else {
                control_stream
                    .write_response(b"426 Transfer aborted\r\n", "FTP response")
                    .await;
            }

            state.rest_offset = 0;
        }
    }

    Ok(true)
}

pub async fn handle_store_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    ctx: &super::session_auth::CommandContext<'_>,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    match cmd {
        STOR(filename) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            if let Some(filename) = filename {
                let (can_write, quota_mb, speed_limit_kbps) = {
                    let users = ctx.user_manager.lock();
                    let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                    let global_speed_limit = {
                        let cfg = ctx.config.lock();
                        if cfg.ftp.max_speed_kbps > 0 {
                            Some(cfg.ftp.max_speed_kbps)
                        } else {
                            None
                        }
                    };
                    (
                        user.is_some_and(|u| u.permissions.can_write),
                        user.and_then(|u| u.permissions.quota_mb),
                        user.and_then(|u| u.permissions.speed_limit_kbps)
                            .or(global_speed_limit),
                    )
                };

                if !can_write {
                    tracing::warn!(
                        "STOR denied: user {} lacks write permission",
                        state.current_user.as_deref().unwrap_or("unknown")
                    );
                    control_stream
                        .write_response(b"550 Permission denied\r\n", "FTP response")
                        .await;
                    return Ok(true);
                }

                let is_abs = filename.starts_with('/');
                tracing::debug!(
                    "STOR: raw_filename='{}', is_absolute={}, cwd='{}', home='{}', passive_mode={}, data_port={:?}",
                    filename,
                    is_abs,
                    state.cwd,
                    state.home_dir,
                    state.passive_mode,
                    state.data_port
                );

                let file_path = match state.resolve_path(filename) {
                    Ok(p) => {
                        tracing::debug!("STOR: resolved_path='{}'", p.display());
                        p
                    }
                    Err(e) => {
                        tracing::warn!("STOR failed for '{}': {}", filename, e);
                        control_stream
                            .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                            .await;
                        return Ok(true);
                    }
                };

                if !path_starts_with_ignore_case(&file_path, &state.home_dir) {
                    tracing::warn!(
                        "STOR denied: path outside home - {} (home: {})",
                        file_path.display(),
                        state.home_dir
                    );
                    control_stream
                        .write_response(b"550 Permission denied\r\n", "FTP response")
                        .await;
                    return Ok(true);
                }

                if let Some(quota) = quota_mb {
                    let current_usage = ctx
                        .quota_manager
                        .get_usage(state.current_user.as_deref().unwrap_or("anonymous"))
                        .await;
                    let quota_bytes = quota * 1024 * 1024;
                    if current_usage >= quota_bytes {
                        control_stream
                            .write_response(b"552 Quota exceeded\r\n", "FTP response")
                            .await;
                        tracing::warn!(
                            client_ip = %ctx.client_ip,
                            username = ?state.current_user.as_deref(),
                            action = "QUOTA_EXCEEDED",
                            "Upload denied: quota exceeded for user {}", state.current_user.as_deref().unwrap_or("unknown")
                        );
                        return Ok(true);
                    }
                }

                let file_existed = file_path.exists();
                control_stream
                    .write_response(
                        b"150 Opening BINARY mode data connection\r\n",
                        "FTP response",
                    )
                    .await;

                let mut transfer_success = false;
                let mut total_written: u64 = 0;
                let is_ascii = state.transfer_mode == "ascii";
                let rate_limiter: Option<std::sync::Arc<RateLimiter>> =
                    speed_limit_kbps.map(|limit| std::sync::Arc::new(RateLimiter::new(limit)));

                if let Ok(mut data_stream) = transfer::get_data_connection(
                    state.passive_mode,
                    state.data_port,
                    &state.data_addr,
                    ctx.client_ip,
                    &mut state.passive_manager,
                    state.data_protection,
                    ctx.tls_config.acceptor.as_deref(),
                )
                .await
                {
                    let abort = Arc::clone(&state.abort_flag);
                    let result = transfer::receive_file_with_limits(
                        &mut data_stream,
                        &file_path,
                        state.rest_offset,
                        abort,
                        is_ascii,
                        rate_limiter.as_deref(),
                    )
                    .await;
                    match result {
                        Ok(written) => {
                            transfer_success = true;
                            total_written = written;
                        }
                        Err(e) => {
                            tracing::error!("STOR transfer error: {}", e);
                        }
                    }
                } else {
                    tracing::error!(
                        "STOR failed to get data connection for file: {}",
                        file_path.display()
                    );
                }

                if state.passive_mode
                    && let Some(port) = state.data_port
                {
                    state.passive_manager.remove_listener(port);
                }
                state.data_port = None;
                state.data_addr = None;

                if transfer_success {
                    control_stream
                        .write_response(b"226 Transfer complete\r\n", "FTP response")
                        .await;

                    let uploaded_size = tokio::fs::metadata(&file_path)
                        .await
                        .map(|m: std::fs::Metadata| m.len())
                        .unwrap_or(total_written);

                    if quota_mb.is_some()
                        && let Err(e) = ctx
                            .quota_manager
                            .add_usage(
                                state.current_user.as_deref().unwrap_or("anonymous"),
                                uploaded_size,
                            )
                            .await
                    {
                        tracing::error!("Failed to update quota usage: {}", e);
                    }

                    if file_existed {
                        crate::file_op_log!(
                            update,
                            state.current_user.as_deref().unwrap_or("anonymous"),
                            ctx.client_ip,
                            &file_path.to_string_lossy(),
                            uploaded_size,
                            "FTP"
                        );
                    } else {
                        crate::file_op_log!(
                            upload,
                            state.current_user.as_deref().unwrap_or("anonymous"),
                            ctx.client_ip,
                            &file_path.to_string_lossy(),
                            uploaded_size,
                            "FTP"
                        );
                    }
                } else {
                    control_stream
                        .write_response(b"451 Transfer failed\r\n", "FTP response")
                        .await;
                    crate::file_op_log!(
                        failed,
                        state.current_user.as_deref().unwrap_or("anonymous"),
                        ctx.client_ip,
                        "UPLOAD",
                        &file_path.to_string_lossy(),
                        "FTP",
                        "Transfer failed"
                    );
                }

                state.rest_offset = 0;
            } else {
                control_stream
                    .write_response(
                        b"501 Syntax error: STOR requires filename\r\n",
                        "FTP response",
                    )
                    .await;
            }
        }

        APPE(filename) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            if let Some(filename) = filename {
                let can_append = {
                    let users = ctx.user_manager.lock();
                    let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                    user.is_some_and(|u| u.permissions.can_append)
                };

                if !can_append {
                    control_stream
                        .write_response(b"550 Permission denied\r\n", "FTP response")
                        .await;
                    return Ok(true);
                }

                let file_path = match state.resolve_path(filename) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("APPE failed for '{}': {}", filename, e);
                        control_stream
                            .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                            .await;
                        return Ok(true);
                    }
                };
                if !path_starts_with_ignore_case(&file_path, &state.home_dir) {
                    control_stream
                        .write_response(b"550 Permission denied\r\n", "FTP response")
                        .await;
                    return Ok(true);
                }
                control_stream
                    .write_response(
                        b"150 Opening BINARY mode data connection for append\r\n",
                        "FTP response",
                    )
                    .await;

                let is_ascii = state.transfer_mode == "ascii";
                let mut transfer_ok = false;

                if let Ok(mut data_stream) = transfer::get_data_connection(
                    state.passive_mode,
                    state.data_port,
                    &state.data_addr,
                    ctx.client_ip,
                    &mut state.passive_manager,
                    state.data_protection,
                    ctx.tls_config.acceptor.as_deref(),
                )
                .await
                {
                    let abort = Arc::clone(&state.abort_flag);
                    match transfer::receive_file_append(
                        &mut data_stream,
                        &file_path,
                        abort,
                        is_ascii,
                    )
                    .await
                    {
                        Ok(_) => transfer_ok = true,
                        Err(e) => tracing::warn!("APPE transfer error: {}", e),
                    }
                }

                if state.passive_mode
                    && let Some(port) = state.data_port
                {
                    state.passive_manager.remove_listener(port);
                }
                state.data_port = None;
                state.data_addr = None;

                if transfer_ok {
                    control_stream
                        .write_response(b"226 Transfer complete\r\n", "FTP response")
                        .await;

                    let appended_size = tokio::fs::metadata(&file_path)
                        .await
                        .map(|m| m.len())
                        .unwrap_or(0);
                    crate::file_op_log!(
                        state.current_user.as_deref().unwrap_or("anonymous"),
                        ctx.client_ip,
                        "APPEND",
                        &file_path.to_string_lossy(),
                        appended_size,
                        "FTP",
                        true,
                        "File append successful"
                    );
                } else {
                    control_stream
                        .write_response(b"426 Transfer aborted\r\n", "FTP response")
                        .await;
                }
            }
        }

        STOU => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let can_write = {
                let users = ctx.user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_write)
            };

            if !can_write {
                control_stream
                    .write_response(b"550 Permission denied\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let file_path = match super::session_dirs::generate_unique_filename(state, 100).await {
                Ok(path) => path,
                Err(e) => {
                    tracing::warn!("STOU failed: {}", e);
                    control_stream
                        .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                        .await;
                    return Ok(true);
                }
            };

            let unique_name = file_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            control_stream
                .write_response(
                    format!("150 FILE: {}\r\n", unique_name).as_bytes(),
                    "FTP response",
                )
                .await;

            let is_ascii = state.transfer_mode == "ascii";
            let mut transfer_ok = false;

            if let Ok(mut data_stream) = transfer::get_data_connection(
                state.passive_mode,
                state.data_port,
                &state.data_addr,
                ctx.client_ip,
                &mut state.passive_manager,
                state.data_protection,
                ctx.tls_config.acceptor.as_deref(),
            )
            .await
            {
                let abort = Arc::clone(&state.abort_flag);
                match transfer::receive_file(&mut data_stream, &file_path, 0, abort, is_ascii).await
                {
                    Ok(_) => transfer_ok = true,
                    Err(e) => tracing::warn!("STOU transfer error: {}", e),
                }
            }

            if state.passive_mode
                && let Some(port) = state.data_port
            {
                state.passive_manager.remove_listener(port);
            }
            state.data_port = None;
            state.data_addr = None;

            if transfer_ok {
                control_stream
                    .write_response(b"226 Transfer complete\r\n", "FTP response")
                    .await;

                let uploaded_size = tokio::fs::metadata(&file_path)
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                crate::file_op_log!(
                    upload,
                    state.current_user.as_deref().unwrap_or("anonymous"),
                    ctx.client_ip,
                    &file_path.to_string_lossy(),
                    uploaded_size,
                    "FTP"
                );
            } else {
                control_stream
                    .write_response(b"426 Transfer aborted\r\n", "FTP response")
                    .await;
            }
        }

        _ => return Ok(true),
    }

    Ok(true)
}

pub async fn handle_fileinfo_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    _ctx: &super::session_auth::CommandContext<'_>,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    match cmd {
        SIZE(filename) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }
            if let Some(filename) = filename {
                let file_path = match state.resolve_path(filename) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("SIZE failed for '{}': {}", filename, e);
                        control_stream
                            .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                            .await;
                        return Ok(true);
                    }
                };
                if path_starts_with_ignore_case(&file_path, &state.home_dir) {
                    if let Ok(metadata) = tokio::fs::metadata(&file_path).await {
                        control_stream
                            .write_response(
                                format!("213 {}\r\n", metadata.len()).as_bytes(),
                                "FTP response",
                            )
                            .await;
                    } else {
                        control_stream
                            .write_response(
                                b"450 File unavailable: file not found\r\n",
                                "FTP response",
                            )
                            .await;
                    }
                } else {
                    control_stream
                        .write_response(b"550 Permission denied\r\n", "FTP response")
                        .await;
                }
            }
        }

        MDTM(filename) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }
            if let Some(filename) = filename {
                let file_path = match state.resolve_path(filename) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("MDTM failed for '{}': {}", filename, e);
                        control_stream
                            .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                            .await;
                        return Ok(true);
                    }
                };
                if path_starts_with_ignore_case(&file_path, &state.home_dir) {
                    if let Ok(metadata) = tokio::fs::metadata(&file_path).await {
                        let mtime = transfer::get_file_mtime_raw(&metadata);
                        control_stream
                            .write_response(format!("213 {}\r\n", mtime).as_bytes(), "FTP response")
                            .await;
                    } else {
                        control_stream
                            .write_response(
                                b"450 File unavailable: file not found\r\n",
                                "FTP response",
                            )
                            .await;
                    }
                } else {
                    control_stream
                        .write_response(b"550 Permission denied\r\n", "FTP response")
                        .await;
                }
            }
        }

        _ => return Ok(true),
    }

    Ok(true)
}
