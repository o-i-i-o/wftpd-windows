//! FTP Passive Mode Address Selection
//!
//! Handles the logic for determining the IP address to return in PASV responses.
//! Priority: Masquerade address > FromConnection (TCP destination IP via getsockname)
//!
//! ## PASV vs EPSV
//! - PASV (RFC 959) returns IP address and port in the response
//! - EPSV (RFC 2428) returns only the port, no IP address
//! - The choice between PASV and EPSV is entirely up to the client

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::core::ftp_server::ip_utils::{is_private_ipv4, is_private_ipv6};

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
}
