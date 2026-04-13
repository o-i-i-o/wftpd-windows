use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BanEvent {
    Banned,
    Unbanned,
}

pub type BanCallback = dyn Fn(&str, BanEvent) + Send + Sync;

struct Fail2BanState {
    failed_attempts: HashMap<String, HashSet<DateTime<Utc>>>,
    banned_ips: HashMap<String, DateTime<Utc>>,
}

pub struct Fail2BanManager {
    state: Mutex<Fail2BanState>,
    config: Mutex<Fail2BanConfig>,
    callbacks: Mutex<Vec<Arc<BanCallback>>>,
}

impl Fail2BanManager {
    pub fn new(config: Fail2BanConfig) -> Self {
        Fail2BanManager {
            state: Mutex::new(Fail2BanState {
                failed_attempts: HashMap::new(),
                banned_ips: HashMap::new(),
            }),
            config: Mutex::new(config),
            callbacks: Mutex::new(Vec::new()),
        }
    }

    pub fn register_callback(&self, callback: Arc<BanCallback>) {
        let mut callbacks = self.callbacks.lock();
        callbacks.push(callback);
    }

    pub fn update_config(&self, new_config: Fail2BanConfig) {
        let mut config = self.config.lock();
        *config = new_config;
        tracing::info!("Fail2Ban: Configuration updated");
    }

    pub fn get_config(&self) -> Fail2BanConfig {
        self.config.lock().clone()
    }

    fn trigger_callbacks(&self, ip: &str, event: BanEvent) {
        let callbacks = self.callbacks.lock();
        for callback in callbacks.iter() {
            callback(ip, event);
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
        entry.insert(now);
        entry.retain(|&time| (now - time).num_seconds() < find_time as i64);

        if entry.len() >= threshold as usize {
            let ban_until = now + chrono::Duration::seconds(ban_time as i64);
            state.banned_ips.insert(ip.to_string(), ban_until);
            state.failed_attempts.remove(ip);

            drop(state);

            tracing::warn!(
                "Fail2Ban: IP {} reached threshold ({} failures), banning for {} seconds",
                ip,
                threshold,
                ban_time
            );
            self.trigger_callbacks(ip, BanEvent::Banned);
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
            drop(state);
            self.trigger_callbacks(ip, BanEvent::Unbanned);
        }

        false
    }

    pub async fn unban_ip(&self, ip: &str) {
        let mut state = self.state.lock();
        let existed = state.banned_ips.remove(ip).is_some();
        drop(state);

        if existed {
            tracing::info!("Fail2Ban: Manually unbanned IP {}", ip);
            self.trigger_callbacks(ip, BanEvent::Unbanned);
        }
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

        let expired_bans: Vec<String> = state
            .banned_ips
            .iter()
            .filter(|&(_, ban_until)| now >= *ban_until)
            .map(|(ip, _)| ip.clone())
            .collect();

        for ip in &expired_bans {
            state.banned_ips.remove(ip);
        }

        state.banned_ips.retain(|_, &mut ban_until| now < ban_until);

        for (_, times) in state.failed_attempts.iter_mut() {
            times.retain(|&time| (now - time).num_seconds() < find_time_secs);
        }
        state.failed_attempts.retain(|_, times| !times.is_empty());

        drop(state);

        for ip in expired_bans {
            self.trigger_callbacks(&ip, BanEvent::Unbanned);
        }

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

    #[tokio::test]
    async fn test_config_hot_update() {
        let manager = Fail2BanManager::new(Fail2BanConfig::default());

        let new_config = Fail2BanConfig {
            enabled: true,
            threshold: 10,
            ban_time: 7200,
            find_time: 1200,
        };
        manager.update_config(new_config.clone());

        let current_config = manager.get_config();
        assert_eq!(current_config.enabled, true);
        assert_eq!(current_config.threshold, 10);
        assert_eq!(current_config.ban_time, 7200);
        assert_eq!(current_config.find_time, 1200);
    }

    #[tokio::test]
    async fn test_event_callback() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let manager = Fail2BanManager::new(Fail2BanConfig {
            enabled: true,
            threshold: 2,
            ban_time: 60,
            find_time: 300,
        });

        let ban_count = Arc::new(AtomicUsize::new(0));
        let unban_count = Arc::new(AtomicUsize::new(0));

        let ban_count_clone = ban_count.clone();
        let unban_count_clone = unban_count.clone();

        manager.register_callback(Arc::new(move |ip, event| match event {
            BanEvent::Banned => {
                ban_count_clone.fetch_add(1, Ordering::SeqCst);
            }
            BanEvent::Unbanned => {
                unban_count_clone.fetch_add(1, Ordering::SeqCst);
            }
        }));

        assert!(!manager.add_failure("192.168.1.2").await);
        assert!(manager.add_failure("192.168.1.2").await);

        assert_eq!(ban_count.load(Ordering::SeqCst), 1);

        manager.unban_ip("192.168.1.2").await;
        assert_eq!(unban_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_hashset_no_duplicates() {
        let manager = Fail2BanManager::new(Fail2BanConfig {
            enabled: true,
            threshold: 5,
            ban_time: 60,
            find_time: 300,
        });

        let now = Utc::now();
        {
            let mut state = manager.state.lock();
            let mut times = HashSet::new();
            times.insert(now);
            times.insert(now);
            times.insert(now);
            state
                .failed_attempts
                .insert("192.168.1.3".to_string(), times);
        }

        assert_eq!(manager.get_failure_count("192.168.1.3").await, 1);
    }
}
