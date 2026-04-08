use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Fail2BanConfig {
    pub enabled: bool,
    pub threshold: u32,
    pub ban_time: u64,
    pub find_time: u64,
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

struct Fail2BanState {
    failed_attempts: HashMap<String, Vec<DateTime<Utc>>>,
    banned_ips: HashMap<String, DateTime<Utc>>,
}

pub struct Fail2BanManager {
    state: Mutex<Fail2BanState>,
    config: Mutex<Fail2BanConfig>,
}

impl Fail2BanManager {
    pub fn new(config: Fail2BanConfig) -> Self {
        Fail2BanManager {
            state: Mutex::new(Fail2BanState {
                failed_attempts: HashMap::new(),
                banned_ips: HashMap::new(),
            }),
            config: Mutex::new(config),
        }
    }

    pub async fn add_failure(&self, ip: &str) -> bool {
        let (threshold, ban_time, find_time, enabled) = {
            let cfg = self.config.lock();
            (cfg.threshold, cfg.ban_time, cfg.find_time, cfg.enabled)
        };

        if !enabled {
            return false;
        }

        let now = Utc::now();
        let mut state = self.state.lock();

        let entry = state.failed_attempts.entry(ip.to_string()).or_default();
        entry.push(now);
        entry.retain(|&time| (now - time).num_seconds() < find_time as i64);

        if entry.len() >= threshold as usize {
            let ban_until = now + chrono::Duration::seconds(ban_time as i64);
            state.banned_ips.insert(ip.to_string(), ban_until);
            state.failed_attempts.remove(ip);

            tracing::warn!(
                "Fail2Ban: IP {} reached threshold ({} failures), banning for {} seconds",
                ip,
                threshold,
                ban_time
            );
            return true;
        }

        false
    }

    pub async fn is_banned(&self, ip: &str) -> bool {
        let now = Utc::now();
        let mut state = self.state.lock();

        if let Some(&ban_until) = state.banned_ips.get(ip) {
            if now < ban_until {
                return true;
            }
            state.banned_ips.remove(ip);
        }

        false
    }

    pub async fn unban_ip(&self, ip: &str) {
        let mut state = self.state.lock();
        state.banned_ips.remove(ip);
        tracing::info!("Fail2Ban: Manually unbanned IP {}", ip);
    }

    pub async fn reset_failures(&self, ip: &str) {
        let mut state = self.state.lock();
        state.failed_attempts.remove(ip);
    }

    pub async fn get_failure_count(&self, ip: &str) -> usize {
        let state = self.state.lock();
        state.failed_attempts.get(ip).map(|v| v.len()).unwrap_or(0)
    }

    pub async fn get_banned_ips(&self) -> Vec<String> {
        let state = self.state.lock();
        state.banned_ips.keys().cloned().collect()
    }

    pub async fn cleanup(&self) {
        let now = Utc::now();
        let find_time_secs = {
            let cfg = self.config.lock();
            cfg.find_time as i64
        };

        let mut state = self.state.lock();
        state.banned_ips.retain(|_, &mut ban_until| now < ban_until);

        for (_, times) in state.failed_attempts.iter_mut() {
            times.retain(|&time| (now - time).num_seconds() < find_time_secs);
        }
        state.failed_attempts.retain(|_, times| !times.is_empty());

        tracing::debug!("Fail2Ban: Cleanup completed");
    }

    pub fn start_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
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
        let manager = Fail2BanManager::new(Fail2BanConfig {
            enabled: true,
            threshold: 3,
            ban_time: 60,
            find_time: 300,
        });

        assert!(!manager.is_banned("192.168.1.1").await);

        assert!(!manager.add_failure("192.168.1.1").await);
        assert!(!manager.add_failure("192.168.1.1").await);
        assert_eq!(manager.get_failure_count("192.168.1.1").await, 2);

        assert!(manager.add_failure("192.168.1.1").await);
        assert!(manager.is_banned("192.168.1.1").await);

        manager.reset_failures("192.168.1.1").await;
        assert_eq!(manager.get_failure_count("192.168.1.1").await, 0);
    }
}
