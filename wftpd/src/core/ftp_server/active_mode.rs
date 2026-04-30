//! FTP Active Mode NAT Detection
//!
//! Detects whether the client is behind NAT by comparing the IP address
//! in the PORT command with the client's actual TCP connection IP address.

use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientNatStatus {
    Direct,
    BehindNat,
}

#[derive(Debug, Clone)]
pub struct ActiveModeInfo {
    pub port_ip: IpAddr,
    pub tcp_ip: IpAddr,
    pub nat_status: ClientNatStatus,
}

pub fn detect_client_nat(port_command_ip: IpAddr, tcp_connection_ip: IpAddr) -> ClientNatStatus {
    if port_command_ip == tcp_connection_ip {
        ClientNatStatus::Direct
    } else {
        ClientNatStatus::BehindNat
    }
}

pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => is_private_ipv4(ipv4),
        IpAddr::V6(ipv6) => is_private_ipv6(ipv6),
    }
}

pub fn is_loopback_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => ipv4.is_loopback(),
        IpAddr::V6(ipv6) => ipv6.is_loopback(),
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

fn is_private_ipv6(ip: &std::net::Ipv6Addr) -> bool {
    let segments = ip.segments();
    if segments[0] == 0xfc00 || segments[0] == 0xfd00 {
        return true;
    }
    false
}

pub fn classify_ip_address(ip: &IpAddr) -> IpAddressClass {
    if is_loopback_ip(ip) {
        IpAddressClass::Loopback
    } else if is_private_ip(ip) {
        IpAddressClass::Private
    } else {
        IpAddressClass::Public
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpAddressClass {
    Loopback,
    Private,
    Public,
}

pub fn should_accept_port_command(
    port_ip: IpAddr,
    tcp_ip: IpAddr,
    allow_nat_clients: bool,
) -> bool {
    let nat_status = detect_client_nat(port_ip, tcp_ip);

    match nat_status {
        ClientNatStatus::Direct => true,
        ClientNatStatus::BehindNat => {
            if allow_nat_clients {
                tracing::info!(
                    "Active mode: Client behind NAT detected (PORT IP: {}, TCP IP: {}), allowing",
                    port_ip,
                    tcp_ip
                );
                true
            } else {
                tracing::warn!(
                    "Active mode: Client behind NAT detected (PORT IP: {}, TCP IP: {}), rejected",
                    port_ip,
                    tcp_ip
                );
                false
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActiveModeConfig {
    pub allow_nat_clients: bool,
    pub validate_port_ip_reachable: bool,
}

impl Default for ActiveModeConfig {
    fn default() -> Self {
        ActiveModeConfig {
            allow_nat_clients: true,
            validate_port_ip_reachable: false,
        }
    }
}

pub fn analyze_active_mode_connection(
    port_ip: IpAddr,
    tcp_ip: IpAddr,
    config: &ActiveModeConfig,
) -> ActiveModeInfo {
    let nat_status = detect_client_nat(port_ip, tcp_ip);

    if nat_status == ClientNatStatus::BehindNat && !config.allow_nat_clients {
        tracing::warn!(
            "Active mode: NAT client rejected by policy (PORT IP: {}, TCP IP: {})",
            port_ip,
            tcp_ip
        );
    }

    ActiveModeInfo {
        port_ip,
        tcp_ip,
        nat_status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    #[test]
    fn test_detect_client_nat_direct() {
        let port_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        let tcp_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(detect_client_nat(port_ip, tcp_ip), ClientNatStatus::Direct);
    }

    #[test]
    fn test_detect_client_nat_behind_nat() {
        let port_ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
        let tcp_ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 50));
        assert_eq!(
            detect_client_nat(port_ip, tcp_ip),
            ClientNatStatus::BehindNat
        );
    }

    #[test]
    fn test_is_private_ipv4() {
        assert!(is_private_ipv4(&Ipv4Addr::new(10, 0, 0, 1)));
        assert!(is_private_ipv4(&Ipv4Addr::new(172, 16, 0, 1)));
        assert!(is_private_ipv4(&Ipv4Addr::new(172, 31, 255, 255)));
        assert!(is_private_ipv4(&Ipv4Addr::new(192, 168, 0, 1)));
        assert!(!is_private_ipv4(&Ipv4Addr::new(8, 8, 8, 8)));
        assert!(!is_private_ipv4(&Ipv4Addr::new(172, 15, 0, 1)));
        assert!(!is_private_ipv4(&Ipv4Addr::new(172, 32, 0, 1)));
    }

    #[test]
    fn test_is_private_ipv6() {
        assert!(is_private_ipv6(&Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1)));
        assert!(is_private_ipv6(&Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1)));
        assert!(!is_private_ipv6(&Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)));
    }

    #[test]
    fn test_classify_ip_address() {
        assert_eq!(
            classify_ip_address(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
            IpAddressClass::Loopback
        );
        assert_eq!(
            classify_ip_address(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))),
            IpAddressClass::Private
        );
        assert_eq!(
            classify_ip_address(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))),
            IpAddressClass::Public
        );
    }

    #[test]
    fn test_should_accept_port_command() {
        let port_ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
        let tcp_ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 50));

        assert!(should_accept_port_command(port_ip, tcp_ip, true));
        assert!(!should_accept_port_command(port_ip, tcp_ip, false));
    }
}
