use anyhow::Result;
use std::collections::HashMap;
use std::time::Instant;
use tokio::net::TcpListener;

/// 被动模式监听器默认超时时间（秒）
const PASSIVE_LISTENER_TIMEOUT_SECS: u64 = 300; // 5 分钟

pub struct PassiveListenerInfo {
    pub listener: TcpListener,
    pub created_at: Instant,
}

pub struct PassiveManager {
    listeners: HashMap<u16, PassiveListenerInfo>,
}

impl PassiveManager {
    pub fn new() -> Self {
        PassiveManager {
            listeners: HashMap::new(),
        }
    }

    /// 清理超时的被动监听端口（应在会话循环中定期调用）
    pub fn cleanup_expired(&mut self) {
        let now = Instant::now();
        let expired: Vec<u16> = self.listeners
            .iter()
            .filter(|(_, info)| now.duration_since(info.created_at).as_secs() > PASSIVE_LISTENER_TIMEOUT_SECS)
            .map(|(&port, _)| port)
            .collect();

        for port in expired {
            if self.listeners.remove(&port).is_some() {
                tracing::debug!("Passive listener on port {} cleaned up (expired after {}s)", port, PASSIVE_LISTENER_TIMEOUT_SECS);
            }
        }
    }

    pub async fn try_bind_port(&mut self, port_min: u16, port_max: u16, bind_ip: &str) -> Result<u16> {
        for port in port_min..=port_max {
            if self.listeners.contains_key(&port) {
                continue;
            }
            
            let addr = format!("{}:{}", bind_ip, port);
            match TcpListener::bind(&addr).await {
                Ok(listener) => {
                    self.listeners.insert(port, PassiveListenerInfo {
                        listener,
                        created_at: Instant::now(),
                    });
                    return Ok(port);
                }
                Err(_) => {
                    continue;
                }
            }
        }

        anyhow::bail!(
            "No available passive ports in range {}-{}",
            port_min,
            port_max
        )
    }

    pub fn get_listener(&mut self, port: u16) -> Option<TcpListener> {
        self.listeners.remove(&port).map(|info| info.listener)
    }

    pub fn remove_listener(&mut self, port: u16) -> bool {
        if self.listeners.remove(&port).is_some() {
            tracing::debug!("Passive listener on port {} removed", port);
            true
        } else {
            tracing::warn!("Attempted to remove non-existent passive listener on port {}", port);
            false
        }
    }

}

impl Default for PassiveManager {
    fn default() -> Self {
        Self::new()
    }
}
