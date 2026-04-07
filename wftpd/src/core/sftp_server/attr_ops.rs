//! SFTP 文件属性操作命令
//!
//! 处理 stat、lstat、fstat、setstat、fsetstat 等文件属性操作命令

use crate::core::sftp_server::{SftpFileHandle, SftpState};
use std::path::PathBuf;

impl SftpState {
    pub async fn handle_stat(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_read || p.can_list) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("STAT failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
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
            Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
        }
    }

    pub async fn handle_lstat(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        if !self.check_permission(|p| p.can_read || p.can_list) {
            let id = self.parse_u32(data, 1);
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }
        self.handle_stat(data).await
    }

    pub async fn handle_fstat(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let handle_str = self.parse_string(data, 5)?;

        let handle = self.handles.get(&handle_str);
        match handle {
            Some(SftpFileHandle::File { path, .. }) => match tokio::fs::metadata(path).await {
                Ok(metadata) => {
                    let mut payload = vec![105];
                    payload.extend_from_slice(&id.to_be_bytes());
                    payload.extend_from_slice(&self.build_attrs(metadata.is_dir(), metadata.len()));
                    Ok(self.build_packet(&payload))
                }
                Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
            },
            _ => Ok(self.build_status_packet(id, 4, "Invalid handle", "")),
        }
    }

    pub async fn handle_setstat(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let (path, path_len) = self.parse_string_with_len(data, 5)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("SETSTAT failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
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
        let id = self.parse_u32(data, 1);
        let (handle_str, handle_len) = self.parse_string_with_len(data, 5)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let path = match self.handles.get(&handle_str) {
            Some(SftpFileHandle::File { path, .. }) => path.clone(),
            Some(SftpFileHandle::Dir { path, .. }) => path.clone(),
            _ => return Ok(self.build_status_packet(id, 4, "Invalid handle", "")),
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

        if flags & 0x00000001 != 0 && offset + 8 <= data.len() {
            let size = u64::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            offset += 8;

            let file = tokio::fs::OpenOptions::new().write(true).open(path).await?;
            file.set_len(size).await?;
            tracing::debug!("SETSTAT: set size to {} for {:?}", size, path);
        }

        if flags & 0x00000002 != 0 && offset + 4 <= data.len() {
            let _uid = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            offset += 4;
            tracing::debug!(
                "SETSTAT: uid change requested for {:?} (ignored on Windows)",
                path
            );
        }

        if flags & 0x00000004 != 0 && offset + 4 <= data.len() {
            let _gid = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            offset += 4;
            tracing::debug!(
                "SETSTAT: gid change requested for {:?} (ignored on Windows)",
                path
            );
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
                tracing::debug!("SETSTAT: set permissions to {:o} for {:?}", mode, path);
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

        if flags & 0x00000010 != 0 && offset + 4 <= data.len() {
            let atime_sec = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as i64;
            offset += 4;

            if offset + 4 <= data.len() {
                let _atime_nsec = u32::from_be_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                offset += 4;
            }

            if flags & 0x00000020 != 0 && offset + 4 <= data.len() {
                let mtime_sec = u32::from_be_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]) as i64;
                offset += 4;

                if offset + 4 <= data.len() {
                    let _mtime_nsec = u32::from_be_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]);
                }

                #[cfg(windows)]
                {
                    use std::time::{Duration, SystemTime};
                    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(mtime_sec as u64);
                    let atime = SystemTime::UNIX_EPOCH + Duration::from_secs(atime_sec as u64);
                    let file = tokio::fs::File::open(path).await?;
                    let std_file = file.into_std().await;
                    let _metadata = std_file.metadata()?;
                    let times = std::fs::FileTimes::new()
                        .set_modified(mtime)
                        .set_accessed(atime);
                    std_file.set_times(times)?;
                    tracing::debug!(
                        "SETSTAT: set mtime={:?}, atime={:?} for {:?}",
                        mtime,
                        atime,
                        path
                    );
                }
                #[cfg(not(windows))]
                {
                    use std::os::unix::fs::FileTimesExt;
                    use std::time::{Duration, SystemTime};
                    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(mtime_sec as u64);
                    let atime = SystemTime::UNIX_EPOCH + Duration::from_secs(atime_sec as u64);
                    let file = tokio::fs::File::open(path).await?;
                    let std_file = file.into_std().await;
                    let times = std::fs::FileTimes::new()
                        .set_modified(mtime)
                        .set_accessed(atime);
                    std_file.set_times(times)?;
                    tracing::debug!(
                        "SETSTAT: set mtime={:?}, atime={:?} for {:?}",
                        mtime,
                        atime,
                        path
                    );
                }
            }
        }

        Ok(())
    }
}
