//! libunftp Binder extension for UPnP port mapping
//!
//! Implements libunftp's Binder trait to add UPnP port mapping support

use async_trait::async_trait;
use igd_next::PortMappingProtocol;
use libunftp::options::Binder;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::ops::RangeInclusive;
use std::sync::Arc;
use tokio::net::TcpSocket;

use super::upnp_manager::UpnpManager;

#[derive(Debug)]
pub struct UpnpBinder {
    upnp_manager: Arc<UpnpManager>,
    local_ip: Ipv4Addr,
    passive_ports: Option<RangeInclusive<u16>>,
    mapped_ports: parking_lot::Mutex<Vec<u16>>,
}

impl UpnpBinder {
    pub fn new(
        upnp_manager: Arc<UpnpManager>,
        local_ip: Ipv4Addr,
        passive_ports: Option<RangeInclusive<u16>>,
    ) -> Self {
        UpnpBinder {
            upnp_manager,
            local_ip,
            passive_ports,
            mapped_ports: parking_lot::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Binder for UpnpBinder {
    async fn bind(
        &mut self,
        local_addr: IpAddr,
        passive_ports: RangeInclusive<u16>,
    ) -> io::Result<TcpSocket> {
        let socket = match local_addr {
            IpAddr::V4(_ipv4) => TcpSocket::new_v4()?,
            IpAddr::V6(_ipv6) => TcpSocket::new_v6()?,
        };

        socket.set_reuseaddr(true)?;

        let bind_addr = SocketAddr::new(local_addr, 0);
        socket.bind(bind_addr)?;

        let bound_addr = socket.local_addr()?;
        let port = bound_addr.port();

        let effective_ports = self.passive_ports.clone().unwrap_or(passive_ports);
        if !effective_ports.contains(&port) {
            tracing::warn!(
                "Bound port {} is outside configured passive port range {:?}",
                port, effective_ports
            );
        }

        match self
            .upnp_manager
            .add_port_mapping(
                SocketAddrV4::new(self.local_ip, port),
                3600,
                &format!("ftp-passive-{}", port),
            )
            .await
        {
            Ok(external_port) => {
                tracing::debug!(
                    "UPnP port mapping added for passive port {} -> external {}",
                    port, external_port
                );
                self.mapped_ports.lock().push(external_port);
            }
            Err(e) => {
                tracing::warn!("Failed to add UPnP port mapping for port {}: {}", port, e);
            }
        }

        Ok(socket)
    }
}

impl Drop for UpnpBinder {
    fn drop(&mut self) {
        let ports: Vec<u16> = self.mapped_ports.lock().drain(..).collect();
        if ports.is_empty() {
            return;
        }
        let upnp_manager = Arc::clone(&self.upnp_manager);
        tokio::spawn(async move {
            for port in ports {
                if let Err(e) = upnp_manager
                    .remove_port_mapping(port, PortMappingProtocol::TCP)
                    .await
                {
                    tracing::warn!("Failed to remove UPnP port mapping for port {}: {}", port, e);
                }
            }
        });
    }
}

pub struct UpnpBinderBuilder {
    upnp_manager: Option<Arc<UpnpManager>>,
    local_ip: Option<Ipv4Addr>,
    passive_ports: Option<RangeInclusive<u16>>,
}

impl UpnpBinderBuilder {
    pub fn new() -> Self {
        UpnpBinderBuilder {
            upnp_manager: None,
            local_ip: None,
            passive_ports: None,
        }
    }

    pub fn upnp_manager(mut self, manager: Arc<UpnpManager>) -> Self {
        self.upnp_manager = Some(manager);
        self
    }

    pub fn local_ip(mut self, ip: Ipv4Addr) -> Self {
        self.local_ip = Some(ip);
        self
    }

    pub fn passive_ports(mut self, ports: RangeInclusive<u16>) -> Self {
        self.passive_ports = Some(ports);
        self
    }

    pub fn build(self) -> Option<UpnpBinder> {
        match (self.upnp_manager, self.local_ip) {
            (Some(manager), Some(ip)) => {
                Some(UpnpBinder::new(manager, ip, self.passive_ports))
            }
            _ => None,
        }
    }
}

impl Default for UpnpBinderBuilder {
    fn default() -> Self {
        Self::new()
    }
}
