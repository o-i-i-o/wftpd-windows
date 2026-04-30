//! FTP server core module
//!
//! Provides FTP server using libunftp with:
//! - Explicit FTPS (FTPES) support
//! - UPnP port mapping
//! - Shared UserManager, QuotaManager, Fail2BanManager
//! - Per-user home directory support

mod active_mode;
mod auth;
mod binder;
pub(crate) mod cert_gen;
mod listeners;
mod passive_mode;
mod storage;
mod unftp;
pub mod upnp_manager;

pub use active_mode::{
    ActiveModeConfig, ActiveModeInfo,
    ClientNatStatus, IpAddressClass, analyze_active_mode_connection,
    classify_ip_address, detect_client_nat, is_loopback_ip, is_private_ip,
    should_accept_port_command,
};
pub use auth::{SessionTracker, WftpdAuthenticator, WftpdUser, WftpdUserDetailProvider};
pub use binder::{UpnpBinder, UpnpBinderBuilder};
pub use listeners::{FtpDataListener, FtpPresenceListener};
pub use passive_mode::{
    PassiveModeConfig, PassiveModeInfo, PassiveAddressSource, PassiveAddressResult,
    BindAddressType, ConnectionSource, build_passive_mode_info,
    classify_bind_address, classify_connection_source, determine_listen_address,
    is_wildcard_bind, select_passive_address,
};
pub use storage::QuotaFilesystem;
pub use unftp::FtpServer;
