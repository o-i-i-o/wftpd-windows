//! FTP Passive Mode Address Selection
//!
//! Handles the logic for determining the IP address to return in PASV responses.
//! Priority: UPnP external IP > Masquerade address > Connection-based IP
//!
//! ## IPv6 Considerations
//! - PASV (RFC 959) only supports IPv4 addresses in the response format
//! - EPSV (RFC 2428) should be used for IPv6 connections - it returns only the port
//! - When server is bound to IPv6 and client sends PASV, we need to handle this gracefully

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::core::ftp_server::ip_utils::{is_private_ipv4, is_private_ipv6};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassiveAddressSource {
    Upnp,
    Masquerade,
    BindAddress,
    Loopback,
    Private,
    Public,
    Ipv6Fallback,
}

#[derive(Debug, Clone)]
pub struct PassiveModeConfig {
    pub upnp_enabled: bool,
    pub upnp_external_ip: Option<Ipv4Addr>,
    pub masquerade_address: Option<Ipv4Addr>,
    pub bind_address: IpAddr,
    pub server_local_ips: Vec<Ipv4Addr>,
}

impl Default for PassiveModeConfig {
    fn default() -> Self {
        PassiveModeConfig {
            upnp_enabled: false,
            upnp_external_ip: None,
            masquerade_address: None,
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            server_local_ips: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindAddressType {
    Wildcard,
    SpecificIpv4,
    SpecificIpv6,
}

pub fn classify_bind_address(bind_addr: &IpAddr) -> BindAddressType {
    match bind_addr {
        IpAddr::V4(ip) if ip.is_unspecified() => BindAddressType::Wildcard,
        IpAddr::V6(ip) if ip.is_unspecified() => BindAddressType::Wildcard,
        IpAddr::V4(_) => BindAddressType::SpecificIpv4,
        IpAddr::V6(_) => BindAddressType::SpecificIpv6,
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

#[derive(Debug, Clone)]
pub struct PassiveAddressResult {
    pub address: Ipv4Addr,
    pub source: PassiveAddressSource,
    pub use_epsv_recommended: bool,
}

pub fn select_passive_address(
    config: &PassiveModeConfig,
    client_ip: IpAddr,
    connection_local_ip: Option<Ipv4Addr>,
) -> PassiveAddressResult {
    if config.upnp_enabled
        && let Some(upnp_ip) = config.upnp_external_ip
    {
        tracing::info!(
            "Passive mode: Using UPnP external IP {} (priority: UPnP)",
            upnp_ip
        );
        return PassiveAddressResult {
            address: upnp_ip,
            source: PassiveAddressSource::Upnp,
            use_epsv_recommended: false,
        };
    }

    if let Some(masq_ip) = config.masquerade_address
        && !masq_ip.is_unspecified()
    {
        tracing::info!(
            "Passive mode: Using masquerade address {} (priority: masquerade)",
            masq_ip
        );
        return PassiveAddressResult {
            address: masq_ip,
            source: PassiveAddressSource::Masquerade,
            use_epsv_recommended: false,
        };
    }

    let bind_type = classify_bind_address(&config.bind_address);

    match bind_type {
        BindAddressType::SpecificIpv4 => {
            let bind_ipv4 = match config.bind_address {
                IpAddr::V4(ip) => ip,
                _ => Ipv4Addr::new(0, 0, 0, 0),
            };
            tracing::info!(
                "Passive mode: Using specific bind address {} (priority: bind address)",
                bind_ipv4
            );
            PassiveAddressResult {
                address: bind_ipv4,
                source: PassiveAddressSource::BindAddress,
                use_epsv_recommended: false,
            }
        }
        BindAddressType::SpecificIpv6 => {
            let fallback_ip = find_ipv4_fallback(&config.server_local_ips, &client_ip);
            tracing::info!(
                "Passive mode: IPv6-only bind, using IPv4 fallback {} (IPv6 clients should use EPSV)",
                fallback_ip
            );
            PassiveAddressResult {
                address: fallback_ip,
                source: PassiveAddressSource::Ipv6Fallback,
                use_epsv_recommended: matches!(client_ip, IpAddr::V6(_)),
            }
        }
        BindAddressType::Wildcard => handle_wildcard_bind(config, &client_ip, connection_local_ip),
    }
}

fn handle_wildcard_bind(
    config: &PassiveModeConfig,
    client_ip: &IpAddr,
    connection_local_ip: Option<Ipv4Addr>,
) -> PassiveAddressResult {
    let client_source = classify_connection_source(client_ip);

    match client_source {
        ConnectionSource::Loopback => {
            tracing::info!("Passive mode: Wildcard bind, loopback connection -> 127.0.0.1");
            PassiveAddressResult {
                address: Ipv4Addr::new(127, 0, 0, 1),
                source: PassiveAddressSource::Loopback,
                use_epsv_recommended: false,
            }
        }
        ConnectionSource::PrivateNetwork => {
            if let Some(local_ip) = connection_local_ip {
                tracing::info!(
                    "Passive mode: Wildcard bind, private network connection -> {} (local interface IP)",
                    local_ip
                );
                PassiveAddressResult {
                    address: local_ip,
                    source: PassiveAddressSource::Private,
                    use_epsv_recommended: false,
                }
            } else {
                let fallback = find_matching_local_ip(&config.server_local_ips, client_source);
                tracing::info!(
                    "Passive mode: Wildcard bind, private network connection -> {} (matched local IP)",
                    fallback
                );
                PassiveAddressResult {
                    address: fallback,
                    source: PassiveAddressSource::Private,
                    use_epsv_recommended: false,
                }
            }
        }
        ConnectionSource::PublicNetwork => {
            if let Some(local_ip) = connection_local_ip {
                tracing::info!(
                    "Passive mode: Wildcard bind, public network connection -> {} (local interface IP)",
                    local_ip
                );
                PassiveAddressResult {
                    address: local_ip,
                    source: PassiveAddressSource::Public,
                    use_epsv_recommended: false,
                }
            } else {
                let fallback = find_matching_local_ip(&config.server_local_ips, client_source);
                tracing::info!(
                    "Passive mode: Wildcard bind, public network connection -> {} (matched local IP)",
                    fallback
                );
                PassiveAddressResult {
                    address: fallback,
                    source: PassiveAddressSource::Public,
                    use_epsv_recommended: matches!(client_ip, IpAddr::V6(_)),
                }
            }
        }
    }
}

fn find_matching_local_ip(local_ips: &[Ipv4Addr], source: ConnectionSource) -> Ipv4Addr {
    if local_ips.is_empty() {
        return Ipv4Addr::new(127, 0, 0, 1);
    }

    match source {
        ConnectionSource::Loopback => Ipv4Addr::new(127, 0, 0, 1),
        ConnectionSource::PrivateNetwork => {
            for ip in local_ips {
                if is_private_ipv4(ip) {
                    return *ip;
                }
            }
            local_ips[0]
        }
        ConnectionSource::PublicNetwork => {
            for ip in local_ips {
                if !is_private_ipv4(ip) && !ip.is_loopback() {
                    return *ip;
                }
            }
            local_ips[0]
        }
    }
}

fn find_ipv4_fallback(local_ips: &[Ipv4Addr], client_ip: &IpAddr) -> Ipv4Addr {
    if local_ips.is_empty() {
        return Ipv4Addr::new(127, 0, 0, 1);
    }

    let client_source = classify_connection_source(client_ip);
    find_matching_local_ip(local_ips, client_source)
}

pub fn determine_listen_address(bind_addr: &IpAddr) -> IpAddr {
    match bind_addr {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        other => *other,
    }
}

pub fn is_wildcard_bind(bind_addr: &IpAddr) -> bool {
    matches!(classify_bind_address(bind_addr), BindAddressType::Wildcard)
}

pub fn is_ipv6_bind(bind_addr: &IpAddr) -> bool {
    matches!(
        classify_bind_address(bind_addr),
        BindAddressType::SpecificIpv6
    )
}

#[derive(Debug, Clone)]
pub struct PassiveModeInfo {
    pub listen_address: IpAddr,
    pub pasv_response_ip: Ipv4Addr,
    pub address_source: PassiveAddressSource,
    pub connection_source: Option<ConnectionSource>,
    pub use_epsv_recommended: bool,
}

pub fn build_passive_mode_info(
    config: &PassiveModeConfig,
    client_ip: IpAddr,
    connection_local_ip: Option<Ipv4Addr>,
) -> PassiveModeInfo {
    let listen_address = determine_listen_address(&config.bind_address);
    let result = select_passive_address(config, client_ip, connection_local_ip);
    let connection_source = if is_wildcard_bind(&config.bind_address) {
        Some(classify_connection_source(&client_ip))
    } else {
        None
    };

    PassiveModeInfo {
        listen_address,
        pasv_response_ip: result.address,
        address_source: result.source,
        connection_source,
        use_epsv_recommended: result.use_epsv_recommended,
    }
}

pub fn get_local_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();

    #[cfg(windows)]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("ipconfig").args(["/all"]).output()
            && let Ok(stdout) = String::from_utf8(output.stdout)
        {
            for line in stdout.lines() {
                let line = line.trim();
                if (line.starts_with("IPv4") || line.contains("IPv4"))
                    && let Some(addr_str) = line.split(':').nth(1)
                {
                    let addr_str = addr_str.trim();
                    if let Some(ip_part) = addr_str.split('(').next()
                        && let Ok(ip) = ip_part.trim().parse::<Ipv4Addr>()
                    {
                        ips.push(ip);
                    }
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        use std::fs;
        if let Ok(entries) = fs::read_dir("/sys/class/net") {
            for entry in entries.flatten() {
                let path = entry.path().join("address");
                if path.exists() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if !content.trim().is_empty() {
                            let addr_path = entry.path().join("addr_assign_type");
                            if let Ok(assign_type) = fs::read_to_string(&addr_path) {
                                if assign_type.trim() == "0" {
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if ips.is_empty() {
        ips.push(Ipv4Addr::new(127, 0, 0, 1));
    }

    ips
}

#[derive(Debug, Clone)]
pub struct LocalIpAddress {
    pub ipv4: Vec<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Addr>,
}

impl LocalIpAddress {
    pub fn is_empty(&self) -> bool {
        self.ipv4.is_empty() && self.ipv6.is_empty()
    }

    pub fn format_for_log(&self) -> String {
        let mut parts = Vec::new();

        if !self.ipv4.is_empty() {
            let ipv4_strs: Vec<String> = self.ipv4.iter().map(|ip| ip.to_string()).collect();
            parts.push(format!("IPv4: [{}]", ipv4_strs.join(", ")));
        }

        if !self.ipv6.is_empty() {
            let ipv6_strs: Vec<String> = self.ipv6.iter().map(|ip| format!("[{}]", ip)).collect();
            parts.push(format!("IPv6: [{}]", ipv6_strs.join(", ")));
        }

        if parts.is_empty() {
            "No addresses".to_string()
        } else {
            parts.join(", ")
        }
    }
}

pub fn get_local_ip_addresses() -> LocalIpAddress {
    let mut result = LocalIpAddress {
        ipv4: Vec::new(),
        ipv6: Vec::new(),
    };

    #[cfg(windows)]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("ipconfig").args(["/all"]).output()
            && let Ok(stdout) = String::from_utf8(output.stdout)
        {
            for line in stdout.lines() {
                let line = line.trim();
                if (line.starts_with("IPv4") || line.contains("IPv4"))
                    && let Some(addr_str) = line.split(':').nth(1)
                {
                    let addr_str = addr_str.trim();
                    if let Some(ip_part) = addr_str.split('(').next()
                        && let Ok(ip) = ip_part.trim().parse::<Ipv4Addr>()
                        && !result.ipv4.contains(&ip)
                    {
                        result.ipv4.push(ip);
                    }
                }
                if (line.starts_with("IPv6") || line.contains("IPv6"))
                    && let Some(addr_str) = line.split(':').nth(1)
                {
                    let addr_str = addr_str.trim();
                    if let Ok(ip) = addr_str.parse::<Ipv6Addr>()
                        && !result.ipv6.contains(&ip)
                    {
                        result.ipv6.push(ip);
                    }
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        use std::fs;
        if let Ok(entries) = fs::read_dir("/sys/class/net") {
            for entry in entries.flatten() {
                let path = entry.path().join("address");
                if path.exists() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if content.trim().is_empty() {
                            continue;
                        }
                    }
                }
            }
        }
    }

    if result.is_empty() {
        result.ipv4.push(Ipv4Addr::new(127, 0, 0, 1));
        result.ipv6.push(Ipv6Addr::LOCALHOST);
    }

    result
}

pub fn format_listen_addresses(bind_ip: &str, port: u16) -> String {
    let bind_addr: IpAddr = bind_ip
        .parse()
        .unwrap_or(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));

    if is_wildcard_bind(&bind_addr) {
        let local_ips = get_local_ip_addresses();
        let mut addresses = Vec::new();

        for ip in &local_ips.ipv4 {
            addresses.push(format!("{}:{}", ip, port));
        }
        for ip in &local_ips.ipv6 {
            addresses.push(format!("[{}]:{}", ip, port));
        }

        if addresses.is_empty() {
            format!("0.0.0.0:{} (all interfaces)", port)
        } else {
            format!(
                "0.0.0.0:{} / [::]:{} (all interfaces: {})",
                port,
                port,
                addresses.join(", ")
            )
        }
    } else {
        match bind_addr {
            IpAddr::V4(ip) => format!("{}:{}", ip, port),
            IpAddr::V6(ip) => format!("[{}]:{}", ip, port),
        }
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
    fn test_classify_bind_address_specific_ipv4() {
        assert_eq!(
            classify_bind_address(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))),
            BindAddressType::SpecificIpv4
        );
    }

    #[test]
    fn test_classify_bind_address_specific_ipv6() {
        assert_eq!(
            classify_bind_address(&IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1))),
            BindAddressType::SpecificIpv6
        );
    }

    #[test]
    fn test_classify_connection_source_loopback() {
        assert_eq!(
            classify_connection_source(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            ConnectionSource::Loopback
        );
        assert_eq!(
            classify_connection_source(&IpAddr::V6(Ipv6Addr::LOCALHOST)),
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
            server_local_ips: vec![Ipv4Addr::new(192, 168, 1, 100)],
        };
        let result =
            select_passive_address(&config, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 200)), None);
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
            server_local_ips: vec![Ipv4Addr::new(192, 168, 1, 100)],
        };
        let result =
            select_passive_address(&config, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 200)), None);
        assert_eq!(result.address, Ipv4Addr::new(203, 0, 113, 50));
        assert_eq!(result.source, PassiveAddressSource::Masquerade);
    }

    #[test]
    fn test_select_passive_address_bind_address_priority() {
        let config = PassiveModeConfig {
            upnp_enabled: false,
            upnp_external_ip: None,
            masquerade_address: None,
            bind_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)),
            server_local_ips: vec![],
        };
        let result =
            select_passive_address(&config, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), None);
        assert_eq!(result.address, Ipv4Addr::new(192, 168, 1, 50));
        assert_eq!(result.source, PassiveAddressSource::BindAddress);
    }

    #[test]
    fn test_select_passive_address_wildcard_loopback() {
        let config = PassiveModeConfig {
            upnp_enabled: false,
            upnp_external_ip: None,
            masquerade_address: None,
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            server_local_ips: vec![Ipv4Addr::new(192, 168, 1, 100)],
        };
        let result = select_passive_address(&config, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), None);
        assert_eq!(result.address, Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(result.source, PassiveAddressSource::Loopback);
    }

    #[test]
    fn test_select_passive_address_wildcard_private() {
        let config = PassiveModeConfig {
            upnp_enabled: false,
            upnp_external_ip: None,
            masquerade_address: None,
            bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            server_local_ips: vec![Ipv4Addr::new(192, 168, 1, 100)],
        };
        let result = select_passive_address(
            &config,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 200)),
            Some(Ipv4Addr::new(192, 168, 1, 100)),
        );
        assert_eq!(result.address, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(result.source, PassiveAddressSource::Private);
    }

