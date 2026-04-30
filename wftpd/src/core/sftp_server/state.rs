//! SFTP server state management

use anyhow::Result;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::core::path_utils::{PathResolveError, safe_resolve_path_with_cwd};
use crate::core::quota::QuotaManager;
use crate::core::rate_limiter::RateLimiter;
use crate::core::users::UserManager;

use super::types::{HANDLE_TIMEOUT_SECS, SftpFileHandle};

pub struct SftpState {
    pub home_dir: String,
    pub cwd: String,
    pub username: Option<String>,
    pub user_manager: Arc<Mutex<UserManager>>,
    pub quota_manager: Arc<QuotaManager>,
    pub handles: HashMap<String, SftpFileHandle>,
    pub next_handle_id: u32,
    pub sftp_version: u32,
    pub buffer: Vec<u8>,
    pub locked_files: HashSet<PathBuf>,
    pub client_ip: String,
    pub cached_permissions: Option<crate::core::users::Permissions>,
    pub rate_limiter: Option<RateLimiter>,
    pub cache_expiry: Option<std::time::Instant>,
    pub last_handle_cleanup: std::time::Instant,
    pub allow_symlinks: bool,
}

impl SftpState {
    pub fn new(
        home_dir: String,
        username: Option<String>,
        user_manager: Arc<Mutex<UserManager>>,
        quota_manager: Arc<QuotaManager>,
        client_ip: String,
        allow_symlinks: bool,
    ) -> Self {
        let mut state = SftpState {
            home_dir: home_dir.clone(),
            cwd: home_dir,
            username,
            user_manager,
            quota_manager,
            handles: HashMap::new(),
            next_handle_id: 0,
            sftp_version: 3,
            buffer: Vec::new(),
            locked_files: HashSet::new(),
            client_ip,
            cached_permissions: None,
            rate_limiter: None,
            cache_expiry: None,
            last_handle_cleanup: std::time::Instant::now(),
            allow_symlinks,
        };
        state.cache_permissions();
        state.init_rate_limiter();
        state
    }

