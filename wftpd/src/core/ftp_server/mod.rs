//! FTP server core module
//!
//! Provides FTP server using libunftp with:
//! - Explicit FTPS (FTPES) support
//! - Shared UserManager, QuotaManager, Fail2BanManager
//! - Per-user home directory support

mod active_mode;
mod auth;
pub(crate) mod cert_gen;
mod ip_utils;
mod listeners;
mod passive_mode;
mod storage;
mod unftp;

pub use active_mode::{
    ActiveModeConfig, ActiveModeInfo, ClientNatStatus, analyze_active_mode_connection,
    detect_client_nat, should_accept_port_command,
};
pub use auth::{SessionTracker, WftpdAuthenticator, WftpdUser, WftpdUserDetailProvider};
pub use ip_utils::{
    IpAddressClass, classify_ip_address, is_loopback_ip, is_private_ip, is_private_ipv4,
    is_private_ipv6,
};
pub use listeners::{FtpDataListener, FtpPresenceListener};
pub use passive_mode::{
    BindAddressType, ConnectionSource, LocalIpAddress, classify_bind_address,
    classify_connection_source, determine_listen_address, format_listen_addresses,
    get_local_ip_addresses, get_local_ipv4_addresses, is_ipv6_bind, is_wildcard_bind,
};
pub use storage::QuotaFilesystem;
pub use unftp::FtpServer;
