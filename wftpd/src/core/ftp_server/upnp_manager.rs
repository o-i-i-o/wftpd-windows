//! UPnP/IGD port mapping management
//!
//! Automatically registers port mappings on NAT gateways to enhance FTP passive mode NAT traversal

use anyhow::Result;
use igd_next::{Gateway, PortMappingProtocol, search_gateway};
use std::net::{SocketAddr, SocketAddrV4};
use tokio::sync::RwLock;
use tracing::{info, warn};

pub struct UpnpManager {
    gateway: RwLock<Option<Gateway>>,
    enabled: bool,
}

impl UpnpManager {
    pub fn new(enabled: bool) -> Self {
        UpnpManager {
            gateway: RwLock::new(None),
            enabled,
        }
    }

    pub async fn initialize(&self) -> Result<bool> {
        if !self.enabled {
            info!("UPnP/IGD port mapping is disabled");
            return Ok(false);
        }

        match search_gateway(Default::default()) {
            Ok(gateway) => {
                info!("UPnP/IGD gateway discovered");
                *self.gateway.write().await = Some(gateway);
                Ok(true)
            }
            Err(e) => {
                warn!("UPnP/IGD gateway not found, will use normal NAT mode: {}", e);
                Ok(false)
            }
        }
    }

    pub async fn add_port_mapping(
        &self,
        internal_addr: SocketAddrV4,
        lease_duration: u32,
        service: &str,
    ) -> Result<u16> {
        if !self.enabled {
            return Ok(internal_addr.port());
        }

        let gateway_guard = self.gateway.read().await;
        match &*gateway_guard {
            Some(gateway) => {
                match gateway.add_any_port(
                    PortMappingProtocol::TCP,
                    SocketAddr::V4(internal_addr),
                    lease_duration,
                    &format!("WFTPG-{}", service),
                ) {
                    Ok(external_port) => {
                        info!(
                            "UPnP port mapping successful: external port {} -> internal {}:{}",
                            external_port,
                            internal_addr.ip(),
                            internal_addr.port()
                        );
                        Ok(external_port)
                    }
                    Err(_) => {
                        warn!("UPnP port mapping failed, using internal port {}", internal_addr.port());
                        Ok(internal_addr.port())
                    }
                }
            }
            None => Ok(internal_addr.port()),
        }
    }

    pub async fn remove_port_mapping(
        &self,
        external_port: u16,
        protocol: PortMappingProtocol,
    ) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let gateway_guard = self.gateway.read().await;
        if let Some(gateway) = &*gateway_guard {
            match gateway.remove_port(protocol, external_port) {
                Ok(()) => {
                    info!("UPnP port mapping removed: {}", external_port);
                }
                Err(e) => {
                    warn!("Failed to remove UPnP port mapping: {}", e);
                }
            }
        }
        Ok(())
    }

    pub async fn get_external_ip(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }

        let gateway_guard = self.gateway.read().await;
        match &*gateway_guard {
            Some(gateway) => match gateway.get_external_ip() {
                Ok(ip) => {
                    info!("External IP obtained: {}", ip);
                    Some(ip.to_string())
                }
                Err(e) => {
                    warn!("Failed to get external IP: {}", e);
                    None
                }
            },
            None => None,
        }
    }

    pub async fn refresh_all_mappings(&self, mappings: &[(u16, SocketAddrV4)]) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        for &(external_port, internal_addr) in mappings {
            if let Err(e) = self
                .add_port_mapping(internal_addr, 3600, &format!("port-{}", external_port))
                .await
            {
                warn!("UPnP port mapping renewal failed (port {}): {}", external_port, e);
            }
        }
        Ok(())
    }
}
