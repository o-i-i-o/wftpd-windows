//! IP address utility functions
//!
//! Provides common IP address classification functions for FTP server

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpAddressClass {
    Loopback,
    Private,
    Public,
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

pub fn is_private_ipv4(ip: &Ipv4Addr) -> bool {
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
    if octets[0] == 169 && octets[1] == 254 {
        return true;
    }
    false
}

pub fn is_private_ipv6(ip: &Ipv6Addr) -> bool {
    let segments = ip.segments();
    if segments[0] == 0xfc00 || segments[0] == 0xfd00 {
        return true;
    }
    if segments[0] == 0xfe80 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_private_ipv4() {
        assert!(is_private_ipv4(&Ipv4Addr::new(10, 0, 0, 1)));
        assert!(is_private_ipv4(&Ipv4Addr::new(172, 16, 0, 1)));
        assert!(is_private_ipv4(&Ipv4Addr::new(172, 31, 255, 255)));
        assert!(is_private_ipv4(&Ipv4Addr::new(192, 168, 0, 1)));
        assert!(is_private_ipv4(&Ipv4Addr::new(169, 254, 0, 1)));
        assert!(!is_private_ipv4(&Ipv4Addr::new(8, 8, 8, 8)));
        assert!(!is_private_ipv4(&Ipv4Addr::new(172, 15, 0, 1)));
        assert!(!is_private_ipv4(&Ipv4Addr::new(172, 32, 0, 1)));
    }

    #[test]
    fn test_is_private_ipv6() {
        assert!(is_private_ipv6(&Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1)));
        assert!(is_private_ipv6(&Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1)));
        assert!(is_private_ipv6(&Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)));
        assert!(!is_private_ipv6(&Ipv6Addr::new(
            0x2001, 0xdb8, 0, 0, 0, 0, 0, 1
        )));
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
}
