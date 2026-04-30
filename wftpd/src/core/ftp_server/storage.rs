//! libunftp storage backend wrapper with quota support
//!
//! Wraps Filesystem storage backend with quota management
//! Permission checking is handled by RestrictingVfs from unftp-sbe-restrict
//! Uses atomic operations to prevent race conditions

use async_trait::async_trait;
use parking_lot::Mutex;
use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncRead;
use unftp_core::storage::{Fileinfo, Metadata, StorageBackend};
use unftp_sbe_fs::Filesystem;

use crate::core::quota::QuotaManager;
use crate::core::rate_limiter::RateLimitedReader;
use crate::core::users::UserManager;

use super::auth::WftpdUser;

const ESTIMATED_MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;

#[derive(Debug)]
pub struct QuotaFilesystem {
    inner: Filesystem,
    quota_manager: Arc<QuotaManager>,
    user_manager: Arc<Mutex<UserManager>>,
}

impl QuotaFilesystem {
    pub fn new(
        inner: Filesystem,
        quota_manager: Arc<QuotaManager>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> Self {
        QuotaFilesystem {
            inner,
            quota_manager,
            user_manager,
        }
    }

    fn get_user_quota_mb(&self, username: &str) -> Option<u64> {
        let users = self.user_manager.lock();
        users
            .get_user(username)
            .and_then(|u| u.permissions.quota_mb)
    }

    fn quota_exceeded() -> unftp_core::storage::Error {
        unftp_core::storage::Error::new(
            unftp_core::storage::ErrorKind::InsufficientStorageSpaceError,
            std::io::Error::other("Quota exceeded"),
        )
    }
}

#[async_trait]
impl StorageBackend<WftpdUser> for QuotaFilesystem {
    type Metadata = <Filesystem as StorageBackend<WftpdUser>>::Metadata;

    fn supported_features(&self) -> u32 {
        <Filesystem as StorageBackend<WftpdUser>>::supported_features(&self.inner)
    }

    async fn metadata<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &WftpdUser,
        path: P,
    ) -> unftp_core::storage::Result<Self::Metadata> {
        self.inner.metadata(user, path).await
    }