    #[test]
    fn test_select_passive_address_ipv6_bind() {
        let config = PassiveModeConfig {
            upnp_enabled: false,
            upnp_external_ip: None,
            masquerade_address: None,
            bind_address: IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
            server_local_ips: vec![Ipv4Addr::new(192, 168, 1, 100)],
        };
        let result = select_passive_address(
            &config,
            IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2)),
            None,
        );
        assert_eq!(result.address, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(result.source, PassiveAddressSource::Ipv6Fallback);
        assert!(result.use_epsv_recommended);
    }

    #[test]
    fn test_is_wildcard_bind() {
        assert!(is_wildcard_bind(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
        assert!(is_wildcard_bind(&IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
        assert!(!is_wildcard_bind(&IpAddr::V4(Ipv4Addr::new(
            192, 168, 1, 1
        ))));
    }

    #[test]
    fn test_is_ipv6_bind() {
        assert!(is_ipv6_bind(&IpAddr::V6(Ipv6Addr::new(
            0x2001, 0xdb8, 0, 0, 0, 0, 0, 1
        ))));
        assert!(!is_ipv6_bind(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(!is_ipv6_bind(&IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
    }

    #[test]
    fn test_find_matching_local_ip() {
        let ips = vec![
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(192, 168, 1, 100),
            Ipv4Addr::new(10, 0, 0, 1),
        ];

        assert_eq!(
            find_matching_local_ip(&ips, ConnectionSource::Loopback),
            Ipv4Addr::new(127, 0, 0, 1)
        );
        assert_eq!(
            find_matching_local_ip(&ips, ConnectionSource::PrivateNetwork),
            Ipv4Addr::new(192, 168, 1, 100)
        );
    }
}
