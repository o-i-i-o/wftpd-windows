//! FTP Passive Mode Address Selection
//!
//! Handles the logic for determining the IP address to return in PASV responses.
//! Priority: UPnP external IP > Masquerade address > Connection-based IP

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassiveAddressSource {
    Upnp,
    Masquerade,
    Connection,
    Loopback,
    Private,
    Public,
}

#[derive(Debug, Clone)]
pub struct PassiveModeConfig {
    pub upnp_enabled: bool,
    pub upnp_external_ip: Option<Ipv4Addr>,
    pub masquerade_address: Option<Ipv4Addr>,
    pub bind_address: IpAddr,
}

impl Default for PassiveModeConfig {
    fn default() -> Self {
        PassiveModeConfig {
            upnp_enabled: false,
            upnp_external_ip: None,
            masquerade_address: None,
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindAddressType {
    Wildcard,
    Specific,
}

pub fn classify_bind_address(bind_addr: &IpAddr) -> BindAddressType {
    match bind_addr {
        IpAddr::V4(ip) if ip.is_unspecified() => BindAddressType::Wildcard,
        IpAddr::V6(ip) if ip.is_unspecified() => BindAddressType::Wildcard,
        _ => BindAddressType::Specific,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionSource {
    Loopback,
    PrivateNetwork,
    PublicNetwork,
}

pub fn classify_connection_source(client_ip: &IpAddr) -> ConnectionSource {
    match client_ip {
        IpAddr::V4(ip) => {
            if ip.is_loopback() {
                ConnectionSource::Loopback
            } else if is_private_ipv4(ip) {
                ConnectionSource::PrivateNetwork
            } else {
                ConnectionSource::PublicNetwork
            }
        }
        IpAddr::V6(ip) => {
            if ip.is_loopback() {
                ConnectionSource::Loopback
            } else if is_private_ipv6(ip) {
                ConnectionSource::PrivateNetwork
            } else {
                ConnectionSource::PublicNetwork
            }
        }
    }
}

fn is_private_ipv4(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    if octets[0] == 10 {
        return true;
    }
    if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31 {
        return true;
    }
    if octets[0] == 192 && octets[1] == 168 {
        return true;
    }
    false
}

fn is_private_ipv6(ip: &Ipv6Addr) -> bool {
    let segments = ip.segments();
    if segments[0] == 0xfc00 || segments[0] == 0xfd00 {
        return true;
    }
    false
}

#[derive(Debug, Clone)]
pub struct PassiveAddressResult {
    pub address: Ipv4Addr,
    pub source: PassiveAddressSource,
}

pub fn select_passive_address(config: &PassiveModeConfig, connection_ip: Ipv4Addr) -> PassiveAddressResult {
    if config.upnp_enabled {
        if let Some(upnp_ip) = config.upnp_external_ip {
            tracing::info!(
                "Passive mode: Using UPnP external IP {}",
                upnp_ip
            );
            return PassiveAddressResult {
                address: upnp_ip,
                source: PassiveAddressSource::Upnp,
            };
        }
    }

    if let Some(masq_ip) = config.masquerade_address {
        if !masq_ip.is_unspecified() {
            tracing::info!(
                "Passive mode: Using masquerade address {}",
                masq_ip
            );
            return PassiveAddressResult {
                address: masq_ip,
                source: PassiveAddressSource::Masquerade,
            };
        }
    }

    let bind_type = classify_bind_address(&config.bind_address);
    
    if bind_type == BindAddressType::Wildcard {
        let source = classify_connection_source(&IpAddr::V4(connection_ip));
        match source {
            ConnectionSource::Loopback => {
                tracing::info!(
                    "Passive mode: Wildcard bind, loopback connection -> 127.0.0.1"
                );
                PassiveAddressResult {
                    address: Ipv4Addr::new(127, 0, 0, 1),
                    source: PassiveAddressSource::Loopback,
                }
            }
            ConnectionSource::PrivateNetwork => {
                tracing::info!(
                    "Passive mode: Wildcard bind, private network connection -> {}",
                    connection_ip
                );
                PassiveAddressResult {
                    address: connection_ip,
                    source: PassiveAddressSource::Private,
                }
            }
            ConnectionSource::PublicNetwork => {
                tracing::info!(
                    "Passive mode: Wildcard bind, public network connection -> {}",
                    connection_ip
                );
                PassiveAddressResult {
                    address: connection_ip,
                    source: PassiveAddressSource::Public,
                }
            }
        }
    } else {
        match config.bind_address {
            IpAddr::V4(bind_ipv4) => {
                tracing::info!(
                    "Passive mode: Specific bind address -> {}",
                    bind_ipv4
                );
                PassiveAddressResult {
                    address: bind_ipv4,
                    source: PassiveAddressSource::Connection,
                }
            }
            IpAddr::V6(_) => {
                tracing::info!(
                    "Passive mode: IPv6 bind, falling back to connection IP {}",
                    connection_ip
                );
                PassiveAddressResult {
                    address: connection_ip,
                    source: PassiveAddressSource::Connection,
                }
            }
        }
    }
}

pub fn determine_listen_address(bind_addr: &IpAddr) -> IpAddr {
    match bind_addr {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        other => *other,
    }
}

pub fn is_wildcard_bind(bind_addr: &IpAddr) -> bool {
    classify_bind_address(bind_addr) == BindAddressType::Wildcard
}

#[derive(Debug, Clone)]
pub struct PassiveModeInfo {
    pub listen_address: IpAddr,
    pub pasv_response_ip: Ipv4Addr,
    pub address_source: PassiveAddressSource,
    pub connection_source: Option<ConnectionSource>,
}

pub fn build_passive_mode_info(
    config: &PassiveModeConfig,
    connection_ip: Ipv4Addr,
) -> PassiveModeInfo {
    let listen_address = determine_listen_address(&config.bind_address);
    let result = select_passive_address(config, connection_ip);
    let connection_source = if is_wildcard_bind(&config.bind_address) {
        Some(classify_connection_source(&IpAddr::V4(connection_ip)))
    } else {
        None
    };

    PassiveModeInfo {
        listen_address,
        pasv_response_ip: result.address,
        address_source: result.source,
        connection_source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_bind_address_wildcard_v4() {
        assert_eq!(
            classify_bind_address(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))),
            BindAddressType::Wildcard
        );
    }

    #[test]
    fn test_classify_bind_address_wildcard_v6() {
        assert_eq!(
            classify_bind_address(&IpAddr::V6(Ipv6Addr::UNSPECIFIED)),
            BindAddressType::Wildcard
        );
    }

    #[test]
    fn test_classify_bind_address_specific() {
        assert_eq!(
            classify_bind_address(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))),
            BindAddressType::Specific
        );
    }

    #[test]
    fn test_classify_connection_source_loopback() {
        assert_eq!(
            classify_connection_source(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            ConnectionSource::Loopback
        );
    }

    #[test]
    fn test_classify_connection_source_private() {
        assert_eq!(
            classify_connection_source(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))),
            ConnectionSource::PrivateNetwork
        );
        assert_eq!(
            classify_connection_source(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))),
            ConnectionSource::PrivateNetwork
        );
        assert_eq!(
            classify_connection_source(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))),
            ConnectionSource::PrivateNetwork
        );
    }

    #[test]
    fn test_classify_connection_source_public() {
        assert_eq!(
            classify_connection_source(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))),
            ConnectionSource::PublicNetwork
        );
    }

    #[test]
    fn test_select_passive_address_upnp_priority() {
        let config = PassiveModeConfig {
            upnp_enabled: true,
            upnp_external_ip: Some(Ipv4Addr::new(203, 0, 113, 50)),
            masquerade_address: Some(Ipv4Addr::new(192, 168, 1, 1)),
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        };
        let result = select_passive_address(&config, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(result.address, Ipv4Addr::new(203, 0, 113, 50));
        assert_eq!(result.source, PassiveAddressSource::Upnp);
    }

    #[test]
    fn test_select_passive_address_masq_priority() {
        let config = PassiveModeConfig {
            upnp_enabled: true,
            upnp_external_ip: None,
            masquerade_address: Some(Ipv4Addr::new(203, 0, 113, 50)),
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        };
        let result = select_passive_address(&config, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(result.address, Ipv4Addr::new(203, 0, 113, 50));
        assert_eq!(result.source, PassiveAddressSource::Masquerade);
    }

    #[test]
    fn test_select_passive_address_connection_fallback() {
        let config = PassiveModeConfig {
            upnp_enabled: false,
            upnp_external_ip: None,
            masquerade_address: None,
            bind_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)),
        };
        let result = select_passive_address(&config, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(result.address, Ipv4Addr::new(192, 168, 1, 50));
        assert_eq!(result.source, PassiveAddressSource::Connection);
    }

    #[test]
    fn test_select_passive_address_wildcard_loopback() {
        let config = PassiveModeConfig {
            upnp_enabled: false,
            upnp_external_ip: None,
            masquerade_address: None,
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        };
        let result = select_passive_address(&config, Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(result.address, Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(result.source, PassiveAddressSource::Loopback);
    }

    #[test]
    fn test_is_wildcard_bind() {
        assert!(is_wildcard_bind(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
        assert!(is_wildcard_bind(&IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
        assert!(!is_wildcard_bind(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    }
}
