//! Pooled passive port listener
//!
//! Pre-binds all passive ports at startup for high-concurrency scenarios

use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use super::upnp_manager::UpnpManager;

pub struct PooledPort {
    pub listener: Arc<Mutex<TcpListener>>,
    pub port: u16,
    pub created_at: Instant,
}

pub struct PooledListener {
    ports: Arc<Mutex<HashMap<u16, PooledPort>>>,
    upnp_manager: Option<Arc<UpnpManager>>,
    bind_ip: String,
}

impl PooledListener {
    pub fn new(bind_ip: &str, upnp_manager: Option<Arc<UpnpManager>>) -> Self {
        PooledListener {
            ports: Arc::new(Mutex::new(HashMap::new())),
            upnp_manager,
            bind_ip: bind_ip.to_string(),
        }
    }

    pub async fn initialize(&self, port_range: (u16, u16)) -> Result<usize> {
        let mut bound_count = 0;
        let (port_min, port_max) = port_range;

        for port in port_min..=port_max {
            let addr: SocketAddr = format!("{}:{}", self.bind_ip, port)
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid address: {}", e))?;

            match TcpListener::bind(addr).await {
                Ok(listener) => {
                    let pooled_port = PooledPort {
                        listener: Arc::new(Mutex::new(listener)),
                        port,
                        created_at: Instant::now(),
                    };
                    self.ports.lock().await.insert(port, pooled_port);
                    bound_count += 1;
                }
                Err(e) => {
                    tracing::debug!("Failed to bind port {}: {}", port, e);
                }
            }
        }

        tracing::info!(
            "Pooled listener initialized: {}/{} ports bound on {}",
            bound_count,
            port_max - port_min + 1,
            self.bind_ip
        );

        Ok(bound_count)
    }

    pub async fn acquire(&self, client_ip: &str) -> Option<(u16, Arc<Mutex<TcpListener>>)> {
        let ports = self.ports.lock().await;

        if let Some((port, pooled)) = ports.iter().next() {
            tracing::debug!("Pooled port {} acquired for client {}", port, client_ip);
            return Some((*port, Arc::clone(&pooled.listener)));
        }

        None
    }

    pub async fn release(&self, port: u16) {
        tracing::debug!("Pooled port {} released", port);
    }

    pub async fn available_count(&self) -> usize {
        self.ports.lock().await.len()
    }

    pub async fn shutdown(&self) {
        let mut ports = self.ports.lock().await;
        let count = ports.len();
        ports.clear();
        tracing::info!("Pooled listener shutdown: {} ports released", count);
    }
}

pub struct PooledPassiveManager {
    pooled: Arc<PooledListener>,
    active_connections: Arc<Mutex<HashMap<u16, (Instant, String)>>>,
}

impl PooledPassiveManager {
    pub fn new(pooled: Arc<PooledListener>) -> Self {
        PooledPassiveManager {
            pooled,
            active_connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn get_listener(&self, client_ip: &str) -> Option<(u16, Arc<Mutex<TcpListener>>)> {
        let result = self.pooled.acquire(client_ip).await?;

        let mut active = self.active_connections.lock().await;
        active.insert(result.0, (Instant::now(), client_ip.to_string()));

        Some(result)
    }

    pub async fn accept_connection(
        &self,
        port: u16,
        listener: &Arc<Mutex<TcpListener>>,
        timeout_secs: u64,
    ) -> Result<tokio::net::TcpStream> {
        let active = self.active_connections.lock().await;

        let (_, expected_client) = active
            .get(&port)
            .ok_or_else(|| anyhow::anyhow!("No active listener for port {}", port))?;

        let expected_client = expected_client.clone();
        drop(active);

        let listener_guard = listener.lock().await;

        let accept_result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            listener_guard.accept(),
        )
        .await;

        match accept_result {
            Ok(Ok((stream, peer_addr))) => {
                let peer_ip = peer_addr.ip();

                if Self::ip_matches_client(&peer_ip, &expected_client) {
                    tracing::debug!(
                        "Pooled connection accepted from {} on port {}",
                        peer_ip,
                        port
                    );
                    return Ok(stream);
                }

                tracing::warn!(
                    "Pooled connection rejected from {} - expected {}",
                    peer_ip,
                    expected_client
                );
                anyhow::bail!("IP mismatch for pooled connection");
            }
            Ok(Err(e)) => anyhow::bail!("Accept error: {}", e),
            Err(_) => anyhow::bail!("Accept timeout on port {}", port),
        }
    }

    fn ip_matches_client(peer_ip: &std::net::IpAddr, expected: &str) -> bool {
        if let Ok(expected_ip) = expected.parse::<std::net::IpAddr>() {
            if peer_ip == &expected_ip {
                return true;
            }
            if let (std::net::IpAddr::V4(peer_v4), std::net::IpAddr::V6(expected_v6)) =
                (peer_ip, expected_ip)
                && let Some(mapped) = expected_v6.to_ipv4_mapped()
            {
                return peer_v4 == &mapped;
            }
            if let (std::net::IpAddr::V6(peer_v6), std::net::IpAddr::V4(expected_v4)) =
                (peer_ip, expected_ip)
                && let Some(mapped) = peer_v6.to_ipv4_mapped()
            {
                return mapped == expected_v4;
            }
        }
        false
    }

    pub async fn release_port(&self, port: u16) {
        self.active_connections.lock().await.remove(&port);
        self.pooled.release(port).await;
    }

    pub async fn cleanup_expired(&self, timeout_secs: u64) {
        let mut active = self.active_connections.lock().await;
        let now = Instant::now();

        let expired: Vec<u16> = active
            .iter()
            .filter(|(_, (created_at, _))| now.duration_since(*created_at).as_secs() > timeout_secs)
            .map(|(&port, _)| port)
            .collect();

        for port in expired {
            active.remove(&port);
            self.pooled.release(port).await;
            tracing::debug!("Cleaned up expired pooled listener on port {}", port);
        }
    }
}
