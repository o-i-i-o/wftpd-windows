//! SFTP file operation commands
//!
//! Handles file operations like open, close, read, write

use crate::core::sftp_server::{
    MAX_HANDLES, SFTP_READ_BUFFER_SIZE, SFTP_WRITE_FLUSH_THRESHOLD, SSH_FXF_APPEND, SSH_FXF_CREAT,
    SSH_FXF_EXCL, SSH_FXF_READ, SSH_FXF_TRUNC, SSH_FXF_WRITE, SftpFileHandle, SftpState,
};

impl SftpState {
    pub async fn handle_open(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let (path, path_len) = self.parse_string_with_len(data, 5)?;
        let pflags_pos = 5 + 4 + path_len;
        let pflags = self.parse_u32(data, pflags_pos);

        let need_read = pflags & SSH_FXF_READ != 0;
        let need_write = pflags & SSH_FXF_WRITE != 0;
        let need_append = pflags & SSH_FXF_APPEND != 0;
        let need_creat = pflags & SSH_FXF_CREAT != 0;
        let need_trunc = pflags & SSH_FXF_TRUNC != 0;
        let need_excl = pflags & SSH_FXF_EXCL != 0;

        if !self.check_permission(|p| {
            (!need_read || p.can_read)
                && (!need_write || p.can_write)
                && (!need_append || p.can_append)
        }) {
            tracing::warn!(
                "SFTP OPEN denied: no permission for user {:?} (read={}, write={}, append={})",
                self.username,
                need_read,
                need_write,
                need_append
            );
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        if self.handles.len() >= MAX_HANDLES {
            tracing::warn!(
                "SFTP OPEN denied: too many open handles ({})",
                self.handles.len()
            );
            return Ok(self.build_status_packet(id, 4, "Too many open handles", ""));
        }

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };
        let file_existed = full_path.exists();

        tracing::debug!(
            "SFTP OPEN: raw='{}', resolved='{}', existed={}, flags=0x{:08X} (read={}, write={}, append={}, creat={}, trunc={}, excl={})",
            path,
            full_path.display(),
            file_existed,
            pflags,
            need_read,
            need_write,
            need_append,
            need_creat,
            need_trunc,
            need_excl
        );

        let file_result = if need_write {
            if need_excl && need_creat && file_existed {
                return Ok(self.build_status_packet(id, 4, "File already exists", ""));
            }

            if need_append {
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(need_creat)
                    .append(true)
                    .open(&full_path)
                    .await
            } else if need_trunc {
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(need_creat)
                    .truncate(true)
                    .open(&full_path)
                    .await
            } else if need_creat {
                tokio::fs::OpenOptions::new()
                    .read(need_read)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(&full_path)
                    .await
            } else {
                tokio::fs::OpenOptions::new()
                    .read(need_read)
                    .write(true)
                    .open(&full_path)
                    .await
            }
        } else {
            tokio::fs::File::open(&full_path).await
        };

        match file_result {
            Ok(file) => {
                let handle = self.generate_handle();
                self.handles.insert(
                    handle.clone(),
                    SftpFileHandle::File {
                        path: full_path,
                        file,
                        locked: false,
                        lock_handle: None,
                        existed: file_existed,
                        written_bytes: 0,
                        read_bytes: 0,
                        pending_flush_bytes: 0,
                        last_access: std::time::Instant::now(),
                    },
                );
                tracing::debug!("SFTP OPEN: handle '{}' created for {}", handle, path);
                Ok(self.build_handle_packet(id, &handle))
            }
            Err(e) => {
                tracing::error!("SFTP OPEN failed for {}: {}", full_path.display(), e);
                Ok(self.build_status_packet(id, 4, "Failed to open file", ""))
            }
        }
    }