    async fn list<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &WftpdUser,
        path: P,
    ) -> unftp_core::storage::Result<Vec<Fileinfo<std::path::PathBuf, Self::Metadata>>> {
        let path_str = path.as_ref().to_string_lossy();
        tracing::warn!(
            username = %user.username,
            path = %path_str,
            "[FTP-DEBUG] QuotaFilesystem::list called"
        );
        self.inner.list(user, path).await
    }

    async fn get<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &WftpdUser,
        path: P,
        start_pos: u64,
    ) -> unftp_core::storage::Result<Box<dyn AsyncRead + Send + Sync + Unpin>> {
        let path_str = path.as_ref().to_string_lossy();
        tracing::warn!(
            username = %user.username,
            path = %path_str,
            start_pos = start_pos,
            "[FTP-DEBUG] QuotaFilesystem::get called"
        );
        let reader = self.inner.get(user, path, start_pos).await?;

        if let Some(speed_limit) = user.permissions.speed_limit_kbps
            && speed_limit > 0
        {
            tracing::debug!(
                username = %user.username,
                limit_kbps = speed_limit,
                "Applying speed limit {} KB/s for download by user {}",
                speed_limit, user.username
            );
            let limited_reader = RateLimitedReader::new(reader, speed_limit);
            Ok(Box::new(limited_reader))
        } else {
            Ok(reader)
        }
    }

    async fn put<P: AsRef<Path> + Send + Debug, R: AsyncRead + Send + Sync + Unpin + 'static>(
        &self,
        user: &WftpdUser,
        input: R,
        path: P,
        start_pos: u64,
    ) -> unftp_core::storage::Result<u64> {
        let path_str = path.as_ref().to_string_lossy();
        tracing::warn!(
            username = %user.username,
            path = %path_str,
            start_pos = start_pos,
            "[FTP-DEBUG] QuotaFilesystem::put called"
        );
        let username = &user.username;
        let reserved_bytes = if let Some(quota_mb) = self.get_user_quota_mb(username) {
            let reserve_amount = ESTIMATED_MAX_FILE_SIZE;

            if !self
                .quota_manager
                .try_reserve_quota(username, reserve_amount, quota_mb)
                .map_err(|e| {
                    tracing::error!("Failed to reserve quota: {}", e);
                    unftp_core::storage::Error::new(
                        unftp_core::storage::ErrorKind::LocalError,
                        std::io::Error::other(e),
                    )
                })?
            {
                tracing::warn!(
                    username = %username,
                    action = "QUOTA_EXCEEDED",
                    "User {} has exceeded quota ({} MB)", username, quota_mb
                );
                return Err(Self::quota_exceeded());
            }

            Some(reserve_amount)
        } else {
            None
        };

        let speed_limit = user.permissions.speed_limit_kbps.filter(|&s| s > 0);
        let upload_result = if let Some(limit) = speed_limit {
            tracing::debug!(
                username = %user.username,
                limit_kbps = limit,
                "Applying speed limit {} KB/s for upload by user {}",
                limit, user.username
            );
            let limited_input = RateLimitedReader::new(input, limit);
            self.inner.put(user, limited_input, path, start_pos).await
        } else {
            self.inner.put(user, input, path, start_pos).await
        };

        match upload_result {
            Ok(bytes_written) => {
                if let Some(reserved) = reserved_bytes
                    && let Err(e) =
                        self.quota_manager
                            .commit_usage(username, reserved, bytes_written)
                {
                    tracing::error!("Failed to commit quota: {}", e);
                }
                Ok(bytes_written)
            }
            Err(e) => {
                if let Some(reserved) = reserved_bytes
                    && let Err(rollback_err) =
                        self.quota_manager.rollback_reservation(username, reserved)
                {
                    tracing::error!("Failed to rollback quota: {}", rollback_err);
                }
                Err(e)
            }
        }
    }

    async fn del<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &WftpdUser,
        path: P,
    ) -> unftp_core::storage::Result<()> {
        let username = &user.username;
        let metadata = self.inner.metadata(user, path.as_ref()).await?;
        let file_size = metadata.len();

        self.inner.del(user, path).await?;

        if file_size > 0
            && let Err(e) = self.quota_manager.subtract_usage(username, file_size)
        {
            tracing::error!("Failed to update quota after delete: {}", e);
        }

        Ok(())
    }

    async fn mkd<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &WftpdUser,
        path: P,
    ) -> unftp_core::storage::Result<()> {
        let path_str = path.as_ref().to_string_lossy();
        tracing::warn!(
            username = %user.username,
            path = %path_str,
            "[FTP-DEBUG] QuotaFilesystem::mkd called"
        );
        self.inner.mkd(user, path).await
    }

    async fn rename<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &WftpdUser,
        from: P,
        to: P,
    ) -> unftp_core::storage::Result<()> {
        self.inner.rename(user, from, to).await
    }

    async fn rmd<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &WftpdUser,
        path: P,
    ) -> unftp_core::storage::Result<()> {
        let username = &user.username;
        let path_ref = path.as_ref();
        let total_size = self.calculate_dir_size(path_ref).await?;

        self.inner.rmd(user, path).await?;

        if total_size > 0
            && let Err(e) = self.quota_manager.subtract_usage(username, total_size)
        {
            tracing::error!("Failed to update quota after rmdir: {}", e);
        }

        Ok(())
    }

    async fn cwd<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &WftpdUser,
        path: P,
    ) -> unftp_core::storage::Result<()> {
        self.inner.cwd(user, path).await
    }
}

impl QuotaFilesystem {
    async fn calculate_dir_size(&self, path: &Path) -> unftp_core::storage::Result<u64> {
        let path = path.to_path_buf();
        let size = tokio::task::spawn_blocking(move || {
            let mut total_size = 0u64;
            if path.is_dir()
                && let Ok(entries) = std::fs::read_dir(&path)
            {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        if let Ok(sub_size) = Self::calculate_dir_size_sync(&entry_path) {
                            total_size = total_size.saturating_add(sub_size);
                        }
                    } else if let Ok(metadata) = entry.metadata() {
                        total_size = total_size.saturating_add(metadata.len());
                    }
                }
            }
            total_size
        })
        .await
        .map_err(|e| {
            unftp_core::storage::Error::new(
                unftp_core::storage::ErrorKind::LocalError,
                std::io::Error::other(e),
            )
        })?;

        Ok(size)
    }

    fn calculate_dir_size_sync(path: &Path) -> std::io::Result<u64> {
        let mut total_size = 0u64;
        if path.is_dir() {
            let entries = std::fs::read_dir(path)?;
            for entry in entries {
                let entry = entry?;
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    total_size =
                        total_size.saturating_add(Self::calculate_dir_size_sync(&entry_path)?);
                } else {
                    total_size = total_size.saturating_add(entry.metadata()?.len());
                }
            }
        }
        Ok(total_size)
    }
}
