//! SFTP 目录操作命令
//!
//! 处理 opendir、readdir、mkdir、rmdir、realpath、rename、remove 等目录操作命令

use crate::core::path_utils::path_starts_with_ignore_case;
use crate::core::sftp_server::{DirEntry, MAX_HANDLES, SftpFileHandle, SftpState};
use std::path::PathBuf;

impl SftpState {
    pub async fn handle_opendir(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_list) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        if self.handles.len() >= MAX_HANDLES {
            tracing::warn!(
                "SFTP OPENDIR denied: too many open handles ({})",
                self.handles.len()
            );
            return Ok(self.build_status_packet(id, 4, "Too many open handles", ""));
        }

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        if !full_path.exists() {
            return Ok(self.build_status_packet(id, 2, "No such directory", ""));
        }

        if !full_path.is_dir() {
            return Ok(self.build_status_packet(id, 4, "Not a directory", ""));
        }

        let handle = self.generate_handle();
        self.handles.insert(
            handle.clone(),
            SftpFileHandle::Dir {
                path: full_path,
                entries: Vec::new(),
                index: 0,
                last_access: std::time::Instant::now(),
            },
        );

        Ok(self.build_handle_packet(id, &handle))
    }

    pub async fn handle_readdir(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let handle_str = self.parse_string(data, 5)?;

        let entries_result = {
            let handle = self.handles.get_mut(&handle_str);
            match handle {
                Some(SftpFileHandle::Dir {
                    path,
                    entries,
                    index,
                    last_access,
                }) => {
                    if entries.is_empty() && *index == 0 {
                        let mut read_entries = Vec::new();
                        match tokio::fs::read_dir(path).await {
                            Ok(mut dir) => {
                                while let Ok(Some(entry)) = dir.next_entry().await {
                                    let name = entry.file_name().to_string_lossy().to_string();
                                    let is_dir = entry
                                        .file_type()
                                        .await
                                        .map(|t| t.is_dir())
                                        .unwrap_or(false);
                                    let (size, mtime) = entry
                                        .metadata()
                                        .await
                                        .map(|m| {
                                            let mt = m
                                                .modified()
                                                .ok()
                                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                                .map(|d| d.as_secs() as u32)
                                                .unwrap_or(0);
                                            (m.len(), mt)
                                        })
                                        .unwrap_or((0, 0));
                                    read_entries.push(DirEntry { name, is_dir, size, mtime });
                                }
                            }
                            Err(e) => {
                                return Ok(self.build_status_packet(
                                    id,
                                    4,
                                    &format!("Failed to read directory: {}", e),
                                    "",
                                ));
                            }
                        }
                        *entries = read_entries;
                    }

                    if *index >= entries.len() {
                        return Ok(self.build_status_packet(id, 1, "End of directory", ""));
                    }

                    let count = (entries.len() - *index).min(100);
                    let result_entries: Vec<DirEntry> =
                        entries[*index..*index + count].to_vec();
                    *index += count;
                    *last_access = std::time::Instant::now();
                    Some(result_entries)
                }
                _ => None,
            }
        };

        match entries_result {
            Some(dir_entries) => {
                let mut payload = vec![104];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&(dir_entries.len() as u32).to_be_bytes());

                for entry in &dir_entries {
                    payload.extend_from_slice(&(entry.name.len() as u32).to_be_bytes());
                    payload.extend_from_slice(entry.name.as_bytes());

                    let mtime_str = chrono::DateTime::from_timestamp(entry.mtime as i64, 0)
                        .map(|dt| dt.format("%b %d %H:%M").to_string())
                        .unwrap_or_else(|| "Jan 01 00:00".to_string());

                    let long_name = format!(
                        "{} 1 user user {:>10} {} {}",
                        if entry.is_dir { "drwxr-xr-x" } else { "-rw-r--r--" },
                        entry.size,
                        mtime_str,
                        entry.name
                    );
                    payload.extend_from_slice(&(long_name.len() as u32).to_be_bytes());
                    payload.extend_from_slice(long_name.as_bytes());

                    payload.extend_from_slice(&self.build_attrs(entry.is_dir, entry.size));
                }

                Ok(self.build_packet(&payload))
            }
            None => {
                tracing::debug!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "READDIR_INVALID_HANDLE",
                    handle = %handle_str,
                    "SFTP READDIR: handle not found or invalid type"
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle", ""))
            }
        }
    }

    pub async fn handle_remove(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_delete) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        if tokio::fs::remove_file(&full_path).await.is_ok() {
            crate::file_op_log!(
                delete,
                self.username.as_deref().unwrap_or("anonymous"),
                &self.client_ip,
                &full_path.to_string_lossy(),
                "SFTP"
            );
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            Ok(self.build_status_packet(id, 4, "Failed to remove file", ""))
        }
    }

    pub async fn handle_mkdir(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_mkdir) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        if tokio::fs::create_dir_all(&full_path).await.is_ok() {
            crate::file_op_log!(
                mkdir,
                self.username.as_deref().unwrap_or("anonymous"),
                &self.client_ip,
                &full_path.to_string_lossy(),
                "SFTP"
            );
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            Ok(self.build_status_packet(id, 4, "Failed to create directory", ""))
        }
    }

    pub async fn handle_rmdir(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_rmdir) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        let result = tokio::fs::remove_dir(&full_path).await;

        if result.is_ok() {
            crate::file_op_log!(
                rmdir,
                self.username.as_deref().unwrap_or("anonymous"),
                &self.client_ip,
                &full_path.to_string_lossy(),
                "SFTP"
            );
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            let err = result.unwrap_err();
            if err.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                Ok(self.build_status_packet(id, 4, "Directory not empty", ""))
            } else {
                Ok(self.build_status_packet(id, 4, "Failed to remove directory", ""))
            }
        }
    }

    pub async fn handle_rename(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let (old_path, old_len) = self.parse_string_with_len(data, 5)?;
        let new_path_pos = 5 + 4 + old_len;
        let new_path = self.parse_string(data, new_path_pos)?;

        if !self.check_permission(|p| p.can_rename) {
            tracing::warn!(
                "SFTP RENAME denied: no permission for user {:?}",
                self.username
            );
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let old_full = match self.resolve_path_checked(id, &old_path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };
        let new_full = match self.resolve_path_checked(id, &new_path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        tracing::debug!(
            "SFTP RENAME: raw_old='{}', resolved_old='{}', raw_new='{}', resolved_new='{}'",
            old_path,
            old_full.display(),
            new_path,
            new_full.display()
        );

        if !old_full.exists() {
            tracing::warn!(
                "SFTP RENAME failed: source does not exist - {}",
                old_full.display()
            );
            return Ok(self.build_status_packet(id, 2, "No such file", ""));
        }

        if !path_starts_with_ignore_case(&old_full, PathBuf::from(&self.home_dir))
            || !path_starts_with_ignore_case(&new_full, PathBuf::from(&self.home_dir))
        {
            tracing::warn!(
                "SFTP RENAME denied: path outside home - old={}, new={}",
                old_full.display(),
                new_full.display()
            );
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        if let Err(resp) = self.check_symlink_in_home(&old_full).await {
            return Ok(resp);
        }

        if new_full.exists() && let Err(resp) = self.check_symlink_in_home(&new_full).await {
            return Ok(resp);
        }

        if new_full.exists() {
            return Ok(self.build_status_packet(id, 4, "File already exists", ""));
        }

        match tokio::fs::rename(&old_full, &new_full).await {
            Ok(()) => {
                let old_parent = old_full
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let new_parent = new_full
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if old_parent == new_parent {
                    crate::file_op_log!(
                        rename,
                        self.username.as_deref().unwrap_or("anonymous"),
                        &self.client_ip,
                        &old_full.to_string_lossy(),
                        &new_full.to_string_lossy(),
                        "SFTP"
                    );
                } else {
                    crate::file_op_log!(
                        move,
                        self.username.as_deref().unwrap_or("anonymous"),
                        &self.client_ip,
                        &old_full.to_string_lossy(),
                        &new_full.to_string_lossy(),
                        "SFTP"
                    );
                }
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                tracing::error!(
                    "SFTP Rename failed: {} -> {}: {} (os error {:?})",
                    old_full.display(),
                    new_full.display(),
                    e,
                    e.raw_os_error()
                );
                Ok(self.build_status_packet(id, 4, "Failed to rename", ""))
            }
        }
    }

    pub async fn handle_realpath(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        let full_path = if path.is_empty() || path == "." {
            Ok(PathBuf::from(&self.cwd))
        } else if path == ".." {
            self.resolve_path("..")
        } else {
            self.resolve_path(&path)
        };

        let full_path = match full_path {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("REALPATH failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        let resolved = if full_path.exists() {
            match full_path.canonicalize() {
                Ok(canon) => {
                    if !path_starts_with_ignore_case(&canon, PathBuf::from(&self.home_dir)) {
                        tracing::warn!(
                            "REALPATH security: canonicalized path escapes home - input: {}, canonicalized: {}",
                            path,
                            canon.display()
                        );
                        return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
                    }
                    canon
                }
                Err(_) => full_path,
            }
        } else {
            full_path
        };

        let path_str = match crate::core::path_utils::to_ftp_path(
            &resolved,
            std::path::Path::new(&self.home_dir),
        ) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("REALPATH failed: {}", e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        let mut payload = vec![104];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&1u32.to_be_bytes());

        payload.extend_from_slice(&(path_str.len() as u32).to_be_bytes());
        payload.extend_from_slice(path_str.as_bytes());

        let longname = format!("drwxr-xr-x  1 user user  0 Jan 01 00:00 {}", path_str);
        payload.extend_from_slice(&(longname.len() as u32).to_be_bytes());
        payload.extend_from_slice(longname.as_bytes());

        payload.extend_from_slice(&self.build_attrs(true, 0));

        tracing::debug!(
            client_ip = %self.client_ip,
            username = ?self.username.as_deref(),
            action = "REALPATH",
            protocol = "SFTP",
            "Resolved '{}' -> '{}'",
            path,
            path_str
        );

        Ok(self.build_packet(&payload))
    }
}
