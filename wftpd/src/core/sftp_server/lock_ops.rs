//! SFTP file lock operation commands
//!
//! Handles file lock operations like lock, unlock (SFTP v5+)

use crate::core::sftp_server::{SftpFileHandle, SftpState};

impl SftpState {
    pub async fn handle_lock(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let handle_str = self.parse_string(data, 5)?;

        if self.sftp_version < 5 {
            return Ok(self.build_status_packet(id, 8, "Lock requires SFTP v5+", ""));
        }

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File {
                path,
                file,
                locked,
                lock_handle,
                ..
            }) => {
                if *locked {
                    return Ok(self.build_status_packet(id, 0, "Already locked", ""));
                }

                let std_file = file.try_clone().await?.into_std().await;
                match fs2::FileExt::lock_exclusive(&std_file) {
                    Ok(()) => {
                        *locked = true;
                        *lock_handle = Some(std_file);
                        self.locked_files.insert(path.clone());
                        tracing::info!(
                            client_ip = %self.client_ip,
                            username = ?self.username.as_deref(),
                            action = "LOCK",
                            "Locked file: {:?}", path
                        );
                        Ok(self.build_status_packet(id, 0, "OK", ""))
                    }
                    Err(_) => Ok(self.build_status_packet(id, 4, "Failed to lock file", "")),
                }
            }
            Some(SftpFileHandle::Dir { path, .. }) => {
                tracing::warn!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "LOCK_INVALID_TYPE",
                    handle = %handle_str,
                    "SFTP LOCK on directory handle (expected file): {:?}",
                    path
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle type", ""))
            }
            None => {
                tracing::debug!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "LOCK_INVALID_HANDLE",
                    handle = %handle_str,
                    "SFTP LOCK: handle not found"
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle", ""))
            }
        }
    }

    pub async fn handle_unlock(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let handle_str = self.parse_string(data, 5)?;

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File {
                path,
                locked,
                lock_handle,
                ..
            }) => {
                if !*locked {
                    return Ok(self.build_status_packet(id, 0, "Not locked", ""));
                }

                if let Some(std_file) = lock_handle.as_ref() {
                    match fs2::FileExt::unlock(std_file) {
                        Ok(()) => {
                            *locked = false;
                            *lock_handle = None;
                            self.locked_files.remove(path);
                            tracing::info!(
                                client_ip = %self.client_ip,
                                username = ?self.username.as_deref(),
                                action = "UNLOCK",
                                "Unlocked file: {:?}", path
                            );
                            Ok(self.build_status_packet(id, 0, "OK", ""))
                        }
                        Err(_) => Ok(self.build_status_packet(id, 4, "Failed to unlock file", "")),
                    }
                } else {
                    *locked = false;
                    Ok(self.build_status_packet(id, 0, "OK", ""))
                }
            }
            Some(SftpFileHandle::Dir { path, .. }) => {
                tracing::warn!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "UNLOCK_INVALID_TYPE",
                    handle = %handle_str,
                    "SFTP UNLOCK on directory handle (expected file): {:?}",
                    path
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle type", ""))
            }
            None => {
                tracing::debug!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "UNLOCK_INVALID_HANDLE",
                    handle = %handle_str,
                    "SFTP UNLOCK: handle not found"
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle", ""))
            }
        }
    }
}
