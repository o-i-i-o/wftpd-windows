//! libunftp storage backend wrapper with quota support
//!
//! Wraps Filesystem storage backend with quota management

use async_trait::async_trait;
use parking_lot::Mutex;
use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncRead;
use unftp_core::auth::DefaultUser;
use unftp_core::storage::{Fileinfo, Metadata, StorageBackend};
use unftp_sbe_fs::Filesystem;

use crate::core::quota::QuotaManager;
use crate::core::users::UserManager;

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
        users.get_user(username).and_then(|u| u.permissions.quota_mb)
    }
}

#[async_trait]
impl StorageBackend<DefaultUser> for QuotaFilesystem {
    type Metadata = <Filesystem as StorageBackend<DefaultUser>>::Metadata;

    fn supported_features(&self) -> u32 {
        <Filesystem as StorageBackend<DefaultUser>>::supported_features(&self.inner)
    }

    async fn metadata<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> unftp_core::storage::Result<Self::Metadata> {
        self.inner.metadata(user, path).await
    }

    async fn list<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> unftp_core::storage::Result<Vec<Fileinfo<std::path::PathBuf, Self::Metadata>>> {
        self.inner.list(user, path).await
    }

    async fn get<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
        start_pos: u64,
    ) -> unftp_core::storage::Result<Box<dyn AsyncRead + Send + Sync + Unpin>> {
        self.inner.get(user, path, start_pos).await
    }

    async fn put<
        P: AsRef<Path> + Send + Debug,
        R: AsyncRead + Send + Sync + Unpin + 'static,
    >(
        &self,
        user: &DefaultUser,
        input: R,
        path: P,
        start_pos: u64,
    ) -> unftp_core::storage::Result<u64> {
        let username = user.to_string();

        if let Some(quota_mb) = self.get_user_quota_mb(&username) {
            let current_usage = self.quota_manager.get_usage(&username).await;
            let quota_bytes = quota_mb * 1024 * 1024;

            if current_usage >= quota_bytes {
                tracing::warn!(
                    username = %username,
                    action = "QUOTA_EXCEEDED",
                    "User {} has exceeded quota ({} MB)", username, quota_mb
                );
                return Err(unftp_core::storage::Error::new(
                    unftp_core::storage::ErrorKind::InsufficientStorageSpaceError,
                    std::io::Error::other("Quota exceeded"),
                ));
            }
        }

        let bytes_written = self.inner.put(user, input, path, start_pos).await?;

        if bytes_written > 0
            && let Err(e) = self.quota_manager.add_usage(&username, bytes_written).await
        {
            tracing::error!("Failed to update quota: {}", e);
        }

        Ok(bytes_written)
    }

    async fn del<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> unftp_core::storage::Result<()> {
        let username = user.to_string();
        let metadata = self.inner.metadata(user, path.as_ref()).await?;
        let file_size = metadata.len();

        self.inner.del(user, path).await?;

        if file_size > 0
            && let Err(e) = self.quota_manager.subtract_usage(&username, file_size).await
        {
            tracing::error!("Failed to update quota after delete: {}", e);
        }

        Ok(())
    }

    async fn mkd<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> unftp_core::storage::Result<()> {
        self.inner.mkd(user, path).await
    }

    async fn rename<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        from: P,
        to: P,
    ) -> unftp_core::storage::Result<()> {
        self.inner.rename(user, from, to).await
    }

    async fn rmd<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> unftp_core::storage::Result<()> {
        let username = user.to_string();
        let path_ref = path.as_ref();
        let total_size = self.calculate_dir_size(path_ref).await?;

        self.inner.rmd(user, path).await?;

        if total_size > 0
            && let Err(e) = self.quota_manager.subtract_usage(&username, total_size).await
        {
            tracing::error!("Failed to update quota after rmdir: {}", e);
        }

        Ok(())
    }

    async fn cwd<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
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
                    total_size = total_size.saturating_add(Self::calculate_dir_size_sync(&entry_path)?);
                } else {
                    total_size = total_size.saturating_add(entry.metadata()?.len());
                }
            }
        }
        Ok(total_size)
    }
}
