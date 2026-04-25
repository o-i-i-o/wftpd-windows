//! FTP passive mode port manager
//!
//! Manages port allocation and lifecycle for passive mode data connections

use anyhow::Result;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;

use super::session_ip::{find_masq_ip, resolve_ip_for_pasv};
use super::upnp_manager::UpnpManager;

/// Get random u32 value (using getrandom crate)
fn getrandom_u32() -> anyhow::Result<u32> {
    let mut buf = [0u8; 4];
    getrandom::fill(&mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

pub struct PassiveListenerInfo {
    pub listener: TcpListener,
    pub created_at: Instant,
    pub client_ip: String,
}

pub struct PassiveManager {
    listeners: HashMap<u16, PassiveListenerInfo>,
    upnp_manager: Option<Arc<UpnpManager>>,
}

pub struct PasvConfig {
    pub client_ip: String,
    pub server_local_ip: String,
    pub bind_ip: String,
    pub port_range: (u16, u16),
    pub masquerade_address: Option<String>,
    pub passive_ip_override: Option<String>,
    pub masquerade_map: HashMap<String, String>,
    pub listener_timeout_secs: u64,
}

impl PassiveManager {
    pub fn new(upnp_manager: Option<Arc<UpnpManager>>) -> Self {
        PassiveManager {
            listeners: HashMap::new(),
            upnp_manager,
        }
    }

    pub fn cleanup_expired(&mut self, timeout_secs: u64) {
        let now = Instant::now();
        let expired: Vec<u16> = self
            .listeners
            .iter()
            .filter(|(_, info)| now.duration_since(info.created_at).as_secs() > timeout_secs)
            .map(|(&port, _)| port)
            .collect();

        for port in expired {
            if self.listeners.remove(&port).is_some() {
                tracing::debug!(
                    "Passive listener on port {} cleaned up (expired after {}s)",
                    port,
                    timeout_secs
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

        if port_max < port_min {
            anyhow::bail!("Invalid port range: {}-{} (max < min)", port_min, port_max);
        }

        let range_size = (port_max - port_min + 1) as usize;
        if range_size == 0 {
            anyhow::bail!("Invalid port range: {}-{}", port_min, port_max);
        }

        // Generate random start position to avoid race conditions and predictability from sequential search
        let start_offset = getrandom_u32()? as usize % range_size;

        // Try the entire range at most once
        for i in 0..range_size {
            let offset = (start_offset + i) % range_size;
            let port = port_min + offset as u16;

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

                    // Try to add UPnP port mapping (with error handling)
                    if let Some(upnp) = &self.upnp_manager {
                        let internal_addr = SocketAddrV4::new(
                            actual_bind_ip
                                .parse()
                                .unwrap_or(std::net::Ipv4Addr::UNSPECIFIED),
                            port,
                        );
                        let upnp_clone = Arc::clone(upnp);
                        tokio::spawn(async move {
                            match upnp_clone
                                .add_port_mapping(internal_addr, 3600, "ftp-passive")
                                .await
                            {
                                Ok(_) => {
                                    tracing::debug!("UPnP port mapping added for port {}", port)
                                }
                                Err(e) => tracing::warn!(
                                    "Failed to add UPnP port mapping for port {}: {}",
                                    port,
                                    e
                                ),
                            }
                        });
                    }

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

    pub async fn accept_with_validation(
        &mut self,
        port: u16,
        timeout_secs: u64,
    ) -> Result<tokio::net::TcpStream> {
        let info = self
            .listeners
            .get_mut(&port)
            .ok_or_else(|| anyhow::anyhow!("No listener found for port {}", port))?;

        let created_at = info.created_at;
        let expected_client_ip = info.client_ip.clone();

        loop {
            let elapsed = created_at.elapsed();
            if elapsed.as_secs() > timeout_secs {
                anyhow::bail!(
                    "Passive listener on port {} timed out after {}s",
                    port,
                    timeout_secs
                );
            }
            let remaining = std::time::Duration::from_secs(timeout_secs) - elapsed;

            let accept_result = tokio::time::timeout(remaining, info.listener.accept()).await;
            let (stream, peer_addr) = match accept_result {
                Ok(Ok(result)) => result,
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => {
                    anyhow::bail!(
                        "Passive listener on port {} timed out after {}s",
                        port,
                        timeout_secs
                    );
                }
            };
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

            // Attempt to remove UPnP port mapping
            if let Some(upnp) = &self.upnp_manager {
                let upnp_clone = Arc::clone(upnp);
                tokio::spawn(async move {
                    let _ = upnp_clone
                        .remove_port_mapping(port, igd_next::PortMappingProtocol::TCP)
                        .await;
                });
            }
            true
        } else {
            tracing::warn!(
                "Attempted to remove non-existent passive listener on port {}",
                port
            );
            false
        }
    }

    /// Handle PASV command
    pub async fn handle_pasv(&mut self, config: &PasvConfig) -> Result<(u16, String)> {
        let client_ip_addr: IpAddr = config
            .client_ip
            .parse()
            .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));
        if client_ip_addr.is_ipv6() {
            return Err(anyhow::anyhow!("Use EPSV for IPv6 connections"));
        }

        let actual_bind_ip = if config.bind_ip == "::" {
            "0.0.0.0"
        } else {
            &config.bind_ip
        };

        let passive_port = self
            .try_bind_port(
                config.port_range.0,
                config.port_range.1,
                actual_bind_ip,
                &config.client_ip,
            )
            .await?;

        let response_ip = if let Some(override_ip) = &config.passive_ip_override {
            if !override_ip.is_empty() {
                resolve_ip_for_pasv(
                    override_ip.clone(),
                    &config.client_ip,
                    &config.server_local_ip,
                )
                .await
            } else {
                find_masq_ip(
                    &config.masquerade_map,
                    &config.masquerade_address,
                    &config.server_local_ip,
                    &config.client_ip,
                )
                .await
            }
        } else {
            find_masq_ip(
                &config.masquerade_map,
                &config.masquerade_address,
                &config.server_local_ip,
                &config.client_ip,
            )
            .await
        };

        Ok((passive_port, response_ip))
    }

    /// Handle EPSV command
    pub async fn handle_epsv(&mut self, config: &PasvConfig) -> Result<u16> {
        let passive_port = self
            .try_bind_port(
                config.port_range.0,
                config.port_range.1,
                &config.bind_ip,
                &config.client_ip,
            )
            .await?;

        Ok(passive_port)
    }
}

impl Default for PassiveManager {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ip_matches_client_exact_ipv4() {
        let peer: IpAddr = "192.168.1.100".parse().unwrap();
        assert!(PassiveManager::ip_matches_client(&peer, "192.168.1.100"));
    }

    #[test]
    fn test_ip_matches_client_mismatched_ipv4() {
        let peer: IpAddr = "192.168.1.100".parse().unwrap();
        assert!(!PassiveManager::ip_matches_client(&peer, "10.0.0.1"));
    }

    #[test]
    fn test_ip_matches_client_ipv4_mapped_to_ipv6() {
        let peer: IpAddr = "192.168.1.100".parse().unwrap();
        let expected = "::ffff:192.168.1.100";
        assert!(PassiveManager::ip_matches_client(&peer, expected));
    }

    #[test]
    fn test_ip_matches_client_ipv6_mapped_to_ipv4() {
        let peer: IpAddr = "::ffff:192.168.1.100".parse().unwrap();
        assert!(PassiveManager::ip_matches_client(&peer, "192.168.1.100"));
    }

    #[test]
    fn test_ip_matches_client_ipv6_exact() {
        let peer: IpAddr = "::1".parse().unwrap();
        assert!(PassiveManager::ip_matches_client(&peer, "::1"));
    }

    #[test]
    fn test_ip_matches_client_ipv6_mismatch() {
        let peer: IpAddr = "::1".parse().unwrap();
        assert!(!PassiveManager::ip_matches_client(&peer, "::2"));
    }

    #[test]
    fn test_ip_matches_client_invalid_expected() {
        let peer: IpAddr = "192.168.1.100".parse().unwrap();
        assert!(!PassiveManager::ip_matches_client(&peer, "not-an-ip"));
    }

    #[tokio::test]
    async fn test_passive_manager_new() {
        let mgr = PassiveManager::new(None);
        assert!(mgr.listeners.is_empty());
    }

    #[tokio::test]
    async fn test_passive_manager_default() {
        let mgr = PassiveManager::default();
        assert!(mgr.listeners.is_empty());
    }

    #[tokio::test]
    async fn test_try_bind_port_success() {
        let mut mgr = PassiveManager::new(None);
        let port = mgr
            .try_bind_port(50000, 50100, "127.0.0.1", "127.0.0.1")
            .await;
        assert!(port.is_ok());
        let p = port.unwrap();
        assert!((50000..=50100).contains(&p));
        assert!(mgr.listeners.contains_key(&p));
    }

    #[tokio::test]
    async fn test_try_bind_port_same_port_twice() {
        let mut mgr = PassiveManager::new(None);
        let port1 = mgr
            .try_bind_port(50200, 50300, "127.0.0.1", "127.0.0.1")
            .await;
        assert!(port1.is_ok());
        let port2 = mgr
            .try_bind_port(50200, 50300, "127.0.0.1", "127.0.0.1")
            .await;
        assert!(port2.is_ok());
        assert_ne!(port1.unwrap(), port2.unwrap());
    }

    #[tokio::test]
    async fn test_remove_listener() {
        let mut mgr = PassiveManager::new(None);
        let port = mgr
            .try_bind_port(50300, 50400, "127.0.0.1", "127.0.0.1")
            .await
            .unwrap();
        assert!(mgr.listeners.contains_key(&port));
        assert!(mgr.remove_listener(port));
        assert!(!mgr.listeners.contains_key(&port));
    }

    #[tokio::test]
    async fn test_remove_nonexistent_listener() {
        let mut mgr = PassiveManager::new(None);
        assert!(!mgr.remove_listener(65535));
    }

    #[tokio::test]
    async fn test_cleanup_expired() {
        let mut mgr = PassiveManager::new(None);
        let port = mgr
            .try_bind_port(50400, 50500, "127.0.0.1", "127.0.0.1")
            .await
            .unwrap();

        if let Some(info) = mgr.listeners.get_mut(&port) {
            info.created_at = std::time::Instant::now() - std::time::Duration::from_millis(1);
        }

        mgr.cleanup_expired(0);
        assert!(!mgr.listeners.contains_key(&port));
    }

    #[tokio::test]
    async fn test_cleanup_not_expired() {
        let mut mgr = PassiveManager::new(None);
        let port = mgr
            .try_bind_port(50500, 50600, "127.0.0.1", "127.0.0.1")
            .await
            .unwrap();

        mgr.cleanup_expired(3600);
        assert!(mgr.listeners.contains_key(&port));
    }

    #[tokio::test]
    async fn test_accept_with_validation_timeout() {
        let mut mgr = PassiveManager::new(None);
        let port = mgr
            .try_bind_port(50600, 50700, "127.0.0.1", "127.0.0.1")
            .await
            .unwrap();

        let result = mgr.accept_with_validation(port, 1).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_accept_with_validation_no_listener() {
        let mut mgr = PassiveManager::new(None);
        let result = mgr.accept_with_validation(65535, 30).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pasv_config_ipv6_rejected() {
        let mut mgr = PassiveManager::new(None);
        let config = PasvConfig {
            client_ip: "::1".to_string(),
            server_local_ip: "::1".to_string(),
            bind_ip: "::".to_string(),
            port_range: (50700, 50800),
            masquerade_address: None,
            passive_ip_override: None,
            masquerade_map: HashMap::new(),
            listener_timeout_secs: 30,
        };
        let result = mgr.handle_pasv(&config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_epsv_config_ipv6_ok() {
        let mut mgr = PassiveManager::new(None);
        let config = PasvConfig {
            client_ip: "::1".to_string(),
            server_local_ip: "::1".to_string(),
            bind_ip: "::1".to_string(),
            port_range: (50800, 50900),
            masquerade_address: None,
            passive_ip_override: None,
            masquerade_map: HashMap::new(),
            listener_timeout_secs: 30,
        };
        let result = mgr.handle_epsv(&config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_try_bind_port_invalid_range() {
        let mut mgr = PassiveManager::new(None);
        let result = mgr.try_bind_port(100, 99, "127.0.0.1", "127.0.0.1").await;
        assert!(result.is_err());
    }
}
