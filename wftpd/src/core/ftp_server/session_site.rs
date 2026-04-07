//! SITE 命令处理
//!
//! 处理 FTP SITE 命令，支持自定义站点操作

use crate::core::path_utils::path_starts_with_ignore_case;
use anyhow::Result;

use super::commands::FtpCommand;
use super::session_state::{ControlStream, SessionState};

pub async fn handle_site_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    ctx: &super::session_auth::CommandContext<'_>,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    if let SITE(site_cmd) = cmd {
        if let Some(site_cmd) = site_cmd {
            let site_parts: Vec<&str> = site_cmd.splitn(2, ' ').collect();
            let site_action = site_parts[0].to_uppercase();
            let site_arg = site_parts.get(1).map(|s| s.trim());

            match site_action.as_str() {
                "HELP" => {
                    control_stream
                        .write_response(
                            b"214-The following SITE commands are recognized:\r\n",
                            "FTP response",
                        )
                        .await;
                    control_stream
                        .write_response(
                            b"214-CHMOD IDLE HELP WHO WHOIS SYMLINK\r\n",
                            "FTP response",
                        )
                        .await;
                    control_stream
                        .write_response(b"214 End\r\n", "FTP response")
                        .await;
                }
                "IDLE" => {
                    if let Some(secs_str) = site_arg {
                        if let Ok(secs) = secs_str.parse::<u64>() {
                            control_stream
                                .write_response(
                                    format!("200 Idle timeout set to {} seconds\r\n", secs)
                                        .as_bytes(),
                                    "FTP response",
                                )
                                .await;
                        } else {
                            control_stream
                                .write_response(b"501 Invalid idle time\r\n", "FTP response")
                                .await;
                        }
                    } else {
                        control_stream
                            .write_response(
                                b"501 SITE IDLE requires time parameter\r\n",
                                "FTP response",
                            )
                            .await;
                    }
                }
                "WHO" | "WHOIS" => {
                    if !state.authenticated {
                        control_stream
                            .write_response(b"530 Not logged in\r\n", "FTP response")
                            .await;
                        return Ok(true);
                    }

                    if let Some(username) = &state.current_user {
                        let user_info = {
                            let users = ctx.user_manager.lock();
                            users.get_user(username).map(|u| {
                                format!(
                                    "User: {}\r\nHome: {}\r\nPermissions: {}{}{}{}{}{}",
                                    u.username,
                                    u.home_dir,
                                    if u.permissions.can_read { "R" } else { "-" },
                                    if u.permissions.can_write { "W" } else { "-" },
                                    if u.permissions.can_delete { "D" } else { "-" },
                                    if u.permissions.can_list { "L" } else { "-" },
                                    if u.permissions.can_mkdir { "C" } else { "-" },
                                    if u.permissions.can_rename { "M" } else { "-" },
                                )
                            })
                        };

                        if let Some(info) = user_info {
                            control_stream
                                .write_response(
                                    format!(
                                        "200-User information for {}:\r\n200 {}\r\n",
                                        username, info
                                    )
                                    .as_bytes(),
                                    "FTP response",
                                )
                                .await;
                        } else {
                            control_stream
                                .write_response(b"550 User not found\r\n", "FTP response")
                                .await;
                        }
                    } else {
                        control_stream
                            .write_response(b"550 Not authenticated\r\n", "FTP response")
                            .await;
                    }
                }
                "SYMLINK" => {
                    if !state.authenticated {
                        control_stream
                            .write_response(b"530 Not logged in\r\n", "FTP response")
                            .await;
                        return Ok(true);
                    }

                    let can_write = if state.current_user.as_deref() == Some("anonymous") {
                        false
                    } else {
                        let users = ctx.user_manager.lock();
                        state
                            .current_user
                            .as_ref()
                            .and_then(|u| users.get_user(u))
                            .is_some_and(|u| u.permissions.can_write)
                    };

                    if !can_write {
                        control_stream
                            .write_response(b"550 Permission denied\r\n", "FTP response")
                            .await;
                        return Ok(true);
                    }

                    if let Some(symlink_args) = site_arg {
                        let symlink_parts: Vec<&str> = symlink_args.splitn(2, ' ').collect();
                        if symlink_parts.len() == 2 {
                            let target = symlink_parts[0];
                            let link_name = symlink_parts[1];

                            let link_path = match state.resolve_path(link_name) {
                                Ok(p) => p,
                                Err(e) => {
                                    control_stream
                                        .write_response(
                                            format!("550 {}\r\n", e).as_bytes(),
                                            "FTP response",
                                        )
                                        .await;
                                    return Ok(true);
                                }
                            };

                            if !path_starts_with_ignore_case(&link_path, &state.home_dir) {
                                control_stream
                                    .write_response(b"550 Permission denied\r\n", "FTP response")
                                    .await;
                                return Ok(true);
                            }

                            #[cfg(windows)]
                            {
                                use std::os::windows::fs::symlink_file;

                                match symlink_file(target, &link_path) {
                                    Ok(()) => {
                                        control_stream
                                            .write_response(
                                                format!(
                                                    "200 Symbolic link created: {} -> {}\r\n",
                                                    link_name, target
                                                )
                                                .as_bytes(),
                                                "FTP response",
                                            )
                                            .await;
                                        tracing::info!(
                                            client_ip = %ctx.client_ip,
                                            username = ?state.current_user.as_deref(),
                                            action = "SITE SYMLINK",
                                            protocol = "FTP",
                                            "Created symlink: {} -> {}",
                                            link_path.display(),
                                            target
                                        );
                                    }
                                    Err(e) => {
                                        control_stream
                                            .write_response(
                                                format!("550 Failed to create symlink: {}\r\n", e)
                                                    .as_bytes(),
                                                "FTP response",
                                            )
                                            .await;
                                        tracing::warn!(
                                            client_ip = %ctx.client_ip,
                                            action = "SITE SYMLINK",
                                            "Failed to create symlink: {}",
                                            e
                                        );
                                    }
                                }
                            }

                            #[cfg(not(windows))]
                            {
                                use std::os::unix::fs::symlink;

                                match symlink(target, &link_path) {
                                    Ok(()) => {
                                        control_stream
                                            .write_response(
                                                format!(
                                                    "200 Symbolic link created: {} -> {}\r\n",
                                                    link_name, target
                                                )
                                                .as_bytes(),
                                                "FTP response",
                                            )
                                            .await;
                                    }
                                    Err(e) => {
                                        control_stream
                                            .write_response(
                                                format!("550 Failed to create symlink: {}\r\n", e)
                                                    .as_bytes(),
                                                "FTP response",
                                            )
                                            .await;
                                    }
                                }
                            }
                        } else {
                            control_stream
                                .write_response(
                                    b"501 SITE SYMLINK requires target and link_name\r\n",
                                    "FTP response",
                                )
                                .await;
                        }
                    } else {
                        control_stream
                            .write_response(
                                b"501 SITE SYMLINK requires parameters\r\n",
                                "FTP response",
                            )
                            .await;
                    }
                }
                "CHMOD" => {
                    if !state.authenticated {
                        control_stream
                            .write_response(b"530 Not logged in\r\n", "FTP response")
                            .await;
                        return Ok(true);
                    }

                    if let Some(chmod_args) = site_arg {
                        let chmod_parts: Vec<&str> = chmod_args.splitn(2, ' ').collect();
                        if chmod_parts.len() == 2 {
                            let mode = chmod_parts[0];
                            let target = chmod_parts[1];

                            let target_path = match state.resolve_path(target) {
                                Ok(p) => p,
                                Err(e) => {
                                    control_stream
                                        .write_response(
                                            format!("550 {}\r\n", e).as_bytes(),
                                            "FTP response",
                                        )
                                        .await;
                                    return Ok(true);
                                }
                            };

                            if !path_starts_with_ignore_case(&target_path, &state.home_dir) {
                                control_stream
                                    .write_response(b"550 Permission denied\r\n", "FTP response")
                                    .await;
                                return Ok(true);
                            }

                            if let Ok(_mode_val) = u32::from_str_radix(mode, 8) {
                                #[cfg(windows)]
                                {
                                    control_stream.write_response(b"200 CHMOD command accepted (Windows: permissions managed by ACL)\r\n", "FTP response").await;
                                }
                                #[cfg(not(windows))]
                                {
                                    use std::os::unix::fs::PermissionsExt;
                                    match std::fs::set_permissions(
                                        &target_path,
                                        std::fs::Permissions::from_mode(mode_val),
                                    ) {
                                        Ok(()) => {
                                            control_stream
                                                .write_response(
                                                    format!("200 CHMOD {} {}\r\n", mode, target)
                                                        .as_bytes(),
                                                    "FTP response",
                                                )
                                                .await;
                                        }
                                        Err(e) => {
                                            control_stream
                                                .write_response(
                                                    format!("550 CHMOD failed: {}\r\n", e)
                                                        .as_bytes(),
                                                    "FTP response",
                                                )
                                                .await;
                                        }
                                    }
                                }
                            } else {
                                control_stream
                                    .write_response(b"501 Invalid mode format\r\n", "FTP response")
                                    .await;
                            }
                        } else {
                            control_stream
                                .write_response(
                                    b"501 SITE CHMOD requires mode and filename\r\n",
                                    "FTP response",
                                )
                                .await;
                        }
                    } else {
                        control_stream
                            .write_response(
                                b"501 SITE CHMOD requires parameters\r\n",
                                "FTP response",
                            )
                            .await;
                    }
                }
                _ => {
                    control_stream
                        .write_response(
                            format!("500 Unknown SITE command: {}\r\n", site_action).as_bytes(),
                            "FTP response",
                        )
                        .await;
                }
            }
        } else {
            control_stream
                .write_response(b"501 SITE command requires parameter\r\n", "FTP response")
                .await;
        }
    }

    Ok(true)
}
