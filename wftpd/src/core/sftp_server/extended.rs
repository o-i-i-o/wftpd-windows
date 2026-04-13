//! SFTP extended commands
//!
//! Handles SFTP extension commands like md5sum, sha256sum, copy-file, statvfs

use crate::core::sftp_server::SftpState;
use md5::Digest as Md5Digest;
use sha2::Digest as Sha2Digest;

impl SftpState {
    pub async fn handle_extended(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1)?;
        let (ext_name, ext_len) = self.parse_string_with_len(data, 5)?;

        match ext_name.as_str() {
            "md5sum" => self.handle_md5sum(id, data, ext_len).await,
            "sha256sum" => self.handle_sha256sum(id, data, ext_len).await,
            "copy-file" => self.handle_copy_file(id, data, ext_len).await,
            "hardlink" => self.handle_hardlink(id, data, ext_len).await,
            "statvfs@openssh.com" => self.handle_statvfs(id, data, ext_len).await,
            _ => {
                tracing::debug!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "EXTENDED_UNKNOWN",
                    extension = %ext_name,
                    "SFTP EXTENDED: unknown extension"
                );
                Ok(self.build_status_packet(
                    id,
                    8,
                    &format!("Unsupported extension: {}", ext_name),
                    "",
                ))
            }
        }
    }

    pub async fn handle_md5sum(
        &mut self,
        id: u32,
        data: &[u8],
        ext_len: usize,
    ) -> Result<Vec<u8>, anyhow::Error> {
        let path_pos = 5 + 4 + ext_len;
        let (path, path_len) = self.parse_string_with_len(data, path_pos)?;
        let start_pos = path_pos + 4 + path_len;
        let start = self.parse_u64(data, start_pos)?;
        let len_pos = start_pos + 8;
        let length = self.parse_u64(data, len_pos)?;

        if !self.check_permission(|p| p.can_read) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        match tokio::fs::File::open(&full_path).await {
            Ok(mut file) => {
                use tokio::io::AsyncSeekExt;
                use tokio::io::AsyncReadExt;

                if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
                    return Ok(self.build_status_packet(id, 4, "Seek failed", ""));
                }

                let mut hasher = md5::Md5::new();
                let mut remaining = if length == 0 { None } else { Some(length) };
                let mut buffer = vec![0u8; 8192];

                loop {
                    let to_read = if let Some(rem) = remaining {
                        if rem == 0 {
                            break;
                        }
                        buffer.len().min(rem as usize)
                    } else {
                        buffer.len()
                    };

                    match file.read(&mut buffer[..to_read]).await {
                        Ok(0) => break,
                        Ok(n) => {
                            hasher.update(&buffer[..n]);
                            if let Some(ref mut rem) = remaining {
                                *rem -= n as u64;
                            }
                        }
                        Err(e) => {
                            return Ok(
                                self.build_status_packet(id, 4, &format!("Read error: {}", e), ""),
                            );
                        }
                    }
                }

                let hash = hasher.finalize();
                let hash_hex = hex::encode(hash);

                let mut payload = vec![124];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&(hash_hex.len() as u32).to_be_bytes());
                payload.extend_from_slice(hash_hex.as_bytes());
                Ok(self.build_packet(&payload))
            }
            Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
        }
    }

    pub async fn handle_sha256sum(
        &mut self,
        id: u32,
        data: &[u8],
        ext_len: usize,
    ) -> Result<Vec<u8>, anyhow::Error> {
        let path_pos = 5 + 4 + ext_len;
        let (path, path_len) = self.parse_string_with_len(data, path_pos)?;
        let start_pos = path_pos + 4 + path_len;
        let start = self.parse_u64(data, start_pos)?;
        let len_pos = start_pos + 8;
        let length = self.parse_u64(data, len_pos)?;

        if !self.check_permission(|p| p.can_read) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        match tokio::fs::File::open(&full_path).await {
            Ok(mut file) => {
                use tokio::io::AsyncSeekExt;
                use tokio::io::AsyncReadExt;

                if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
                    return Ok(self.build_status_packet(id, 4, "Seek failed", ""));
                }

                let mut hasher = sha2::Sha256::new();
                let mut remaining = if length == 0 { None } else { Some(length) };
                let mut buffer = vec![0u8; 8192];

                loop {
                    let to_read = if let Some(rem) = remaining {
                        if rem == 0 {
                            break;
                        }
                        buffer.len().min(rem as usize)
                    } else {
                        buffer.len()
                    };

                    match file.read(&mut buffer[..to_read]).await {
                        Ok(0) => break,
                        Ok(n) => {
                            hasher.update(&buffer[..n]);
                            if let Some(ref mut rem) = remaining {
                                *rem -= n as u64;
                            }
                        }
                        Err(e) => {
                            return Ok(
                                self.build_status_packet(id, 4, &format!("Read error: {}", e), ""),
                            );
                        }
                    }
                }

                let hash = hasher.finalize();
                let hash_hex = hex::encode(hash);

                let mut payload = vec![124];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&(hash_hex.len() as u32).to_be_bytes());
                payload.extend_from_slice(hash_hex.as_bytes());
                Ok(self.build_packet(&payload))
            }
            Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
        }
    }

    pub async fn handle_copy_file(
        &mut self,
        id: u32,
        data: &[u8],
        ext_len: usize,
    ) -> Result<Vec<u8>, anyhow::Error> {
        let src_pos = 5 + 4 + ext_len;
        let (src_path, src_len) = self.parse_string_with_len(data, src_pos)?;
        let dst_pos = src_pos + 4 + src_len;
        let dst_path = self.parse_string(data, dst_pos)?;

        if !self.check_permission(|p| p.can_read && p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let src_full = match self.resolve_path_checked(id, &src_path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };
        let dst_full = match self.resolve_path_checked(id, &dst_path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        if !src_full.exists() {
            return Ok(self.build_status_packet(id, 2, "Source file not found", ""));
        }

        if dst_full.exists() {
            return Ok(self.build_status_packet(id, 4, "Destination already exists", ""));
        }

        match tokio::fs::copy(&src_full, &dst_full).await {
            Ok(bytes_copied) => {
                crate::file_op_log!(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    "COPY",
                    &format!("{} -> {}", src_full.to_string_lossy(), dst_full.to_string_lossy()),
                    bytes_copied,
                    "SFTP",
                    true,
                    "File copied successfully"
                );
                tracing::debug!(
                    "SFTP COPY_FILE: copied {} bytes from {:?} to {:?}",
                    bytes_copied,
                    src_full,
                    dst_full
                );
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                tracing::error!(
                    "SFTP COPY_FILE failed: {} -> {}: {}",
                    src_full.display(),
                    dst_full.display(),
                    e
                );
                Ok(self.build_status_packet(id, 4, "Failed to copy file", ""))
            }
        }
    }

    pub async fn handle_hardlink(
        &mut self,
        id: u32,
        data: &[u8],
        ext_len: usize,
    ) -> Result<Vec<u8>, anyhow::Error> {
        let src_pos = 5 + 4 + ext_len;
        let (src_path, src_len) = self.parse_string_with_len(data, src_pos)?;
        let dst_pos = src_pos + 4 + src_len;
        let dst_path = self.parse_string(data, dst_pos)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let src_full = match self.resolve_path_checked(id, &src_path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };
        let dst_full = match self.resolve_path_checked(id, &dst_path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        if !src_full.exists() {
            return Ok(self.build_status_packet(id, 2, "Source file not found", ""));
        }

        if dst_full.exists() {
            return Ok(self.build_status_packet(id, 4, "Destination already exists", ""));
        }

        match std::fs::hard_link(&src_full, &dst_full) {
            Ok(()) => {
                crate::file_op_log!(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    "HARDLINK",
                    &format!("{} -> {}", src_full.to_string_lossy(), dst_full.to_string_lossy()),
                    0u64,
                    "SFTP",
                    true,
                    "Hardlink created successfully"
                );
                tracing::debug!(
                    "SFTP HARDLINK: created hardlink from {:?} to {:?}",
                    src_full,
                    dst_full
                );
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                tracing::error!(
                    "SFTP HARDLINK failed: {} -> {}: {}",
                    src_full.display(),
                    dst_full.display(),
                    e
                );
                Ok(self.build_status_packet(id, 4, "Failed to create hardlink", ""))
            }
        }
    }

    pub async fn handle_statvfs(
        &mut self,
        id: u32,
        data: &[u8],
        ext_len: usize,
    ) -> Result<Vec<u8>, anyhow::Error> {
        let path_pos = 5 + 4 + ext_len;
        let path = self.parse_string(data, path_pos)?;

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        let mut target_path = full_path.clone();
        while !target_path.exists() {
            if !target_path.pop() {
                target_path = std::path::PathBuf::from(&self.home_dir);
                break;
            }
        }

        match fs2::free_space(&target_path) {
            Ok(free_space) => {
                match fs2::available_space(&target_path) {
                    Ok(available_space) => {
                        let total_space = fs2::total_space(&target_path).unwrap_or(0);

                        let bsize: u64 = 4096;
                        let frsize: u64 = 4096;
                        let blocks: u64 = total_space / frsize;
                        let bfree: u64 = free_space / frsize;
                        let bavail: u64 = available_space / frsize;
                        let files: u64 = 1000000;
                        let ffree: u64 = 500000;
                        let favail: u64 = 500000;
                        let fsid: u64 = 0;
                        let flag: u64 = 0;
                        let namemax: u64 = 255;

                        let mut payload = vec![124];
                        payload.extend_from_slice(&id.to_be_bytes());

                        payload.extend_from_slice(&bsize.to_be_bytes());
                        payload.extend_from_slice(&frsize.to_be_bytes());
                        payload.extend_from_slice(&blocks.to_be_bytes());
                        payload.extend_from_slice(&bfree.to_be_bytes());
                        payload.extend_from_slice(&bavail.to_be_bytes());
                        payload.extend_from_slice(&files.to_be_bytes());
                        payload.extend_from_slice(&ffree.to_be_bytes());
                        payload.extend_from_slice(&favail.to_be_bytes());
                        payload.extend_from_slice(&fsid.to_be_bytes());
                        payload.extend_from_slice(&flag.to_be_bytes());
                        payload.extend_from_slice(&namemax.to_be_bytes());

                        Ok(self.build_packet(&payload))
                    }
                    Err(e) => {
                        tracing::error!("STATVFS available_space error: {}", e);
                        Ok(self.build_status_packet(id, 4, "Failed to get filesystem info", ""))
                    }
                }
            }
            Err(e) => {
                tracing::error!("STATVFS free_space error: {}", e);
                Ok(self.build_status_packet(id, 4, "Failed to get filesystem info", ""))
            }
        }
    }
}
