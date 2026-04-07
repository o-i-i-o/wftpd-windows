//! FTP 认证命令处理
//!
//! 处理 USER、PASS、AUTH 等认证相关命令

use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::Mutex;
use anyhow::Result;

use crate::core::config::Config;
use crate::core::quota::QuotaManager;
use crate::core::users::UserManager;
use crate::core::fail2ban::Fail2BanManager;

use super::commands::FtpCommand;
use super::tls::TlsConfig;
use super::session_state::{ControlStream, SessionState};

pub struct CommandContext<'a> {
    pub config: &'a Arc<Mutex<Config>>,
    pub user_manager: &'a Arc<Mutex<UserManager>>,
    pub quota_manager: &'a Arc<QuotaManager>,
    pub fail2ban_manager: &'a Arc<Fail2BanManager>,
    pub client_ip: &'a str,
    pub allow_anonymous: &'a bool,
    pub anonymous_home: &'a Option<String>,
    pub tls_config: &'a TlsConfig,
    pub require_ssl: bool,
}

pub async fn handle_auth_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    ctx: &CommandContext<'_>,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    match cmd {
        AUTH(tls_type) => {
            let ftps_enabled = {
                let cfg = ctx.config.lock();
                cfg.ftp.ftps.enabled
            };

            if !ftps_enabled {
                control_stream
                    .write_response(b"502 FTPS is disabled on this server\r\n", "FTP response")
                    .await;
                return Ok(false);
            }

            let tls_type = tls_type.as_deref().unwrap_or("TLS");
            let tls_upper = tls_type.to_uppercase();

            if ctx.tls_config.is_tls_available() {
                if tls_upper == "TLS" || tls_upper == "TLS-C" || tls_upper == "SSL" {
                    control_stream
                        .write_response(
                            b"234 AUTH command OK; starting TLS connection\r\n",
                            "FTP response",
                        )
                        .await;

                    if let Some(acceptor) = &ctx.tls_config.acceptor {
                        match control_stream.upgrade_to_tls(acceptor).await {
                            Ok(()) => {
                                state.tls_enabled = true;
                                tracing::info!("TLS connection established for {}", ctx.client_ip);
                            }
                            Err(e) => {
                                tracing::error!("TLS upgrade failed: {}", e);
                                control_stream
                                    .write_response(
                                        b"431 Unable to negotiate TLS connection\r\n",
                                        "FTP response",
                                    )
                                    .await;
                            }
                        }
                    }
                } else {
                    control_stream
                        .write_response(
                            format!("504 AUTH {} not supported\r\n", tls_type).as_bytes(),
                            "FTP response",
                        )
                        .await;
                }
            } else {
                control_stream
                    .write_response(b"502 TLS not configured on server\r\n", "FTP response")
                    .await;
            }
        }

        PBSZ(size) => {
            if state.tls_enabled {
                if let Some(size_str) = size {
                    if let Ok(size_val) = size_str.parse::<u64>() {
                        state.pbsz_set = true;
                        control_stream
                            .write_response(
                                format!("200 PBSZ={} OK\r\n", size_val).as_bytes(),
                                "FTP response",
                            )
                            .await;
                    } else {
                        control_stream
                            .write_response(b"501 Invalid PBSZ value\r\n", "FTP response")
                            .await;
                    }
                } else {
                    state.pbsz_set = true;
                    control_stream
                        .write_response(b"200 PBSZ=0 OK\r\n", "FTP response")
                        .await;
                }
            } else {
                control_stream
                    .write_response(b"503 PBSZ requires AUTH first\r\n", "FTP response")
                    .await;
            }
        }

        PROT(level) => {
            if state.tls_enabled && state.pbsz_set {
                if let Some(level) = level {
                    match level.to_uppercase().as_str() {
                        "P" => {
                            state.data_protection = true;
                            control_stream
                                .write_response(b"200 PROT Private OK\r\n", "FTP response")
                                .await;
                        }
                        "C" => {
                            state.data_protection = false;
                            control_stream
                                .write_response(b"200 PROT Clear OK\r\n", "FTP response")
                                .await;
                        }
                        "S" => {
                            control_stream
                                .write_response(b"536 PROT Safe not supported\r\n", "FTP response")
                                .await;
                        }
                        "E" => {
                            control_stream
                                .write_response(
                                    b"536 PROT Confidential not supported\r\n",
                                    "FTP response",
                                )
                                .await;
                        }
                        _ => {
                            control_stream
                                .write_response(b"504 Unknown PROT level\r\n", "FTP response")
                                .await;
                        }
                    }
                } else {
                    control_stream
                        .write_response(
                            b"501 PROT requires parameter (C/P/S/E)\r\n",
                            "FTP response",
                        )
                        .await;
                }
            } else {
                control_stream
                    .write_response(b"503 PROT requires PBSZ first\r\n", "FTP response")
                    .await;
            }
        }

        CCC => {
            if state.tls_enabled {
                control_stream
                    .write_response(b"200 CCC OK - reverting to clear text\r\n", "FTP response")
                    .await;
            } else {
                control_stream
                    .write_response(
                        b"533 CCC not available - not in TLS mode\r\n",
                        "FTP response",
                    )
                    .await;
            }
        }

        ADAT(_data) => {
            if state.tls_enabled {
                tracing::debug!(
                    "ADAT received but not implemented (TLS already provides security)"
                );
                control_stream
                    .write_response(
                        b"504 ADAT not implemented - TLS provides security\r\n",
                        "FTP response",
                    )
                    .await;
            } else {
                control_stream
                    .write_response(b"503 ADAT requires AUTH first\r\n", "FTP response")
                    .await;
            }
        }

        MIC(data) => {
            if state.tls_enabled {
                if let Some(data) = data {
                    tracing::debug!(
                        "MIC command received: {} (TLS already provides integrity)",
                        data
                    );
                    control_stream
                        .write_response(
                            b"200 MIC accepted - integrity provided by TLS\r\n",
                            "FTP response",
                        )
                        .await;
                } else {
                    control_stream
                        .write_response(b"501 MIC requires data parameter\r\n", "FTP response")
                        .await;
                }
            } else {
                control_stream
                    .write_response(b"503 MIC requires AUTH first\r\n", "FTP response")
                    .await;
            }
        }

        CONF(data) => {
            if state.tls_enabled {
                if let Some(data) = data {
                    tracing::debug!(
                        "CONF command received: {} (TLS already provides confidentiality)",
                        data
                    );
                    control_stream
                        .write_response(
                            b"200 CONF accepted - confidentiality provided by TLS\r\n",
                            "FTP response",
                        )
                        .await;
                } else {
                    control_stream
                        .write_response(b"501 CONF requires data parameter\r\n", "FTP response")
                        .await;
                }
            } else {
                control_stream
                    .write_response(b"503 CONF requires AUTH first\r\n", "FTP response")
                    .await;
            }
        }

        ENC(data) => {
            if state.tls_enabled {
                if let Some(data) = data {
                    tracing::debug!(
                        "ENC command received: {} (TLS already provides encryption)",
                        data
                    );
                    control_stream
                        .write_response(
                            b"200 ENC accepted - encryption provided by TLS\r\n",
                            "FTP response",
                        )
                        .await;
                } else {
                    control_stream
                        .write_response(b"501 ENC requires data parameter\r\n", "FTP response")
                        .await;
                }
            } else {
                control_stream
                    .write_response(b"503 ENC requires AUTH first\r\n", "FTP response")
                    .await;
            }
        }

        USER(username) => {
            if ctx.require_ssl && !state.tls_enabled {
                control_stream
                    .write_response(b"530 SSL required for login\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let username_lower = username.to_lowercase();
            if username_lower == "anonymous" || username_lower == "ftp" {
                if *ctx.allow_anonymous {
                    state.current_user = Some("anonymous".to_string());
                    control_stream
                        .write_response(
                            b"331 Anonymous login okay, send email as password\r\n",
                            "FTP response",
                        )
                        .await;
                } else {
                    control_stream
                        .write_response(b"530 Anonymous access not allowed\r\n", "FTP response")
                        .await;
                }
            } else {
                state.current_user = Some(username.to_string());
                control_stream
                    .write_response(b"331 User name okay, need password\r\n", "FTP response")
                    .await;
            }
        }

        PASS(password) => {
            if ctx.require_ssl && !state.tls_enabled {
                control_stream
                    .write_response(b"530 SSL required for login\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            if let Some(ref username) = state.current_user {
                if username == "anonymous" {
                    if *ctx.allow_anonymous {
                        if let Some(anon_home) = ctx.anonymous_home {
                            match PathBuf::from(anon_home).canonicalize() {
                                Ok(home_canon) => {
                                    state.cwd = home_canon.to_string_lossy().to_string();
                                    state.home_dir = state.cwd.clone();
                                    state.authenticated = true;
                                    control_stream
                                        .write_response(
                                            b"230 Anonymous user logged in\r\n",
                                            "FTP response",
                                        )
                                        .await;
                                    tracing::info!(
                                        client_ip = %ctx.client_ip,
                                        username = "anonymous",
                                        action = "LOGIN",
                                        protocol = "FTP",
                                        "Anonymous user logged in"
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "PASS failed: cannot canonicalize anonymous home directory '{}': {}",
                                        anon_home,
                                        e
                                    );
                                    control_stream
                                        .write_response(
                                            b"550 Anonymous home directory not found\r\n",
                                            "FTP response",
                                        )
                                        .await;
                                    state.current_user = None;
                                }
                            }
                        } else {
                            tracing::error!(
                                "PASS failed: anonymous access allowed but no anonymous_home configured"
                            );
                            control_stream
                                .write_response(
                                    b"530 Anonymous home directory not configured\r\n",
                                    "FTP response",
                                )
                                .await;
                            state.current_user = None;
                        }
                    } else {
                        control_stream
                            .write_response(b"530 Anonymous access not allowed\r\n", "FTP response")
                            .await;
                    }
                } else {
                    let password = password.as_deref().unwrap_or("");
                    let (auth_result, home_dir_opt) = {
                        let mut users = ctx.user_manager.lock();
                        if users.get_user(username).is_none() {
                            let _ = users.reload(&Config::get_users_path());
                        }
                        let result = users.authenticate(username, password);
                        let home = users.get_user(username).map(|u| u.home_dir.clone());
                        (result, home)
                    };

                    match auth_result {
                        Ok(true) => {
                            ctx.fail2ban_manager.reset_failures(ctx.client_ip).await;

                            state.authenticated = true;
                            if let Some(home_dir) = home_dir_opt {
                                match PathBuf::from(&home_dir).canonicalize() {
                                    Ok(home_canon) => {
                                        state.cwd = home_canon.to_string_lossy().to_string();
                                        state.home_dir = state.cwd.clone();
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "PASS failed: cannot canonicalize user home directory '{}': {}",
                                            home_dir,
                                            e
                                        );
                                        control_stream
                                            .write_response(
                                                b"550 Home directory not found\r\n",
                                                "FTP response",
                                            )
                                            .await;
                                        state.authenticated = false;
                                        state.current_user = None;
                                        return Ok(true);
                                    }
                                }
                            }
                            control_stream
                                .write_response(b"230 User logged in\r\n", "FTP response")
                                .await;
                            tracing::info!(
                                client_ip = %ctx.client_ip,
                                username = %username,
                                action = "LOGIN",
                                protocol = "FTP",
                                "User {} logged in", username
                            );
                        }
                        Ok(false) => {
                            ctx.fail2ban_manager.add_failure(ctx.client_ip).await;

                            tracing::warn!(
                                client_ip = %ctx.client_ip,
                                username = %username,
                                action = "AUTH_FAIL",
                                protocol = "FTP",
                                "Authentication failed for user {}", username
                            );
                            control_stream
                                .write_response(
                                    b"530 Not logged in, user cannot be authenticated\r\n",
                                    "FTP response",
                                )
                                .await;
                        }
                        Err(e) => {
                            ctx.fail2ban_manager.add_failure(ctx.client_ip).await;

                            tracing::error!(
                                client_ip = %ctx.client_ip,
                                username = %username,
                                action = "AUTH_ERROR",
                                "Authentication error for user {}: {}", username, e
                            );
                            control_stream
                                .write_response(b"530 Not logged in\r\n", "FTP response")
                                .await;
                        }
                    }
                }
            } else {
                control_stream
                    .write_response(b"530 Please login with USER and PASS\r\n", "FTP response")
                    .await;
            }
        }

        _ => return Ok(true),
    }

    Ok(true)
}
