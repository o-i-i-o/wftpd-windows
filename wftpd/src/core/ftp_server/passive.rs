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
fn getrandom_u32() -> u32 {
    let mut buf = [0u8; 4];
    getrandom::fill(&mut buf).expect("Failed to generate random bytes");
    u32::from_be_bytes(buf)
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

        // Calculate available port range size
        let range_size = (port_max - port_min + 1) as usize;
        if range_size == 0 {
            anyhow::bail!("Invalid port range: {}-{}", port_min, port_max);
        }

        // Generate random start position to avoid race conditions and predictability from sequential search
        let start_offset = getrandom_u32() as usize % range_size;

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

    pub async fn accept_with_validation(&mut self, port: u16) -> Result<tokio::net::TcpStream> {
        let info = self
            .listeners
            .get_mut(&port)
            .ok_or_else(|| anyhow::anyhow!("No listener found for port {}", port))?;

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

            // 尝试移除 UPnP 端口映射
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

        // EPSV doesn't need to return IP, but we can log for debugging
        let _response_ip = if let Some(override_ip) = &config.passive_ip_override {
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

        Ok(passive_port)
    }
}

impl Default for PassiveManager {
    fn default() -> Self {
        Self::new(None)
    }
}
