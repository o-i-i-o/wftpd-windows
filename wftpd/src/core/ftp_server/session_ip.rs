//! FTP IP and domain name resolution tools
//!
//! Provides IP address resolution, domain name resolution, and NAT traversal support

use std::collections::HashMap;
use std::net::IpAddr;
use std::str::FromStr;

pub fn is_domain_name(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_alphabetic()) && !s.chars().all(|c| c.is_ascii_digit() || c == '.')
}

pub async fn resolve_domain_to_ip(domain: &str) -> Option<String> {
    use tokio::net::lookup_host;
    match lookup_host((domain, 21)).await {
        Ok(mut addrs) => addrs.next().map(|addr| addr.ip().to_string()),
        Err(_) => None,
    }
}

pub async fn resolve_ip_for_pasv<S1: AsRef<str>, S2: AsRef<str>, S3: AsRef<str>>(
    ip_or_domain: S1,
    client_ip: S2,
    server_local_ip: S3,
) -> String {
    let ip_or_domain = ip_or_domain.as_ref();
    let client_ip = client_ip.as_ref();
    let server_local_ip = server_local_ip.as_ref();
    if is_domain_name(ip_or_domain) {
        match resolve_domain_to_ip(ip_or_domain).await {
            Some(resolved) => {
                if let Ok(ip) = IpAddr::from_str(&resolved) {
                    let client_ip_addr: IpAddr = client_ip
                        .parse()
                        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));
                    if ip.is_loopback() && !client_ip_addr.is_loopback() {
                        tracing::warn!(
                            "PASV: DNS resolved to loopback {} but client is from {}, using server_local_ip",
                            resolved,
                            client_ip
                        );
                        server_local_ip.to_string()
                    } else {
                        resolved
                    }
                } else {
                    tracing::warn!(
                        "PASV: invalid DNS resolution '{}', falling back to server_local_ip",
                        resolved
                    );
                    server_local_ip.to_string()
                }
            }
            None => {
                tracing::warn!(
                    "PASV: DNS resolution failed for '{}', using server_local_ip",
                    ip_or_domain
                );
                server_local_ip.to_string()
            }
        }
    } else {
        ip_or_domain.to_string()
    }
}

pub async fn find_masq_ip<S1: AsRef<str>, S2: AsRef<str>>(
    masquerade_map: &HashMap<String, String>,
    masquerade_address: &Option<String>,
    server_local_ip: S1,
    client_ip: S2,
) -> String {
    let server_local_ip = server_local_ip.as_ref();
    let client_ip = client_ip.as_ref();
    if let Some(masq_ip) = masquerade_map.get(server_local_ip)
        && !masq_ip.is_empty()
    {
        tracing::debug!(
            "PASV: found masq IP {} for server_local_ip {}",
            masq_ip,
            server_local_ip
        );
        return resolve_ip_for_pasv(masq_ip, client_ip, server_local_ip).await;
    }

    if let Some(masq_addr) = masquerade_address
        && !masq_addr.is_empty()
    {
        tracing::debug!(
            "PASV: using masquerade_address {} (no mapping for server_local_ip {})",
            masq_addr,
            server_local_ip
        );
        return resolve_ip_for_pasv(masq_addr, client_ip, server_local_ip).await;
    }

    tracing::debug!(
        "PASV: using server_local_ip {} (no masq configured)",
        server_local_ip
    );
    server_local_ip.to_string()
}

pub fn get_local_ip_for_client(client_ip: &str) -> String {
    let client_ip_addr: IpAddr = client_ip
        .parse()
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)));

    if client_ip_addr.is_loopback() {
        return "127.0.0.1".to_string();
    }

    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        let target = &format!("{}:80", client_ip);

        if socket.connect(target).is_ok()
            && let Ok(local_addr) = socket.local_addr()
        {
            let local_ip = local_addr.ip();
            if !local_ip.is_unspecified()
                && !local_ip.is_loopback()
                && let IpAddr::V4(ipv4) = local_ip
            {
                return ipv4.to_string();
            }
        }
    }

    let test_targets = ["8.8.8.8:80", "1.1.1.1:80", "192.168.1.1:80", "10.0.0.1:80"];

    for target in test_targets {
        if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0")
            && socket.connect(target).is_ok()
            && let Ok(local_addr) = socket.local_addr()
        {
            let local_ip = local_addr.ip();
            if !local_ip.is_unspecified()
                && !local_ip.is_loopback()
                && let IpAddr::V4(ipv4) = local_ip
                && is_same_subnet(&client_ip_addr, &local_ip)
            {
                return ipv4.to_string();
            }
        }
    }

    for target in test_targets {
        if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0")
            && socket.connect(target).is_ok()
            && let Ok(local_addr) = socket.local_addr()
        {
            let local_ip = local_addr.ip();
            if !local_ip.is_unspecified()
                && !local_ip.is_loopback()
                && let IpAddr::V4(ipv4) = local_ip
            {
                return ipv4.to_string();
            }
        }
    }

    "127.0.0.1".to_string()
}

pub fn is_same_subnet(ip1: &IpAddr, ip2: &IpAddr) -> bool {
    match (ip1, ip2) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let a_bytes = a.octets();
            let b_bytes = b.octets();
            a_bytes[0] == b_bytes[0] && a_bytes[1] == b_bytes[1] && a_bytes[2] == b_bytes[2]
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let a_bytes = a.octets();
            let b_bytes = b.octets();
            a_bytes[..8] == b_bytes[..8]
        }
        _ => false,
    }
}
