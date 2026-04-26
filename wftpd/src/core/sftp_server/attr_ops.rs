//! SFTP file attribute operation commands
//!
//! Handles file attribute operations like stat, lstat, fstat, setstat, fsetstat

use crate::core::sftp_server::{SftpFileHandle, SftpState};
use std::path::PathBuf;

impl SftpState {
    pub async fn handle_stat(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1)?;
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_read || p.can_list) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        match tokio::fs::metadata(&full_path).await {
            Ok(metadata) => {
                let is_dir = metadata.is_dir();
                let attrs = self.build_attrs_extended(&metadata, is_dir);

                let mut payload = vec![105];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&attrs);
                Ok(self.build_packet(&payload))
            }
            Err(e) => {
                tracing::debug!("SFTP STAT failed for {:?}: {}", full_path, e);
                let msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                    "Permission denied"
                } else {
                    "No such file"
                };
                Ok(self.build_status_packet(id, 2, msg, ""))
            }
        }
    }

    pub async fn handle_lstat(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1)?;
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_read || p.can_list) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        match tokio::fs::symlink_metadata(&full_path).await {
            Ok(metadata) => {
                let is_dir = metadata.is_dir();
                let is_symlink = metadata.file_type().is_symlink();
                let attrs = self.build_attrs_extended(&metadata, is_dir || is_symlink);

                let mut payload = vec![105];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&attrs);
                Ok(self.build_packet(&payload))
            }
            Err(e) => {
                tracing::debug!("SFTP LSTAT failed for {:?}: {}", full_path, e);
                let msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                    "Permission denied"
                } else {
                    "No such file"
                };
                Ok(self.build_status_packet(id, 2, msg, ""))
            }
        }
    }

    pub async fn handle_fstat(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1)?;
        let handle_str = self.parse_string(data, 5)?;

        let handle = self.handles.get(&handle_str);
        match handle {
            Some(SftpFileHandle::File { file, .. }) => match file.metadata().await {
                Ok(metadata) => {
                    let mut payload = vec![105];
                    payload.extend_from_slice(&id.to_be_bytes());
                    payload.extend_from_slice(
                        &self.build_attrs_extended(&metadata, metadata.is_dir()),
                    );
                    Ok(self.build_packet(&payload))
                }
                Err(e) => {
                    tracing::debug!("SFTP FSTAT failed: {}", e);
                    let msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                        "Permission denied"
                    } else {
                        "No such file"
                    };
                    Ok(self.build_status_packet(id, 2, msg, ""))
                }
            },
            Some(SftpFileHandle::Dir { .. }) => {
                tracing::warn!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "FSTAT_INVALID_TYPE",
                    handle = %handle_str,
                    "SFTP FSTAT on directory handle (expected file)"
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle type", ""))
            }
            None => {
                tracing::debug!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "FSTAT_INVALID_HANDLE",
                    handle = %handle_str,
                    "SFTP FSTAT: handle not found"
                );
                Ok(self.build_status_packet(id, 4, "Invalid handle", ""))
            }
        }
    }

    pub async fn handle_setstat(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1)?;
        let (path, path_len) = self.parse_string_with_len(data, 5)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        if !full_path.exists() {
            return Ok(self.build_status_packet(id, 2, "No such file", ""));
        }

        let attr_offset = 5 + 4 + path_len;
        if attr_offset >= data.len() {
            return Ok(self.build_status_packet(id, 0, "OK", ""));
        }

        match self
            .apply_file_attributes(&full_path, &data[attr_offset..])
            .await
        {
            Ok(()) => {
                tracing::debug!("SETSTAT applied to {:?}", full_path);
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                tracing::warn!("SETSTAT failed for {:?}: {}", full_path, e);
                Ok(
                    self.build_status_packet(
                        id,
                        4,
                        &format!("Failed to set attributes: {}", e),
                        "",
                    ),
                )
            }
        }
    }

    pub async fn handle_fsetstat(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1)?;
        let (handle_str, handle_len) = self.parse_string_with_len(data, 5)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let path = match self.handles.get(&handle_str) {
            Some(SftpFileHandle::File { path, .. }) => path.clone(),
            Some(SftpFileHandle::Dir { path, .. }) => path.clone(),
            None => {
                tracing::debug!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "FSETSTAT_INVALID_HANDLE",
                    handle = %handle_str,
                    "SFTP FSETSTAT: handle not found"
                );
                return Ok(self.build_status_packet(id, 4, "Invalid handle", ""));
            }
        };

        let attr_offset = 5 + 4 + handle_len;
        if attr_offset >= data.len() {
            return Ok(self.build_status_packet(id, 0, "OK", ""));
        }

        match self
            .apply_file_attributes(&path, &data[attr_offset..])
            .await
        {
            Ok(()) => {
                tracing::debug!("FSETSTAT applied to {:?}", path);
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                tracing::warn!("FSETSTAT failed for {:?}: {}", path, e);
                Ok(
                    self.build_status_packet(
                        id,
                        4,
                        &format!("Failed to set attributes: {}", e),
                        "",
                    ),
                )
            }
        }
    }

    pub async fn apply_file_attributes(
        &self,
        path: &PathBuf,
        data: &[u8],
    ) -> Result<(), anyhow::Error> {
        if data.len() < 4 {
            return Ok(());
        }

        let flags = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let mut offset = 4;

        let need_set_size = flags & 0x00000001 != 0;
        let need_set_times = (flags & 0x00000010 != 0) || (flags & 0x00000020 != 0);

        let mut target_size: Option<u64> = None;
        let mut atime_sec: Option<u64> = None;
        let mut mtime_sec: Option<u64> = None;

        if need_set_size && offset + 8 <= data.len() {
            target_size = Some(u64::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]));
            offset += 8;
        }

        if flags & 0x00000002 != 0 && offset + 4 <= data.len() {
            offset += 4;
        }

        if flags & 0x00000004 != 0 && offset + 4 <= data.len() {
            offset += 4;
        }

        if flags & 0x00000008 != 0 && offset + 4 <= data.len() {
            let permissions = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            offset += 4;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = permissions & 0o777;
                let metadata = tokio::fs::metadata(path).await?;
                let mut perm = metadata.permissions();
                perm.set_mode(mode);
                tokio::fs::set_permissions(path, perm).await?;
            }
            #[cfg(windows)]
            {
                tracing::debug!(
                    "SETSTAT: permissions change to {:o} for {:?} (ignored on Windows)",
                    permissions,
                    path
                );
            }
        }

        if flags & 0x00000010 != 0 && offset + 8 <= data.len() {
            atime_sec = Some(u64::from(u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])));
            offset += 8;
        }

        if flags & 0x00000020 != 0 && offset + 8 <= data.len() {
            mtime_sec = Some(u64::from(u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])));
        }

        if !need_set_size && !need_set_times {
            return Ok(());
        }

        let file = tokio::fs::OpenOptions::new().write(true).open(path).await?;

        if let Some(size) = target_size {
            file.set_len(size).await?;
            tracing::debug!("SETSTAT: set size to {} for {:?}", size, path);
        }

        if atime_sec.is_some() || mtime_sec.is_some() {
            use std::time::{Duration, SystemTime};
            let std_file = file.into_std().await;
            let metadata = std_file.metadata()?;
            let original_atime = metadata.accessed().ok().unwrap_or(SystemTime::UNIX_EPOCH);
            let original_mtime = metadata.modified().ok().unwrap_or(SystemTime::UNIX_EPOCH);

            let atime = atime_sec
                .map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s))
                .unwrap_or(original_atime);
            let mtime = mtime_sec
                .map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s))
                .unwrap_or(original_mtime);

            let times = std::fs::FileTimes::new()
                .set_accessed(atime)
                .set_modified(mtime);
            std_file.set_times(times)?;
            tracing::debug!(
                "SETSTAT: set atime={:?}, mtime={:?} for {:?}",
                atime,
                mtime,
                path
            );
        }

        Ok(())
    }
}
