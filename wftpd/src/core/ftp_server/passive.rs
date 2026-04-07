use anyhow::Result;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Instant;
use tokio::net::TcpListener;

const PASSIVE_LISTENER_TIMEOUT_SECS: u64 = 300;

pub struct PassiveListenerInfo {
    pub listener: TcpListener,
    pub created_at: Instant,
    pub client_ip: String,
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

    pub fn cleanup_expired(&mut self) {
        let now = Instant::now();
        let expired: Vec<u16> = self
            .listeners
            .iter()
            .filter(|(_, info)| {
                now.duration_since(info.created_at).as_secs() > PASSIVE_LISTENER_TIMEOUT_SECS
            })
            .map(|(&port, _)| port)
            .collect();

        for port in expired {
            if self.listeners.remove(&port).is_some() {
                tracing::debug!(
                    "Passive listener on port {} cleaned up (expired after {}s)",
                    port,
                    PASSIVE_LISTENER_TIMEOUT_SECS
                );
            }
        }
    }

    pub async fn try_bind_port(
        &mut self,
        port_min: u16,
        port_max: u16,
        bind_ip: &str,
        client_ip: &str,
    ) -> Result<u16> {
        let actual_bind_ip = if bind_ip == "0.0.0.0" || bind_ip == "::" {
            "0.0.0.0"
        } else {
            bind_ip
        };

        for port in port_min..=port_max {
            if self.listeners.contains_key(&port) {
                continue;
            }

            let addr = format!("{}:{}", actual_bind_ip, port);
            match TcpListener::bind(&addr).await {
                Ok(listener) => {
                    self.listeners.insert(
                        port,
                        PassiveListenerInfo {
                            listener,
                            created_at: Instant::now(),
                            client_ip: client_ip.to_string(),
                        },
                    );
                    tracing::debug!(
                        "Passive listener bound to {} on port {} for client {}",
                        actual_bind_ip,
                        port,
                        client_ip
                    );
                    return Ok(port);
                }
                Err(e) => {
                    tracing::debug!("Failed to bind passive port {}: {}", port, e);
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

    pub async fn accept_with_validation(&mut self, port: u16) -> Result<tokio::net::TcpStream> {
        let info = self.listeners.get_mut(&port).ok_or_else(|| {
            anyhow::anyhow!("No listener found for port {}", port)
        })?;

        let expected_client_ip = info.client_ip.clone();
        
        loop {
            let (stream, peer_addr) = info.listener.accept().await?;
            let peer_ip = peer_addr.ip();

            if Self::ip_matches_client(&peer_ip, &expected_client_ip) {
                tracing::debug!(
                    "Passive connection accepted from {} (expected: {})",
                    peer_ip,
                    expected_client_ip
                );
                return Ok(stream);
            }

            tracing::warn!(
                "Passive connection rejected from {} - expected client IP {}",
                peer_ip,
                expected_client_ip
            );
        }
    }

    fn ip_matches_client(peer_ip: &IpAddr, expected: &str) -> bool {
        if let Ok(expected_ip) = expected.parse::<IpAddr>() {
            if peer_ip == &expected_ip {
                return true;
            }
            if let (IpAddr::V4(peer_v4), IpAddr::V6(expected_v6)) = (peer_ip, expected_ip)
                && let Some(mapped) = expected_v6.to_ipv4_mapped()
                && peer_v4 == &mapped
            {
                return true;
            }
            if let (IpAddr::V6(peer_v6), IpAddr::V4(expected_v4)) = (peer_ip, expected_ip)
                && let Some(mapped) = peer_v6.to_ipv4_mapped()
                && mapped == expected_v4
            {
                return true;
            }
        }
        false
    }

    pub fn remove_listener(&mut self, port: u16) -> bool {
        if self.listeners.remove(&port).is_some() {
            tracing::debug!("Passive listener on port {} removed", port);
            true
        } else {
            tracing::warn!(
                "Attempted to remove non-existent passive listener on port {}",
                port
            );
            false
        }
    }
}

impl Default for PassiveManager {
    fn default() -> Self {
        Self::new()
    }
}