    pub fn check_permission(
        &mut self,
        check_fn: impl Fn(&crate::core::users::Permissions) -> bool,
    ) -> bool {
        if let Some(expiry) = self.cache_expiry
            && expiry < std::time::Instant::now()
        {
            self.cached_permissions = None;
            self.cache_permissions();
        }

        if let Some(perms) = &self.cached_permissions {
            return check_fn(perms);
        }

        if let Some(username) = &self.username {
            let users = self.user_manager.lock();
            if let Some(user) = users.get_user(username) {
                check_fn(&user.permissions)
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn cache_permissions(&mut self) {
        if let Some(username) = &self.username {
            let users = self.user_manager.lock();
            if let Some(user) = users.get_user(username) {
                self.cached_permissions = Some(user.permissions);
                self.cache_expiry =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
            }
        }
    }

    pub fn refresh_permissions(&mut self) {
        self.cached_permissions = None;
        self.cache_permissions();
    }

    pub fn init_rate_limiter(&mut self) {
        if let Some(perms) = &self.cached_permissions
            && let Some(speed_kbps) = perms.speed_limit_kbps
        {
            self.rate_limiter = Some(RateLimiter::new(speed_kbps));
        }
    }

    pub fn resolve_path_checked(
        &self,
        id: u32,
        path: &str,
    ) -> std::result::Result<PathBuf, Vec<u8>> {
        self.resolve_path(path).map_err(|e| {
            tracing::warn!("Path resolve failed for '{}': {}", path, e);
            self.build_status_packet(id, 2, &e.to_string(), "")
        })
    }

    pub async fn check_symlink_in_home(
        &self,
        id: u32,
        path: &PathBuf,
    ) -> std::result::Result<(), Vec<u8>> {
        if !path.is_symlink() {
            return Ok(());
        }
        match tokio::fs::read_link(path).await {
            Ok(link_target) => {
                let resolved = if link_target.is_absolute() {
                    link_target
                } else {
                    let parent = path
                        .parent()
                        .unwrap_or(std::path::Path::new(&self.home_dir));
                    parent.join(&link_target)
                };
                match resolved.canonicalize() {
                    Ok(canon) => {
                        let home = PathBuf::from(&self.home_dir);
                        if !crate::core::path_utils::path_starts_with_ignore_case(&canon, home) {
                            tracing::warn!(
                                "Symlink points outside home: {:?} -> {:?}",
                                path,
                                canon
                            );
                            return Err(self.build_status_packet(
                                id,
                                3,
                                "Permission denied: symlink target outside home",
                                "",
                            ));
                        }
                        Ok(())
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Symlink target cannot be resolved (rejecting for security): {:?} -> {:?}, error: {}",
                            path,
                            resolved,
                            e
                        );
                        Err(self.build_status_packet(
                            id,
                            3,
                            "Permission denied: symlink target cannot be resolved",
                            "",
                        ))
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Cannot read symlink {:?}: {}", path, e);
                Err(self.build_status_packet(id, 4, "Failed to read symlink", ""))
            }
        }
    }

    pub fn cleanup(&mut self) {
        for (_, handle) in self.handles.drain() {
            if let SftpFileHandle::File { locked, path, .. } = handle
                && locked
            {
                tracing::info!("Releasing lock on {:?} during cleanup", path);
                self.locked_files.remove(&path);
            }
        }
        self.locked_files.clear();
        tracing::info!("SFTP session cleanup completed");
    }

    pub async fn cleanup_expired_handles(&mut self) {
        let timeout = std::time::Duration::from_secs(HANDLE_TIMEOUT_SECS);
        let expired: Vec<String> = self
            .handles
            .iter()
            .filter(|(_, h)| match h {
                SftpFileHandle::File { last_access, .. } => last_access.elapsed() > timeout,
                SftpFileHandle::Dir { last_access, .. } => last_access.elapsed() > timeout,
            })
            .map(|(k, _)| k.clone())
            .collect();

        for handle_key in expired {
            if let Some(handle) = self.handles.remove(&handle_key) {
                match handle {
                    SftpFileHandle::File {
                        locked,
                        path,
                        mut file,
                        ..
                    } => {
                        use tokio::io::AsyncWriteExt;
                        if let Err(e) = file.flush().await {
                            tracing::warn!(
                                "Failed to flush file {:?} on handle expiry: {}",
                                path,
                                e
                            );
                        }
                        if locked {
                            self.locked_files.remove(&path);
                        }
                        tracing::info!(
                            "SFTP handle expired and closed: {:?} (handle={})",
                            path,
                            handle_key
                        );
                    }
                    SftpFileHandle::Dir { path, .. } => {
                        tracing::info!(
                            "SFTP dir handle expired and closed: {:?} (handle={})",
                            path,
                            handle_key
                        );
                    }
                }
            }
        }
    }

    pub fn resolve_path(&self, path: &str) -> Result<PathBuf, PathResolveError> {
        safe_resolve_path_with_cwd(&self.cwd, &self.home_dir, path, self.allow_symlinks)
    }

    pub fn generate_handle(&mut self) -> String {
        let handle = format!("h{:08x}", self.next_handle_id);
        self.next_handle_id = self.next_handle_id.wrapping_add(1);
        handle
    }

    pub fn parse_u32(&self, data: &[u8], offset: usize) -> Result<u32> {
        if offset + 4 > data.len() {
            return Err(anyhow::anyhow!(
                "Insufficient data for u32 at offset {}",
                offset
            ));
        }
        Ok(u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]))
    }

    pub fn parse_u64(&self, data: &[u8], offset: usize) -> Result<u64> {
        if offset + 8 > data.len() {
            return Err(anyhow::anyhow!(
                "Insufficient data for u64 at offset {}",
                offset
            ));
        }
        Ok(u64::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]))
    }

    pub fn parse_string(&self, data: &[u8], offset: usize) -> Result<String> {
        let len = self.parse_u32(data, offset)? as usize;
        if offset + 4 + len > data.len() {
            return Err(anyhow::anyhow!(
                "String length {} exceeds available data at offset {}",
                len,
                offset
            ));
        }
        Ok(String::from_utf8_lossy(&data[offset + 4..offset + 4 + len]).to_string())
    }

    pub fn parse_string_with_len(&self, data: &[u8], offset: usize) -> Result<(String, usize)> {
        let len = self.parse_u32(data, offset)? as usize;
        if offset + 4 + len > data.len() {
            return Err(anyhow::anyhow!(
                "String length {} exceeds available data at offset {}",
                len,
                offset
            ));
        }
        let s = String::from_utf8_lossy(&data[offset + 4..offset + 4 + len]).to_string();
        Ok((s, len))
    }

    pub fn build_packet(&self, payload: &[u8]) -> Vec<u8> {
        let mut packet = Vec::new();
        packet.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        packet.extend_from_slice(payload);
        packet
    }

    pub fn build_version_packet(&self, version: u32) -> Vec<u8> {
        let mut payload = vec![2];
        payload.extend_from_slice(&version.to_be_bytes());
        self.build_packet(&payload)
    }

