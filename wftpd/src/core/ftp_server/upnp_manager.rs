//! UPnP/IGD 端口映射管理
//!
//! 自动在 NAT 网关上注册端口映射，增强 FTP 被动模式的 NAT 穿透能力

use anyhow::Result;
use igd_next::{PortMappingProtocol, search_gateway, Gateway};
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
            info!("UPnP/IGD 端口映射已禁用");
            return Ok(false);
        }

        match search_gateway(Default::default()) {
            Ok(gateway) => {
                info!("发现 UPnP/IGD 网关");
                *self.gateway.write().await = Some(gateway);
                Ok(true)
            }
            Err(e) => {
                warn!("未发现 UPnP/IGD 网关，将使用普通 NAT 模式：{}", e);
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
                            "UPnP 端口映射成功：外部端口 {} -> 内部 {}:{}",
                            external_port,
                            internal_addr.ip(),
                            internal_addr.port()
                        );
                        Ok(external_port)
                    }
                    Err(_) => {
                        warn!("UPnP 端口映射失败，使用内部端口 {}", internal_addr.port());
                        Ok(internal_addr.port())
                    }
                }
            }
            None => {
                Ok(internal_addr.port())
            }
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
                    info!("删除 UPnP 端口映射：{}", external_port);
                }
                Err(e) => {
                    warn!("删除 UPnP 端口映射失败：{}", e);
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
                    info!("获取外部 IP: {}", ip);
                    Some(ip.to_string())
                }
                Err(e) => {
                    warn!("获取外部 IP 失败：{}", e);
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
            if let Err(e) = self.add_port_mapping(internal_addr, 3600, &format!("port-{}", external_port)).await {
                warn!("UPnP 端口映射续期失败 (端口 {}): {}", external_port, e);
            }
        }
        Ok(())
    }
}
