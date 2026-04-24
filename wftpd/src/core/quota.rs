//! User quota manager
//!
//! Tracks user upload and download bytes, supports file size-based quota limits

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuotaUsage {
    pub used_bytes: u64,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuotaData {
    pub users: HashMap<String, QuotaUsage>,
}

pub struct QuotaManager {
    data_path: PathBuf,
    data: Arc<Mutex<QuotaData>>,
    dirty: AtomicBool,
}

impl QuotaManager {
    pub fn new(data_dir: &Path) -> Self {
        let data_path = data_dir.join("quota.json");
        let data = Self::load_data(&data_path).unwrap_or_default();

        QuotaManager {
            data_path,
            data: Arc::new(Mutex::new(data)),
            dirty: AtomicBool::new(false),
        }
    }

    fn load_data(path: &Path) -> Result<QuotaData> {
        if !path.exists() {
            return Ok(QuotaData::default());
        }

        let content = fs::read_to_string(path)?;
        if content.trim().is_empty() {
            return Ok(QuotaData::default());
        }

        let data: QuotaData = serde_json::from_str(&content)?;
        Ok(data)
    }

    async fn save_data(&self) -> Result<()> {
        let data = self.data.lock().await;
        if let Some(parent) = self.data_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&*data)?;
        fs::write(&self.data_path, content)?;
        self.dirty.store(false, Ordering::Release);
        Ok(())
    }

    /// Flush to disk only when data is modified (for periodic calls or explicit save)
    pub async fn flush_if_dirty(&self) -> Result<()> {
        if self.dirty.load(Ordering::Acquire) {
            self.save_data().await
        } else {
            Ok(())
        }
    }

    /// Force immediate flush (ignore dirty flag)
    pub async fn force_flush(&self) -> Result<()> {
        self.save_data().await
    }

    pub async fn get_usage(&self, username: &str) -> u64 {
        let data = self.data.lock().await;
        data.users.get(username).map(|u| u.used_bytes).unwrap_or(0)
    }

    pub async fn check_quota(
        &self,
        username: &str,
        quota_mb: u64,
        additional_bytes: u64,
    ) -> Result<bool> {
        let data = self.data.lock().await;
        let used = data.users.get(username).map(|u| u.used_bytes).unwrap_or(0);

        let quota_bytes = quota_mb * 1024 * 1024;
        Ok(used.saturating_add(additional_bytes) <= quota_bytes)
    }

    pub async fn add_usage(&self, username: &str, bytes: u64) -> Result<()> {
        {
            let mut data = self.data.lock().await;
            let usage = data.users.entry(username.to_string()).or_default();
            usage.used_bytes = usage.used_bytes.saturating_add(bytes);
            usage.last_updated = chrono::Utc::now();
        }
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }

    pub async fn subtract_usage(&self, username: &str, bytes: u64) -> Result<()> {
        {
            let mut data = self.data.lock().await;
            if let Some(usage) = data.users.get_mut(username) {
                usage.used_bytes = usage.used_bytes.saturating_sub(bytes);
                usage.last_updated = chrono::Utc::now();
            }
        }
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }

    pub async fn reset_usage(&self, username: &str) -> Result<()> {
        {
            let mut data = self.data.lock().await;
            data.users.remove(username);
        }
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }

    pub async fn recalculate_usage(&self, username: &str, home_dir: &Path) -> Result<u64> {
        let home_dir = home_dir.to_path_buf();
        let total_size = tokio::task::spawn_blocking(move || Self::calculate_dir_size(&home_dir))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking error: {}", e))??;

        {
            let mut data = self.data.lock().await;
            let usage = data.users.entry(username.to_string()).or_default();
            usage.used_bytes = total_size;
            usage.last_updated = chrono::Utc::now();
        }
        self.dirty.store(true, Ordering::Release);
        Ok(total_size)
    }

    fn calculate_dir_size(path: &Path) -> Result<u64> {
        let mut total_size = 0u64;

        if path.is_file() {
            return Ok(fs::metadata(path)?.len());
        }

        if path.is_dir() {
            let entries = fs::read_dir(path)?;
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    total_size = total_size.saturating_add(Self::calculate_dir_size(&path)?);
                } else {
                    total_size = total_size.saturating_add(entry.metadata()?.len());
                }
            }
        }

        Ok(total_size)
    }

    pub async fn get_all_usage(&self) -> HashMap<String, QuotaUsage> {
        let data = self.data.lock().await;
        data.users.clone()
    }
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_quota_manager_new() {
        let dir = tempfile::tempdir().unwrap();
        let manager = QuotaManager::new(dir.path());
        let usage = manager.get_usage("testuser").await;
        assert_eq!(usage, 0);
    }

    #[tokio::test]
    async fn test_quota_manager_add_usage() {
        let dir = tempfile::tempdir().unwrap();
        let manager = QuotaManager::new(dir.path());

        manager.add_usage("testuser", 1024).await.unwrap();
        let usage = manager.get_usage("testuser").await;
        assert_eq!(usage, 1024);

        manager.add_usage("testuser", 2048).await.unwrap();
        let usage = manager.get_usage("testuser").await;
        assert_eq!(usage, 3072);
    }

    #[tokio::test]
    async fn test_quota_manager_subtract_usage() {
        let dir = tempfile::tempdir().unwrap();
        let manager = QuotaManager::new(dir.path());

        manager.add_usage("testuser", 1024).await.unwrap();
        manager.subtract_usage("testuser", 512).await.unwrap();

        let usage = manager.get_usage("testuser").await;
        assert_eq!(usage, 512);
    }

    #[tokio::test]
    async fn test_quota_manager_subtract_usage_underflow() {
        let dir = tempfile::tempdir().unwrap();
        let manager = QuotaManager::new(dir.path());

        manager.add_usage("testuser", 100).await.unwrap();
        manager.subtract_usage("testuser", 200).await.unwrap();

        let usage = manager.get_usage("testuser").await;
        assert_eq!(usage, 0);
    }

    #[tokio::test]
    async fn test_quota_manager_reset_usage() {
        let dir = tempfile::tempdir().unwrap();
        let manager = QuotaManager::new(dir.path());

        manager.add_usage("testuser", 1024).await.unwrap();
        manager.reset_usage("testuser").await.unwrap();

        let usage = manager.get_usage("testuser").await;
        assert_eq!(usage, 0);
    }

    #[tokio::test]
    async fn test_quota_manager_check_quota() {
        let dir = tempfile::tempdir().unwrap();
        let manager = QuotaManager::new(dir.path());

        manager
            .add_usage("testuser", 500 * 1024 * 1024)
            .await
            .unwrap();

        let can_upload = manager
            .check_quota("testuser", 1024, 100 * 1024 * 1024)
            .await
            .unwrap();
        assert!(can_upload);

        let cannot_upload = manager
            .check_quota("testuser", 1024, 600 * 1024 * 1024)
            .await
            .unwrap();
        assert!(!cannot_upload);
    }

    #[tokio::test]
    async fn test_quota_manager_get_all_usage() {
        let dir = tempfile::tempdir().unwrap();
        let manager = QuotaManager::new(dir.path());

        manager.add_usage("user1", 1024).await.unwrap();
        manager.add_usage("user2", 2048).await.unwrap();

        let all = manager.get_all_usage().await;
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("user1").unwrap().used_bytes, 1024);
        assert_eq!(all.get("user2").unwrap().used_bytes, 2048);
    }

    #[tokio::test]
    async fn test_quota_manager_flush_if_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let manager = QuotaManager::new(dir.path());

        manager.add_usage("testuser", 1024).await.unwrap();
        manager.flush_if_dirty().await.unwrap();
    }

    #[tokio::test]
    async fn test_quota_manager_force_flush() {
        let dir = tempfile::tempdir().unwrap();
        let manager = QuotaManager::new(dir.path());

        manager.add_usage("testuser", 1024).await.unwrap();
        manager.force_flush().await.unwrap();
    }

    #[tokio::test]
    async fn test_quota_manager_persistence() {
        let dir = tempfile::tempdir().unwrap();

        {
            let manager = QuotaManager::new(dir.path());
            manager.add_usage("testuser", 4096).await.unwrap();
            manager.force_flush().await.unwrap();
        }

        let manager2 = QuotaManager::new(dir.path());
        let usage = manager2.get_usage("testuser").await;
        assert_eq!(usage, 4096);
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.00 MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.00 GB");
    }

    #[test]
    fn test_quota_data_default() {
        let data = QuotaData::default();
        assert!(data.users.is_empty());
    }

    #[test]
    fn test_quota_usage_default() {
        let usage = QuotaUsage::default();
        assert_eq!(usage.used_bytes, 0);
    }

    #[tokio::test]
    async fn test_quota_manager_multiple_users() {
        let dir = tempfile::tempdir().unwrap();
        let manager = QuotaManager::new(dir.path());

        for i in 0..5 {
            let username = format!("user{}", i);
            manager.add_usage(&username, (i + 1) * 100).await.unwrap();
        }

        let all = manager.get_all_usage().await;
        assert_eq!(all.len(), 5);

        for i in 0..5 {
            let username = format!("user{}", i);
            assert_eq!(all.get(&username).unwrap().used_bytes, (i + 1) * 100);
        }
    }
}
