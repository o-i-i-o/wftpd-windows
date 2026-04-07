use std::path::PathBuf;

use crate::core::path_utils::{path_starts_with_ignore_case, to_ftp_path};

use super::commands::FtpCommand;
use super::session_state::{ControlStream, SessionState};


pub async fn handle_directory_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    ctx: &super::session_auth::CommandContext<'_>,
) -> anyhow::Result<bool> {
    use super::commands::FtpCommand::*;

    match cmd {
        PWD | XPWD => {
            match to_ftp_path(
                std::path::Path::new(&state.cwd),
                std::path::Path::new(&state.home_dir),
            ) {
                Ok(ftp_path) => {
                    control_stream
                        .write_response(
                            format!("257 \"{}\"\r\n", ftp_path).as_bytes(),
                            "FTP response",
                        )
                        .await;
                }
                Err(e) => {
                    tracing::error!("PWD failed: {}", e);
                    control_stream
                        .write_response(b"550 Failed to get current directory\r\n", "FTP response")
                        .await;
                }
            }
        }

        CWD(dir) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }
            if let Some(dir) = dir {
                match state.resolve_path(dir) {
                    Ok(new_path) => {
                        if new_path.exists()
                            && new_path.is_dir()
                            && path_starts_with_ignore_case(&new_path, &state.home_dir)
                        {
                            state.cwd = new_path.to_string_lossy().to_string();
                            control_stream
                                .write_response(
                                    b"250 Directory successfully changed\r\n",
                                    "FTP response",
                                )
                                .await;
                        } else {
                            control_stream
                                .write_response(
                                    b"550 Failed to change directory\r\n",
                                    "FTP response",
                                )
                                .await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("CWD failed for '{}': {}", dir, e);
                        control_stream
                            .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                            .await;
                    }
                }
            } else {
                control_stream
                    .write_response(
                        b"501 Syntax error: CWD requires directory parameter\r\n",
                        "FTP response",
                    )
                    .await;
            }
        }

        CDUP | XCUP => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }
            match state.resolve_path("..") {
                Ok(new_path) => {
                    if path_starts_with_ignore_case(&new_path, &state.home_dir) && new_path.exists()
                    {
                        state.cwd = new_path.to_string_lossy().to_string();
                        control_stream
                            .write_response(b"250 Directory changed\r\n", "FTP response")
                            .await;
                    } else {
                        control_stream
                            .write_response(
                                b"550 Cannot change to parent directory: Permission denied\r\n",
                                "FTP response",
                            )
                            .await;
                    }
                }
                Err(e) => {
                    tracing::warn!("CDUP failed: {}", e);
                    control_stream
                        .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                        .await;
                }
            }
        }

        MKD(dirname) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let can_mkdir = {
                let users = ctx.user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_mkdir)
            };

            if !can_mkdir {
                control_stream
                    .write_response(b"550 Permission denied\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            if let Some(dirname) = dirname {
                let dir_path = match state.resolve_path(dirname) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("MKD failed for '{}': {}", dirname, e);
                        control_stream
                            .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                            .await;
                        return Ok(true);
                    }
                };
                if !path_starts_with_ignore_case(&dir_path, &state.home_dir) {
                    control_stream
                        .write_response(b"550 Permission denied\r\n", "FTP response")
                        .await;
                    return Ok(true);
                }
                if tokio::fs::create_dir_all(&dir_path).await.is_ok() {
                    match to_ftp_path(&dir_path, std::path::Path::new(&state.home_dir)) {
                        Ok(ftp_path) => {
                            control_stream
                                .write_response(
                                    format!("257 \"{}\" created\r\n", ftp_path).as_bytes(),
                                    "FTP response",
                                )
                                .await;
                        }
                        Err(e) => {
                            tracing::error!("MKD failed to get ftp path: {}", e);
                            control_stream
                                .write_response(b"257 Directory created\r\n", "FTP response")
                                .await;
                        }
                    }
                    crate::file_op_log!(
                        mkdir,
                        state.current_user.as_deref().unwrap_or("anonymous"),
                        ctx.client_ip,
                        &dir_path.to_string_lossy(),
                        "FTP"
                    );
                } else {
                    control_stream
                        .write_response(
                            b"550 Create directory operation failed\r\n",
                            "FTP response",
                        )
                        .await;
                }
            }
        }

        RMD(dirname) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let can_rmdir = {
                let users = ctx.user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_rmdir)
            };

            if !can_rmdir {
                control_stream
                    .write_response(b"550 Permission denied\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            if let Some(dirname) = dirname {
                let dir_path = match state.resolve_path(dirname) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("RMD failed for '{}': {}", dirname, e);
                        control_stream
                            .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                            .await;
                        return Ok(true);
                    }
                };
                if !path_starts_with_ignore_case(&dir_path, &state.home_dir) {
                    control_stream
                        .write_response(b"550 Permission denied\r\n", "FTP response")
                        .await;
                    return Ok(true);
                }

                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if let Ok(metadata) = tokio::fs::symlink_metadata(&dir_path).await {
                        let file_attrs = metadata.file_attributes();
                        if file_attrs & 0x400 != 0 {
                            tracing::warn!(
                                "RMD denied: path is a junction/reparse point - {:?}",
                                dir_path
                            );
                            control_stream
                                .write_response(b"550 Junction points not allowed\r\n", "FTP response")
                                .await;
                            return Ok(true);
                        }
                    }
                }

                let is_symlink = dir_path.is_symlink();

                let result = if is_symlink {
                    std::fs::remove_dir(&dir_path)
                } else {
                    tokio::fs::remove_dir_all(&dir_path).await
                };

                if result.is_ok() {
                    control_stream
                        .write_response(b"250 Directory removed\r\n", "FTP response")
                        .await;
                    crate::file_op_log!(
                        rmdir,
                        state.current_user.as_deref().unwrap_or("anonymous"),
                        ctx.client_ip,
                        &dir_path.to_string_lossy(),
                        "FTP"
                    );
                } else {
                    control_stream
                        .write_response(
                            b"550 Remove directory operation failed\r\n",
                            "FTP response",
                        )
                        .await;
                }
            }
        }

        RNFR(from_name) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let can_rename = {
                let users = ctx.user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_rename)
            };

            if !can_rename {
                control_stream
                    .write_response(b"550 Permission denied\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            if let Some(from_name) = from_name {
                let from_path = match state.resolve_path(from_name) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("RNFR failed for '{}': {}", from_name, e);
                        control_stream
                            .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                            .await;
                        return Ok(true);
                    }
                };
                tracing::debug!(
                    "RNFR: raw='{}', resolved='{}', exists={}, starts_with={}",
                    from_name,
                    from_path.display(),
                    from_path.exists(),
                    path_starts_with_ignore_case(&from_path, &state.home_dir)
                );
                if from_path.exists() && path_starts_with_ignore_case(&from_path, &state.home_dir) {
                    state.rename_from = Some(from_path.to_string_lossy().to_string());
                    control_stream
                        .write_response(
                            b"350 File exists, ready for destination name\r\n",
                            "FTP response",
                        )
                        .await;
                    tracing::debug!(
                        client_ip = %ctx.client_ip,
                        username = ?state.current_user.as_deref(),
                        action = "RNFR",
                        "RNFR: {}", from_path.display()
                    );
                } else {
                    tracing::warn!(
                        "RNFR failed: file not found or outside home - raw='{}', resolved='{}'",
                        from_name,
                        from_path.display()
                    );
                    control_stream
                        .write_response(b"450 File unavailable: file not found\r\n", "FTP response")
                        .await;
                }
            } else {
                control_stream
                    .write_response(
                        b"501 Syntax error: RNFR requires filename\r\n",
                        "FTP response",
                    )
                    .await;
            }
        }

        RNTO(to_name) => {
            if let Some(ref from_path) = state.rename_from {
                if let Some(to_name) = to_name {
                    let to_path = match state.resolve_path(to_name) {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::warn!("RNTO failed for '{}': {}", to_name, e);
                            control_stream
                                .write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response")
                                .await;
                            state.rename_from = None;
                            return Ok(true);
                        }
                    };
                    tracing::debug!(
                        "RNTO: raw='{}', resolved='{}', from='{}'",
                        to_name,
                        to_path.display(),
                        from_path
                    );
                    if !path_starts_with_ignore_case(&to_path, &state.home_dir) {
                        tracing::warn!(
                            "RNTO failed: destination outside home - {}",
                            to_path.display()
                        );
                        control_stream
                            .write_response(b"550 Permission denied\r\n", "FTP response")
                            .await;
                        state.rename_from = None;
                        return Ok(true);
                    }
                    if to_path.exists() {
                        tracing::warn!(
                            "RNTO failed: destination already exists - {}",
                            to_path.display()
                        );
                        control_stream
                            .write_response(b"550 Destination file already exists\r\n", "FTP response")
                            .await;
                        state.rename_from = None;
                        return Ok(true);
                    }
                    let from_path_buf = PathBuf::from(from_path);
                    match tokio::fs::rename(&from_path_buf, &to_path).await {
                        Ok(()) => {
                            control_stream
                                .write_response(b"250 Rename successful\r\n", "FTP response")
                                .await;
                            let from_parent = from_path_buf
                                .parent()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let to_parent = to_path
                                .parent()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default();
                            if from_parent == to_parent {
                                crate::file_op_log!(
                                    rename,
                                    state.current_user.as_deref().unwrap_or("anonymous"),
                                    ctx.client_ip,
                                    from_path,
                                    &to_path.to_string_lossy(),
                                    "FTP"
                                );
                            } else {
                                crate::file_op_log!(
                                    move,
                                    state.current_user.as_deref().unwrap_or("anonymous"),
                                    ctx.client_ip,
                                    from_path,
                                    &to_path.to_string_lossy(),
                                    "FTP"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "Rename failed: {} -> {}: {} (os error {})",
                                from_path,
                                to_path.display(),
                                e,
                                e.raw_os_error().unwrap_or(0)
                            );
                            control_stream
                                .write_response(b"550 Rename failed\r\n", "FTP response")
                                .await;
                        }
                    }
                } else {
                    control_stream
                        .write_response(
                            b"501 Syntax error: RNTO requires filename\r\n",
                            "FTP response",
                        )
                        .await;
                }
            } else {
                control_stream
                    .write_response(b"503 Bad sequence of commands\r\n", "FTP response")
                    .await;
            }
            state.rename_from = None;
        }

        DELE(filename) => {
            if !state.authenticated {
                control_stream
                    .write_response(b"530 Not logged in\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            let can_delete = {
                let users = ctx.user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_delete)
            };

            if !can_delete {
                control_stream
                    .write_response(b"550 Permission denied\r\n", "FTP response")
                    .await;
                return Ok(true);
            }

            if let Some(filename) = filename {
                let file_path = match state.resolve_path(filename) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("DELE failed for '{}': {}", filename, e);
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

                if !file_path.exists() {
                    control_stream
                        .write_response(b"450 File unavailable: file not found\r\n", "FTP response")
                        .await;
                    return Ok(true);
                }

                if tokio::fs::remove_file(&file_path).await.is_ok() {
                    control_stream
                        .write_response(b"250 File deleted\r\n", "FTP response")
                        .await;
                    crate::file_op_log!(
                        delete,
                        state.current_user.as_deref().unwrap_or("anonymous"),
                        ctx.client_ip,
                        &file_path.to_string_lossy(),
                        "FTP"
                    );
                } else {
                    control_stream
                        .write_response(
                            b"450 File unavailable: delete operation failed\r\n",
                            "FTP response",
                        )
                        .await;
                }
            }
        }

        _ => return Ok(true),
    }

    Ok(true)
}

pub async fn generate_unique_filename(
    state: &super::session_state::SessionState,
    max_attempts: u32,
) -> anyhow::Result<std::path::PathBuf, String> {
    use crate::core::path_utils::path_starts_with_ignore_case;

    let base_name = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for attempt in 0..max_attempts {
        let unique_name = if attempt == 0 {
            format!("stou_{}_{:04x}", base_name, rand::random::<u16>())
        } else {
            format!(
                "stou_{}_{:04x}_{}",
                base_name,
                rand::random::<u16>(),
                attempt
            )
        };

        let file_path = match state.resolve_path(&unique_name) {
            Ok(p) => p,
            Err(e) => return Err(format!("Path resolution error: {}", e)),
        };

        if !path_starts_with_ignore_case(&file_path, &state.home_dir) {
            return Err("Generated path outside home directory".to_string());
        }

        if !file_path.exists() {
            return Ok(file_path);
        }

        tracing::debug!(
            "STOU filename collision detected, retrying: {}",
            unique_name
        );
    }

    Err(format!(
        "Could not generate unique filename after {} attempts",
        max_attempts
    ))
}
