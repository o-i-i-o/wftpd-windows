//! SFTP file locking operation commands
//!
//! Handles file locking operations for SFTP protocol version 5+

use crate::core::sftp_server::{SftpFileHandle, SftpState};

impl SftpState {
    pub async fn handle_lock(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1)?;
        let (handle_str, handle_len) = self.parse_string_with_len(data, 5)?;

        let offset_pos = 5 + 4 + handle_len;
        let offset = self.parse_u64(data, offset_pos)?;
        let length_pos = offset_pos + 8;
        let length = self.parse_u64(data, length_pos)?;
        let flags_pos = length_pos + 8;
        let flags = self.parse_u32(data, flags_pos)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File {
                path,
                locked,
                last_access,
                ..
            }) => {
                if *locked {
                    return Ok(self.build_status_packet(id, 4, "File already locked", ""));
                }

                *last_access = std::time::Instant::now();

                let lock_type = flags & 0x7;
                let blocking = (flags & 0x8) != 0;

                if blocking {
                    return Ok(self.build_status_packet(id, 4, "Blocking locks not supported", ""));
                }

                match lock_type {
                    0 => {
                        tracing::debug!(
                            "SFTP LOCK: read lock requested on {:?} at offset {} length {}",
                            path,
                            offset,
                            length
                        );
                    }
                    1 => {
                        tracing::debug!(
                            "SFTP LOCK: write lock requested on {:?} at offset {} length {}",
                            path,
                            offset,
                            length
                        );
                    }
                    2 => {
                        tracing::debug!(
                            "SFTP LOCK: unlock requested on {:?} at offset {} length {}",
                            path,
                            offset,
                            length
                        );
                        *locked = false;
                        self.locked_files.remove(path);
                        return Ok(self.build_status_packet(id, 0, "OK", ""));
                    }
                    _ => {
                        return Ok(self.build_status_packet(id, 4, "Invalid lock type", ""));
                    }
                }

                if self.locked_files.contains(path) {
                    return Ok(self.build_status_packet(id, 4, "Lock conflict", ""));
                }

                *locked = true;
                self.locked_files.insert(path.clone());

                tracing::debug!("SFTP LOCK: advisory lock acquired on {:?}", path);
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Some(SftpFileHandle::Dir { .. }) => {
                tracing::warn!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "LOCK_INVALID_TYPE",
                    handle = %handle_str,
                    "SFTP LOCK on directory handle (expected file)"
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
        let id = self.parse_u32(data, 1)?;
        let (handle_str, handle_len) = self.parse_string_with_len(data, 5)?;

        let offset_pos = 5 + 4 + handle_len;
        let offset = self.parse_u64(data, offset_pos)?;
        let length_pos = offset_pos + 8;
        let length = self.parse_u64(data, length_pos)?;

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File {
                path,
                locked,
                last_access,
                ..
            }) => {
                *last_access = std::time::Instant::now();

                tracing::debug!(
                    "SFTP UNLOCK: releasing lock on {:?} at offset {} length {}",
                    path,
                    offset,
                    length
                );

                if *locked {
                    *locked = false;
                    self.locked_files.remove(path);
                    tracing::debug!("SFTP UNLOCK: lock released on {:?}", path);
                    Ok(self.build_status_packet(id, 0, "OK", ""))
                } else {
                    tracing::debug!("SFTP UNLOCK: no lock to release on {:?}", path);
                    Ok(self.build_status_packet(id, 0, "OK", ""))
                }
            }
            Some(SftpFileHandle::Dir { .. }) => {
                tracing::warn!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "UNLOCK_INVALID_TYPE",
                    handle = %handle_str,
                    "SFTP UNLOCK on directory handle (expected file)"
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
