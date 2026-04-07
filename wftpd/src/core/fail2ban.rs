//! Fail2Ban 风格的自动封禁管理器
//!
//! 根据失败尝试次数自动封禁 IP 地址，支持配置封禁时长和阈值



use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Fail2Ban 风格的自动封禁管理器
pub struct Fail2BanManager {
    // IP 失败记录
    failed_attempts: Arc<RwLock<HashMap<String, Vec<DateTime<Utc>>>>>,
    // 当前被封禁的 IP
    banned_ips: Arc<RwLock<HashMap<String, DateTime<Utc>>>>,
    // 配置
    config: Arc<Mutex<Fail2BanConfig>>,
}

#[derive(Debug, Clone)]
pub struct Fail2BanConfig {
    pub enabled: bool,
    pub threshold: u32,      // 失败次数阈值
    pub ban_time: u64,       // 封禁时长（秒）
    pub find_time: u64,      // 检测窗口（秒）
}

impl Default for Fail2BanConfig {
    fn default() -> Self {
        Fail2BanConfig {
            enabled: false,
            threshold: 5,
            ban_time: 3600,
            find_time: 600,
        }
    }
}

impl Fail2BanManager {
    pub fn new(config: Arc<Mutex<Fail2BanConfig>>) -> Self {
        Fail2BanManager {
            failed_attempts: Arc::new(RwLock::new(HashMap::new())),
            banned_ips: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// 记录失败尝试
    pub async fn add_failure(&self, ip: &str) -> bool {
        // 先检查是否启用（使用 parking_lot 同步锁）
        let enabled = {
            let cfg = self.config.lock();
            cfg.enabled
        };
        
        if !enabled {
            return false;
        }

        let now = Utc::now();
        let find_time_secs = 600i64; // 默认 10 分钟，避免持有 config 锁

        // 添加失败记录（使用 tokio RwLock）
        let mut attempts = self.failed_attempts.write().await;
        let entry = attempts.entry(ip.to_string()).or_insert_with(Vec::new);
        entry.push(now);

        // 清理过期记录
        entry.retain(|&time| (now - time).num_seconds() < find_time_secs);
        drop(attempts); // 提前释放锁

        // 检查阈值（重新获取 config）
        let threshold = {
            let cfg = self.config.lock();
            cfg.threshold as usize
        };

        // 再次获取锁进行检查
        let entry_len = {
            let attempts = self.failed_attempts.read().await;
            if let Some(entry) = attempts.get(ip) {
                entry.len()
            } else {
                0
            }
        };
        
        if entry_len >= threshold {
            tracing::warn!(
                "Fail2Ban: IP {} reached threshold ({} failures), banning for {} seconds",
                ip,
                entry_len,
                threshold
            );

            // 添加到封禁列表
            let ban_time = {
                let cfg = self.config.lock();
                cfg.ban_time as i64
            };
            let ban_until = now + chrono::Duration::seconds(ban_time);
            let mut banned = self.banned_ips.write().await;
            banned.insert(ip.to_string(), ban_until);

            // 清理该 IP 的失败记录
            let mut attempts_write = self.failed_attempts.write().await;
            attempts_write.remove(ip);

            return true;
        }
        
        false
    }

    /// 检查 IP 是否被封禁
    pub async fn is_banned(&self, ip: &str) -> bool {
        let now = Utc::now();
        let mut banned = self.banned_ips.write().await;

        // 检查封禁状态
        if let Some(&ban_until) = banned.get(ip) {
            if now < ban_until {
                return true;
            } else {
                // 封禁已过期，移除
                banned.remove(ip);
            }
        }

        false
    }

    /// 手动解封 IP
    pub async fn unban_ip(&self, ip: &str) {
        let mut banned = self.banned_ips.write().await;
        banned.remove(ip);
        tracing::info!("Fail2Ban: Manually unbanned IP {}", ip);
    }

    /// 重置某 IP 的失败计数
    pub async fn reset_failures(&self, ip: &str) {
        let mut attempts = self.failed_attempts.write().await;
        attempts.remove(ip);
    }

    /// 获取失败计数
    pub async fn get_failure_count(&self, ip: &str) -> usize {
        let attempts = self.failed_attempts.read().await;
        attempts.get(ip).map(|v| v.len()).unwrap_or(0)
    }

    /// 获取所有被封禁的 IP
    pub async fn get_banned_ips(&self) -> Vec<String> {
        let banned = self.banned_ips.read().await;
        banned.keys().cloned().collect()
    }

    /// 清理过期的封禁和失败记录
    pub async fn cleanup(&self) {
        let now = Utc::now();
        
        // 清理过期封禁
        {
            let mut banned = self.banned_ips.write().await;
            banned.retain(|_, &mut ban_until| now < ban_until);
        }

        // 清理过期失败记录
        {
            let find_time_secs = {
                let cfg = self.config.lock();
                cfg.find_time as i64
            };
            
            let mut attempts = self.failed_attempts.write().await;
            for (_, times) in attempts.iter_mut() {
                times.retain(|&time| (now - time).num_seconds() < find_time_secs);
            }
            attempts.retain(|_, times| !times.is_empty());
        }

        tracing::debug!("Fail2Ban: Cleanup completed");
    }

    /// 启动后台清理任务
    pub fn start_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // 每 5 分钟清理一次
            loop {
                interval.tick().await;
                self.cleanup().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fail2ban_basic() {
        let config = Arc::new(Mutex::new(Fail2BanConfig {
            enabled: true,
            threshold: 3,
            ban_time: 60,
            find_time: 300,
        }));

        let manager = Fail2BanManager::new(config);

        // 初始不应该被封禁
        assert!(!manager.is_banned("192.168.1.1").await);

        // 添加 2 次失败，不应该触发封禁
        assert!(!manager.add_failure("192.168.1.1").await);
        assert!(!manager.add_failure("192.168.1.1").await);
        assert_eq!(manager.get_failure_count("192.168.1.1").await, 2);

        // 第 3 次失败，应该触发封禁
        assert!(manager.add_failure("192.168.1.1").await);
        assert!(manager.is_banned("192.168.1.1").await);

        // 重置后应该不再封禁
        manager.reset_failures("192.168.1.1").await;
        assert_eq!(manager.get_failure_count("192.168.1.1").await, 0);
    }
}