    pub async fn handle_close(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let handle = self.parse_string(data, 5)?;

        if let Some(handle_obj) = self.handles.remove(&handle) {
            match handle_obj {
                SftpFileHandle::File {
                    path,
                    locked,
                    lock_handle: _,
                    existed,
                    written_bytes,
                    read_bytes,
                    pending_flush_bytes: _,
                    last_access: _,
                    mut file,
                } => {
                    use tokio::io::AsyncWriteExt;
                    if let Err(e) = file.flush().await {
                        tracing::warn!("Failed to flush file on close {:?}: {}", path, e);
                    }
                    if written_bytes > 0
                        && let Err(e) = file.sync_data().await
                    {
                        tracing::debug!("sync_data on close {:?}: {}", path, e);
                    }

                    // File will be automatically closed on drop, but explicit close releases resources earlier
                    drop(file);

                    if locked {
                        self.locked_files.remove(&path);
                    }

                    if written_bytes > 0 {
                        let file_size = tokio::fs::metadata(&path)
                            .await
                            .map(|m| m.len())
                            .unwrap_or(written_bytes);

                        if existed {
                            crate::file_op_log!(
                                update,
                                self.username.as_deref().unwrap_or("anonymous"),
                                &self.client_ip,
                                &path.to_string_lossy(),
                                file_size,
                                "SFTP"
                            );
                        } else {
                            crate::file_op_log!(
                                upload,
                                self.username.as_deref().unwrap_or("anonymous"),
                                &self.client_ip,
                                &path.to_string_lossy(),
                                written_bytes,
                                "SFTP"
                            );
                        }
                    }

                    if read_bytes > 0 {
                        crate::file_op_log!(
                            download,
                            self.username.as_deref().unwrap_or("anonymous"),
                            &self.client_ip,
                            &path.to_string_lossy(),
                            read_bytes,
                            "SFTP"
                        );
                    }

                    tracing::debug!(
                        client_ip = %self.client_ip,
                        username = ?self.username,
                        action = "CLOSE",
                        handle = %handle,
                        "File handle closed: {:?}",
                        path
                    );
                }
                SftpFileHandle::Dir { path, .. } => {
                    tracing::debug!(
                        client_ip = %self.client_ip,
                        username = ?self.username,
                        action = "CLOSE",
                        handle = %handle,
                        "Directory handle closed: {:?}",
                        path
                    );
                }
            }
        } else {
            tracing::debug!(
                client_ip = %self.client_ip,
                username = ?self.username,
                action = "CLOSE_INVALID",
                handle = %handle,
                "Close request for non-existent handle (already closed or invalid)"
            );
        }
        Ok(self.build_status_packet(id, 0, "OK", ""))
    }