    pub fn build_status_packet(&self, id: u32, status: u32, msg: &str, lang: &str) -> Vec<u8> {
        let mut payload = vec![101];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&status.to_be_bytes());
        payload.extend_from_slice(&(msg.len() as u32).to_be_bytes());
        payload.extend_from_slice(msg.as_bytes());
        payload.extend_from_slice(&(lang.len() as u32).to_be_bytes());
        payload.extend_from_slice(lang.as_bytes());
        self.build_packet(&payload)
    }

    pub fn build_handle_packet(&self, id: u32, handle: &str) -> Vec<u8> {
        let mut payload = vec![102];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&(handle.len() as u32).to_be_bytes());
        payload.extend_from_slice(handle.as_bytes());
        self.build_packet(&payload)
    }

    pub fn build_data_packet(&self, id: u32, data: &[u8]) -> Vec<u8> {
        let mut payload = vec![103];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&(data.len() as u32).to_be_bytes());
        payload.extend_from_slice(data);
        self.build_packet(&payload)
    }

    pub fn build_attrs(&self, is_dir: bool, size: u64) -> Vec<u8> {
        let mut attrs = Vec::new();
        let flags: u32 = 0x00000001 | 0x00000002 | 0x00000004 | 0x00000008;
        attrs.extend_from_slice(&flags.to_be_bytes());
        attrs.extend_from_slice(&size.to_be_bytes());
        let uid: u32 = 1000;
        let gid: u32 = 1000;
        attrs.extend_from_slice(&uid.to_be_bytes());
        attrs.extend_from_slice(&gid.to_be_bytes());
        let permissions = if is_dir { 0o40755u32 } else { 0o100644u32 };
        attrs.extend_from_slice(&permissions.to_be_bytes());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        attrs.extend_from_slice(&now.to_be_bytes());
        attrs.extend_from_slice(&now.to_be_bytes());
        attrs
    }

    pub fn build_attrs_extended(&self, metadata: &std::fs::Metadata, is_dir: bool) -> Vec<u8> {
        use std::os::windows::fs::MetadataExt;

        let mut attrs = Vec::new();

        let mut flags: u32 = 0x00000001 | 0x00000002 | 0x00000004 | 0x00000008;

        attrs.extend_from_slice(&flags.to_be_bytes());

        attrs.extend_from_slice(&metadata.len().to_be_bytes());

        let uid: u32 = 1000;
        let gid: u32 = 1000;
        attrs.extend_from_slice(&uid.to_be_bytes());
        attrs.extend_from_slice(&gid.to_be_bytes());

        let permissions = if is_dir { 0o40755u32 } else { 0o100644u32 };
        attrs.extend_from_slice(&permissions.to_be_bytes());

        let atime = metadata
            .accessed()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);

        attrs.extend_from_slice(&atime.to_be_bytes());
        attrs.extend_from_slice(&mtime.to_be_bytes());

        #[cfg(windows)]
        {
            if let Some(ctime) = metadata
                .creation_time()
                .checked_sub(116444736000000000)
                .map(|ns100| ns100 / 10_000_000)
            {
                flags |= 0x80000000;

                attrs[0..4].copy_from_slice(&flags.to_be_bytes());

                attrs.extend_from_slice(&1u32.to_be_bytes());

                let ext_name = "createtime";
                attrs.extend_from_slice(&(ext_name.len() as u32).to_be_bytes());
                attrs.extend_from_slice(ext_name.as_bytes());

                attrs.extend_from_slice(&8u32.to_be_bytes());
                attrs.extend_from_slice(&ctime.to_be_bytes());
            }
        }

        attrs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::get_program_data_path;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn create_test_sftp_state() -> SftpState {
        SftpState::new(
            "C:\\sftp_test".to_string(),
            Some("testuser".to_string()),
            Arc::new(Mutex::new(UserManager::new())),
            Arc::new(QuotaManager::new(&get_program_data_path())),
            "127.0.0.1".to_string(),
            false,
        )
    }

    #[test]
    fn test_sftp_state_initial() {
        let state = create_test_sftp_state();
        assert_eq!(state.home_dir, "C:\\sftp_test");
        assert_eq!(state.cwd, "C:\\sftp_test");
        assert_eq!(state.username, Some("testuser".to_string()));
        assert!(state.handles.is_empty());
        assert_eq!(state.next_handle_id, 0);
        assert_eq!(state.sftp_version, 3);
        assert!(state.buffer.is_empty());
        assert!(state.locked_files.is_empty());
        assert_eq!(state.client_ip, "127.0.0.1");
    }

    #[test]
    fn test_parse_u32_valid() {
        let state = create_test_sftp_state();
        let data: Vec<u8> = 0x12345678u32.to_be_bytes().to_vec();
        let result = state.parse_u32(&data, 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0x12345678);
    }

    #[test]
    fn test_parse_u32_insufficient_data() {
        let state = create_test_sftp_state();
        let data = [0x00, 0x01, 0x02];
        let result = state.parse_u32(&data, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_u32_offset_out_of_bounds() {
        let state = create_test_sftp_state();
        let data = [0x00; 8];
        let result = state.parse_u32(&data, 6);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_u64_valid() {
        let state = create_test_sftp_state();
        let data: Vec<u8> = 0x0102030405060708u64.to_be_bytes().to_vec();
        let result = state.parse_u64(&data, 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0x0102030405060708);
    }

    #[test]
    fn test_parse_u64_insufficient_data() {
        let state = create_test_sftp_state();
        let data = [0x00; 4];
        let result = state.parse_u64(&data, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_string_valid() {
        let state = create_test_sftp_state();
        let mut data = Vec::new();
        let test_str = "hello";
        data.extend_from_slice(&(test_str.len() as u32).to_be_bytes());
        data.extend_from_slice(test_str.as_bytes());
        let result = state.parse_string(&data, 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn test_parse_string_empty() {
        let state = create_test_sftp_state();
        let data: Vec<u8> = 0u32.to_be_bytes().to_vec();
        let result = state.parse_string(&data, 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_parse_string_truncated() {
        let state = create_test_sftp_state();
        let mut data = Vec::new();
        data.extend_from_slice(&100u32.to_be_bytes());
        data.extend_from_slice(b"short");
        let result = state.parse_string(&data, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_string_with_len_valid() {
        let state = create_test_sftp_state();
        let mut data = Vec::new();
        let test_str = "world";
        data.extend_from_slice(&(test_str.len() as u32).to_be_bytes());
        data.extend_from_slice(test_str.as_bytes());
        let result = state.parse_string_with_len(&data, 0);
        assert!(result.is_ok());
        let (s, len) = result.unwrap();
        assert_eq!(s, "world");
        assert_eq!(len, 5);
    }

    #[test]
    fn test_generate_handle_sequential() {
        let mut state = create_test_sftp_state();
        let h1 = state.generate_handle();
        let h2 = state.generate_handle();
        let h3 = state.generate_handle();
        assert_eq!(h1, "h00000000");
        assert_eq!(h2, "h00000001");
        assert_eq!(h3, "h00000002");
    }

    #[test]
    fn test_generate_handle_wrapping() {
        let mut state = create_test_sftp_state();
        state.next_handle_id = u32::MAX;
        let h1 = state.generate_handle();
        assert_eq!(h1, "hffffffff");
        let h2 = state.generate_handle();
        assert_eq!(h2, "h00000000");
    }

    #[test]
    fn test_build_packet() {
        let state = create_test_sftp_state();
        let payload = [1u8, 2, 3, 4];
        let packet = state.build_packet(&payload);
        assert_eq!(packet.len(), 8);
        assert_eq!(
            u32::from_be_bytes([packet[0], packet[1], packet[2], packet[3]]),
            4
        );
        assert_eq!(&packet[4..], &[1, 2, 3, 4]);
    }

    #[test]
    fn test_build_version_packet() {
        let state = create_test_sftp_state();
        let packet = state.build_version_packet(3);
        assert!(packet.len() > 4);
        let payload_len = u32::from_be_bytes([packet[0], packet[1], packet[2], packet[3]]) as usize;
        assert_eq!(packet[4], 2);
        let version = u32::from_be_bytes([packet[5], packet[6], packet[7], packet[8]]);
        assert_eq!(version, 3);
        tracing::trace!("SFTP init: payload_len={}", payload_len);
    }

    #[test]
    fn test_build_status_packet() {
        let state = create_test_sftp_state();
        let packet = state.build_status_packet(1, 0, "OK", "");
        assert!(packet.len() > 4);
        let payload = &packet[4..];
        assert_eq!(payload[0], 101);
    }

    #[test]
    fn test_build_handle_packet() {
        let state = create_test_sftp_state();
        let packet = state.build_handle_packet(42, "h00000001");
        assert!(packet.len() > 4);
        let payload = &packet[4..];
        assert_eq!(payload[0], 102);
    }

    #[test]
    fn test_build_data_packet() {
        let state = create_test_sftp_state();
        let packet = state.build_data_packet(7, b"test data");
        assert!(packet.len() > 4);
        let payload = &packet[4..];
        assert_eq!(payload[0], 103);
    }

    #[test]
    fn test_build_attrs() {
        let state = create_test_sftp_state();
        let attrs = state.build_attrs(false, 1024);
        assert!(!attrs.is_empty());
        let flags = u32::from_be_bytes([attrs[0], attrs[1], attrs[2], attrs[3]]);
        assert_ne!(flags, 0);
    }

    #[test]
    fn test_build_attrs_dir() {
        let state = create_test_sftp_state();
        let attrs = state.build_attrs(true, 0);
        let flags = u32::from_be_bytes([attrs[0], attrs[1], attrs[2], attrs[3]]);
        assert_ne!(flags, 0);
    }
}
