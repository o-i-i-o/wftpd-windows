//! SFTP 扩展命令处理
//!
//! 处理 limits@openssh.com、statvfs@openssh.com、md5sum@ssh.com 等扩展命令

use crate::core::sftp_server::SftpState;

impl SftpState {
    pub async fn handle_extended(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let ext_name = self.parse_string(data, 5)?;

        match ext_name.as_str() {
            "limits@openssh.com" => self.handle_limits(id).await,
            "statvfs@openssh.com" => self.handle_statvfs(id, data).await,
            "md5sum@openssh.com" | "md5-hash@openssh.com" => self.handle_md5sum(id, data).await,
            "sha256sum@openssh.com" | "sha256-hash@openssh.com" => {
                self.handle_sha256sum(id, data).await
            }
            "copy-file" => self.handle_copy_file(id, data).await,
            "hardlink@openssh.com" => self.handle_hardlink(id, data).await,
            _ => Ok(self.build_status_packet(
                id,
                8,
                &format!("Unsupported extension: {}", ext_name),
                "",
            )),
        }
    }

    pub async fn handle_limits(&self, id: u32) -> Result<Vec<u8>, anyhow::Error> {
        let max_packet_size: u64 = 256 * 1024;
        let max_read_size: u64 = 128 * 1024;
        let max_write_size: u64 = 256 * 1024;
        let max_open_handles: u64 = 256;
        let max_locks: u64 = 100;

        let mut payload = vec![201];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&max_packet_size.to_be_bytes());
        payload.extend_from_slice(&max_read_size.to_be_bytes());
        payload.extend_from_slice(&max_write_size.to_be_bytes());
        payload.extend_from_slice(&max_open_handles.to_be_bytes());
        payload.extend_from_slice(&max_locks.to_be_bytes());
        Ok(self.build_packet(&payload))
    }

    pub async fn handle_statvfs(&self, id: u32, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let (_ext_name, ext_len) = self.parse_string_with_len(data, 5)?;
        let path_offset = 5 + 4 + ext_len;
        let path = self.parse_string(data, path_offset)?;
        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

            let wide_path: Vec<u16> = full_path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let mut free_bytes_available: u64 = 0;
            let mut total_bytes: u64 = 0;
            let mut total_free_bytes: u64 = 0;

            unsafe {
                if GetDiskFreeSpaceExW(
                    windows::core::PCWSTR(wide_path.as_ptr()),
                    Some(&mut free_bytes_available),
                    Some(&mut total_bytes),
                    Some(&mut total_free_bytes),
                )
                .is_err()
                {
                    return Ok(self.build_status_packet(
                        id,
                        4,
                        "Failed to get disk space info",
                        "",
                    ));
                }
            }

            let block_size: u64 = 4096;
            let total_blocks = total_bytes / block_size;
            let free_blocks = total_free_bytes / block_size;
            let available_blocks = free_bytes_available / block_size;
            let total_inodes = total_blocks / 16;
            let free_inodes = free_blocks / 16;
            let avail_inodes = available_blocks / 16;
            let fsid: u64 = 0;
            let namemax: u64 = 255;

            let mut payload = vec![201];
            payload.extend_from_slice(&id.to_be_bytes());
            payload.extend_from_slice(&total_blocks.to_be_bytes());
            payload.extend_from_slice(&free_blocks.to_be_bytes());
            payload.extend_from_slice(&available_blocks.to_be_bytes());
            payload.extend_from_slice(&total_inodes.to_be_bytes());
            payload.extend_from_slice(&free_inodes.to_be_bytes());
            payload.extend_from_slice(&avail_inodes.to_be_bytes());
            payload.extend_from_slice(&block_size.to_be_bytes());
            payload.extend_from_slice(&fsid.to_be_bytes());
            payload.extend_from_slice(&namemax.to_be_bytes());

            tracing::debug!(
                "statvfs: path={:?}, total={}MB, free={}MB, available={}MB",
                full_path,
                total_bytes / 1024 / 1024,
                total_free_bytes / 1024 / 1024,
                free_bytes_available / 1024 / 1024
            );

            Ok(self.build_packet(&payload))
        }

        #[cfg(not(windows))]
        {
            use libc::statvfs;
            use std::ffi::CString;

            let path_cstr = match CString::new(full_path.to_string_lossy().as_bytes()) {
                Ok(s) => s,
                Err(_) => return Ok(self.build_status_packet(id, 4, "Invalid path encoding", "")),
            };

            let mut vfs: statvfs = unsafe { std::mem::zeroed() };

            unsafe {
                if statvfs(path_cstr.as_ptr(), &mut vfs) != 0 {
                    return Ok(self.build_status_packet(
                        id,
                        4,
                        "Failed to get filesystem info",
                        "",
                    ));
                }
            }

            let total_blocks = vfs.f_blocks;
            let free_blocks = vfs.f_bfree;
            let available_blocks = vfs.f_bavail;
            let total_inodes = vfs.f_files;
            let free_inodes = vfs.f_ffree;
            let avail_inodes = vfs.f_favail;
            let block_size = vfs.f_bsize as u64;
            let fsid = vfs.f_fsid as u64;
            let namemax = vfs.f_namemax as u64;

            let mut payload = vec![201];
            payload.extend_from_slice(&id.to_be_bytes());
            payload.extend_from_slice(&total_blocks.to_be_bytes());
            payload.extend_from_slice(&free_blocks.to_be_bytes());
            payload.extend_from_slice(&available_blocks.to_be_bytes());
            payload.extend_from_slice(&total_inodes.to_be_bytes());
            payload.extend_from_slice(&free_inodes.to_be_bytes());
            payload.extend_from_slice(&avail_inodes.to_be_bytes());
            payload.extend_from_slice(&block_size.to_be_bytes());
            payload.extend_from_slice(&fsid.to_be_bytes());
            payload.extend_from_slice(&namemax.to_be_bytes());

            tracing::debug!(
                "statvfs: path={:?}, total={}MB, free={}MB, available={}MB",
                full_path,
                (total_blocks * block_size) / 1024 / 1024,
                (free_blocks * block_size) / 1024 / 1024,
                (available_blocks * block_size) / 1024 / 1024
            );

            Ok(self.build_packet(&payload))
        }
    }

    pub async fn handle_md5sum(&self, id: u32, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let (_ext_name, ext_len) = self.parse_string_with_len(data, 5 + 4)?;
        let path_pos = 5 + 4 + ext_len;
        let path = self.parse_string(data, path_pos)?;
        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        if !self.check_permission(|p| p.can_read) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        match tokio::fs::File::open(&full_path).await {
            Ok(mut file) => {
                use md5::{Digest, Md5};
                use tokio::io::AsyncReadExt;
                let mut hasher = Md5::new();
                let mut buffer = [0u8; 8192];
                loop {
                    match file.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(n) => hasher.update(&buffer[..n]),
                        Err(_) => return Ok(self.build_status_packet(id, 4, "Read error", "")),
                    }
                }
                Ok(self.build_hash_response(id, &hex::encode(hasher.finalize())))
            }
            Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
        }
    }

    pub async fn handle_sha256sum(&self, id: u32, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let (_ext_name, ext_len) = self.parse_string_with_len(data, 5 + 4)?;
        let path_pos = 5 + 4 + ext_len;
        let path = self.parse_string(data, path_pos)?;
        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        if !self.check_permission(|p| p.can_read) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        match tokio::fs::File::open(&full_path).await {
            Ok(mut file) => {
                use sha2::{Digest, Sha256};
                use tokio::io::AsyncReadExt;
                let mut hasher = Sha256::new();
                let mut buffer = [0u8; 8192];
                loop {
                    match file.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(n) => hasher.update(&buffer[..n]),
                        Err(_) => return Ok(self.build_status_packet(id, 4, "Read error", "")),
                    }
                }
                Ok(self.build_hash_response(id, &hex::encode(hasher.finalize())))
            }
            Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
        }
    }

    fn build_hash_response(&self, id: u32, hash_hex: &str) -> Vec<u8> {
        let mut payload = vec![201];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&(hash_hex.len() as u32).to_be_bytes());
        payload.extend_from_slice(hash_hex.as_bytes());
        self.build_packet(&payload)
    }

    pub async fn handle_copy_file(
        &mut self,
        id: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, anyhow::Error> {
        let (_ext_name, ext_len) = self.parse_string_with_len(data, 5)?;
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

        match tokio::fs::copy(&src_full, &dst_full).await {
            Ok(size) => {
                crate::file_op_log!(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    "COPY",
                    &format!(
                        "{} -> {}",
                        src_full.to_string_lossy(),
                        dst_full.to_string_lossy()
                    ),
                    size,
                    "SFTP",
                    true,
                    "文件复制成功"
                );
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(_) => Ok(self.build_status_packet(id, 4, "Failed to copy file", "")),
        }
    }

    pub async fn handle_hardlink(
        &mut self,
        id: u32,
        data: &[u8],
    ) -> Result<Vec<u8>, anyhow::Error> {
        let (_ext_name, ext_len) = self.parse_string_with_len(data, 5)?;
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

        match std::fs::hard_link(&src_full, &dst_full) {
            Ok(_) => {
                crate::file_op_log!(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    "HARDLINK",
                    &format!(
                        "{} -> {}",
                        src_full.to_string_lossy(),
                        dst_full.to_string_lossy()
                    ),
                    0,
                    "SFTP",
                    true,
                    "硬链接创建成功"
                );
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                tracing::error!(
                    client_ip = %self.client_ip,
                    username = ?self.username.as_deref(),
                    action = "HARDLINK_FAIL",
                    "Failed to create hardlink: {} -> {}: {}", src_path, dst_path, e
                );
                Ok(self.build_status_packet(id, 4, "Failed to create hardlink", ""))
            }
        }
    }
}
