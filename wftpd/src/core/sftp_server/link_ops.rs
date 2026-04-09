//! SFTP 链接操作命令
//!
//! 处理 readlink、symlink 等符号链接操作命令

use crate::core::path_utils::path_starts_with_ignore_case;
use crate::core::sftp_server::SftpState;

impl SftpState {
    pub async fn handle_readlink(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        let full_path = match self.resolve_path_checked(id, &path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        match tokio::fs::read_link(&full_path).await {
            Ok(target) => {
                let target_str = target.to_string_lossy().to_string();
                let mut payload = vec![104];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&1u32.to_be_bytes());
                payload.extend_from_slice(&(target_str.len() as u32).to_be_bytes());
                payload.extend_from_slice(target_str.as_bytes());
                payload.extend_from_slice(&(target_str.len() as u32).to_be_bytes());
                payload.extend_from_slice(target_str.as_bytes());
                payload.extend_from_slice(&self.build_attrs(false, 0));
                Ok(self.build_packet(&payload))
            }
            Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
        }
    }

    pub async fn handle_symlink(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1);
        let (target, target_len) = self.parse_string_with_len(data, 5)?;
        let link_pos = 5 + 4 + target_len;
        let link_path = self.parse_string(data, link_pos)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_link = match self.resolve_path_checked(id, &link_path) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };
        let full_target = match self.resolve_path_checked(id, &target) {
            Ok(p) => p,
            Err(resp) => return Ok(resp),
        };

        let home_path = std::path::Path::new(&self.home_dir);
        if !path_starts_with_ignore_case(&full_link, home_path) {
            return Ok(self.build_status_packet(
                id,
                3,
                "Permission denied: link path outside home",
                "",
            ));
        }

        if let Ok(canon_target) = full_target.canonicalize() {
            if !path_starts_with_ignore_case(&canon_target, home_path) {
                tracing::warn!(
                    "SFTP SYMLINK denied: canonicalized target outside home - {} -> {:?}",
                    full_link.display(),
                    canon_target
                );
                return Ok(self.build_status_packet(
                    id,
                    3,
                    "Permission denied: target path outside home",
                    "",
                ));
            }
        } else if !full_target.starts_with(home_path) {
            return Ok(self.build_status_packet(
                id,
                3,
                "Permission denied: target path outside home",
                "",
            ));
        }

        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if full_target.exists()
                && let Ok(metadata) = std::fs::metadata(&full_target)
                && metadata.file_attributes() & 0x400 != 0
            {
                tracing::warn!(
                    "SFTP SYMLINK denied: target is a junction/reparse point - {:?}",
                    full_target
                );
                return Ok(self.build_status_packet(
                    id,
                    3,
                    "Permission denied: junction points not allowed",
                    "",
                ));
            }
        }

        let symlink_result = std::os::windows::fs::symlink_file(&full_target, &full_link);

        if symlink_result.is_ok() {
            crate::file_op_log!(
                self.username.as_deref().unwrap_or("anonymous"),
                &self.client_ip,
                "SYMLINK",
                &format!(
                    "{} -> {}",
                    full_link.to_string_lossy(),
                    full_target.to_string_lossy()
                ),
                0,
                "SFTP",
                true,
                "符号链接创建成功"
            );
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            let e = symlink_result.unwrap_err();
            let msg = if e.raw_os_error() == Some(1314) {
                "Symlink requires administrator privileges on Windows"
            } else {
                "Failed to create symlink"
            };
            Ok(self.build_status_packet(id, 4, msg, ""))
        }
    }
}
