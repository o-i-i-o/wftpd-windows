//! libunftp Binder extension for UPnP port mapping
//!
//! Implements libunftp's Binder trait to add UPnP port mapping support

use async_trait::async_trait;
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
}

impl UpnpBinder {
    pub fn new(upnp_manager: Arc<UpnpManager>, local_ip: Ipv4Addr) -> Self {
        UpnpBinder {
            upnp_manager,
            local_ip,
        }
    }
}

#[async_trait]
impl Binder for UpnpBinder {
    async fn bind(
        &mut self,
        local_addr: IpAddr,
        _passive_ports: RangeInclusive<u16>,
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

        if let Err(e) = self
            .upnp_manager
            .add_port_mapping(
                SocketAddrV4::new(self.local_ip, port),
                3600,
                &format!("ftp-passive-{}", port),
            )
            .await
        {
            tracing::warn!("Failed to add UPnP port mapping for port {}: {}", port, e);
        } else {
            tracing::debug!("UPnP port mapping added for passive port {}", port);
        }

        Ok(socket)
    }
}

impl Drop for UpnpBinder {
    fn drop(&mut self) {
        tracing::debug!("UpnpBinder dropped");
    }
}

pub struct UpnpBinderBuilder {
    upnp_manager: Option<Arc<UpnpManager>>,
    local_ip: Option<Ipv4Addr>,
}

impl UpnpBinderBuilder {
    pub fn new() -> Self {
        UpnpBinderBuilder {
            upnp_manager: None,
            local_ip: None,
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

    pub fn build(self) -> Option<UpnpBinder> {
        match (self.upnp_manager, self.local_ip) {
            (Some(manager), Some(ip)) => Some(UpnpBinder::new(manager, ip)),
            _ => None,
        }
    }
}

impl Default for UpnpBinderBuilder {
    fn default() -> Self {
        Self::new()
    }
}