    pub async fn handle_read(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let (handle_str, handle_len) = self.parse_string_with_len(data, 5)?;
        let offset_pos = 5 + 4 + handle_len;
        let offset = self.parse_u64(data, offset_pos);
        let len = self.parse_u32(data, offset_pos + 8) as usize;

        if !self.check_permission(|p| p.can_read) {
            tracing::warn!(
                client_ip = %self.client_ip,
                username = ?self.username,
                action = "READ_DENIED",
                "SFTP READ denied: no read permission"
            );
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File {
                path,
                file,
                read_bytes,
                last_access,
                ..
            }) => {
                use tokio::io::{AsyncReadExt, AsyncSeekExt};

                if let Err(e) = file.seek(std::io::SeekFrom::Start(offset)).await {
                    tracing::error!(
                        client_ip = %self.client_ip,
                        username = ?self.username,
                        action = "READ_ERROR",
                        "SFTP READ seek error for {:?}: {}",
                        path, e
                    );
                    return Ok(self.build_status_packet(id, 4, &format!("Seek error: {}", e), ""));
                }

                let read_len = len.min(SFTP_READ_BUFFER_SIZE);
                let mut buffer = vec![0u8; read_len];

                match file.read(&mut buffer).await {
                    Ok(0) => Ok(self.build_status_packet(id, 1, "End of file", "")),
                    Ok(n) => {
                        if let Some(limiter) = &self.rate_limiter {
                            limiter.acquire(n).await;
                        }

                        buffer.truncate(n);
                        *read_bytes += n as u64;
                        *last_access = std::time::Instant::now();

                        tracing::debug!(
                            client_ip = %self.client_ip,
                            username = ?self.username.as_deref(),
                            action = "READ",
                            handle = %handle_str,
                            "Read {} bytes from {:?} at offset {}",
                            n,
                            path,
                            offset
                        );

                        Ok(self.build_data_packet(id, &buffer))
                    }
                    Err(e) => {
                        tracing::error!(
                            client_ip = %self.client_ip,
                            username = ?self.username,
                            action = "READ_ERROR",
                            "SFTP READ error for {:?}: {}",
                            path, e
                        );
                        Ok(self.build_status_packet(id, 4, &format!("Read error: {}", e), ""))
                    }
                }
            }
            Some(SftpFileHandle::Dir { path, .. }) => {
                tracing::warn!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "READ_INVALID_TYPE",
                    handle = %handle_str,
                    "SFTP READ on directory handle (expected file): {:?}",
                    path
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle type", ""))
            }
            None => {
                tracing::warn!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "READ_INVALID_HANDLE",
                    handle = %handle_str,
                    total_handles = self.handles.len(),
                    "SFTP READ: handle not found (may be closed or invalid)"
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle", ""))
            }
        }
    }

    pub async fn handle_write(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let (handle_str, handle_len) = self.parse_string_with_len(data, 5)?;
        let offset_pos = 5 + 4 + handle_len;
        let offset = self.parse_u64(data, offset_pos);
        let data_len = self.parse_u32(data, offset_pos + 8) as usize;

        if offset_pos + 12 + data_len > data.len() {
            tracing::error!(
                "SFTP WRITE: invalid data length - offset_pos={}, data_len={}, packet_len={}",
                offset_pos,
                data_len,
                data.len()
            );
            return Ok(self.build_status_packet(id, 4, "Invalid data length", ""));
        }
        let write_data = &data[offset_pos + 12..offset_pos + 12 + data_len];

        if let Some(limiter) = &self.rate_limiter {
            limiter.acquire(data_len).await;
        }

        if !self.check_permission(|p| p.can_write) {
            tracing::warn!(
                "SFTP WRITE denied: no write permission for user {:?}",
                self.username
            );
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let quota_mb = self.cached_permissions.as_ref().and_then(|p| p.quota_mb);

        if let Some(quota) = quota_mb {
            let current_usage = self
                .quota_manager
                .get_usage(self.username.as_deref().unwrap_or("anonymous"))
                .await;
            let quota_bytes = quota * 1024 * 1024;
            if current_usage >= quota_bytes {
                tracing::warn!(
                    "SFTP WRITE denied: quota exceeded for user {:?}",
                    self.username
                );
                return Ok(self.build_status_packet(id, 4, "Quota exceeded", ""));
            }
        }

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File {
                path,
                file,
                written_bytes,
                pending_flush_bytes,
                last_access,
                ..
            }) => {
                use tokio::io::{AsyncSeekExt, AsyncWriteExt};

                if let Err(e) = file.seek(std::io::SeekFrom::Start(offset)).await {
                    tracing::error!("SFTP WRITE seek error for {:?}: {}", path, e);
                    return Ok(self.build_status_packet(id, 4, &format!("Seek error: {}", e), ""));
                }

                if let Err(e) = file.write_all(write_data).await {
                    tracing::error!("SFTP WRITE error for {:?}: {}", path, e);
                    return Ok(self.build_status_packet(id, 4, &format!("Write error: {}", e), ""));
                }

                *written_bytes += data_len as u64;
                *pending_flush_bytes += data_len as u64;
                *last_access = std::time::Instant::now();

                if *pending_flush_bytes >= SFTP_WRITE_FLUSH_THRESHOLD as u64 {
                    if let Err(e) = file.flush().await {
                        tracing::error!("SFTP WRITE flush error for {:?}: {}", path, e);
                        return Ok(self.build_status_packet(
                            id,
                            4,
                            &format!("Flush error: {}", e),
                            "",
                        ));
                    }
                    if let Err(e) = file.sync_all().await {
                        tracing::warn!("SFTP WRITE sync_all error for {:?}: {}", path, e);
                    }
                    *pending_flush_bytes = 0;
                }

                tracing::debug!(
                    "SFTP WRITE: {} bytes to {:?} at offset {} (pending_flush: {})",
                    data_len,
                    path,
                    offset,
                    pending_flush_bytes
                );

                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Some(SftpFileHandle::Dir { path, .. }) => {
                tracing::warn!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "WRITE_INVALID_TYPE",
                    handle = %handle_str,
                    "SFTP WRITE on directory handle (expected file): {:?}",
                    path
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle type", ""))
            }
            None => {
                tracing::warn!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "WRITE_INVALID_HANDLE",
                    handle = %handle_str,
                    total_handles = self.handles.len(),
                    "SFTP WRITE: handle not found (may be closed or invalid)"
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle", ""))
            }
        }
    }
}
