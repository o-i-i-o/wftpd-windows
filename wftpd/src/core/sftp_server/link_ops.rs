//! SFTP link operation commands
//!
//! Handles symbolic link operations like readlink, symlink

use crate::core::path_utils::path_starts_with_ignore_case;
use crate::core::sftp_server::SftpState;

impl SftpState {
    pub async fn handle_readlink(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1)?;
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_read || p.can_list) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

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
            Err(e) => {
                tracing::debug!("SFTP READLINK failed for {:?}: {}", full_path, e);
                let msg = if e.kind() == std::io::ErrorKind::PermissionDenied {
                    "Permission denied"
                } else {
                    "No such file"
                };
                Ok(self.build_status_packet(id, 2, msg, ""))
            }
        }
    }

    pub async fn handle_symlink(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let id = self.parse_u32(data, 1)?;
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

        let home_path = std::path::Path::new(&self.home_dir);
        if !path_starts_with_ignore_case(&full_link, home_path) {
            return Ok(self.build_status_packet(
                id,
                3,
                "Permission denied: link path outside home",
                "",
            ));
        }

        let link_parent = full_link
            .parent()
            .unwrap_or(std::path::Path::new(&self.home_dir));
        let resolved_target = if std::path::Path::new(&target).is_absolute() {
            match self.resolve_path_checked(id, &target) {
                Ok(p) => p,
                Err(resp) => return Ok(resp),
            }
        } else {
            link_parent.join(&target)
        };

        if let Ok(canon_target) = resolved_target.canonicalize() {
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
        } else if !resolved_target.starts_with(home_path) {
            tracing::warn!(
                "SFTP SYMLINK denied: target path outside home (not resolvable) - {} -> {:?}",
                full_link.display(),
                resolved_target
            );
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
            if resolved_target.exists()
                && let Ok(metadata) = std::fs::metadata(&resolved_target)
                && metadata.file_attributes() & 0x400 != 0
            {
                tracing::warn!(
                    "SFTP SYMLINK denied: target is a junction/reparse point - {:?}",
                    resolved_target
                );
                return Ok(self.build_status_packet(
                    id,
                    3,
                    "Permission denied: junction points not allowed",
                    "",
                ));
            }
        }

        match std::os::windows::fs::symlink_file(&target, &full_link) {
            Ok(()) => {
                crate::file_op_log!(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    "SYMLINK",
                    &format!(
                        "{} -> {}",
                        full_link.to_string_lossy(),
                        resolved_target.to_string_lossy()
                    ),
                    0,
                    "SFTP",
                    true,
                    "Symlink created successfully"
                );
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                let msg = if e.raw_os_error() == Some(1314) {
                    "Symlink requires administrator privileges on Windows"
                } else {
                    "Failed to create symlink"
                };
                Ok(self.build_status_packet(id, 4, msg, ""))
            }
        }
    }
}
