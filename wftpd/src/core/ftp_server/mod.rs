//! FTP server core module
//!
//! Provides FTP server using libunftp with:
//! - Explicit FTPS (FTPES) support
//! - UPnP port mapping
//! - Shared UserManager, QuotaManager, Fail2BanManager
//! - Per-user home directory support

mod auth;
mod binder;
mod cert_gen;
mod listeners;
mod storage;
mod tls;
mod unftp;
pub mod upnp_manager;

pub use auth::{WftpdAuthenticator, WftpdUser, WftpdUserDetailProvider};
pub use binder::{UpnpBinder, UpnpBinderBuilder};
pub use listeners::{FtpDataListener, FtpPresenceListener};
pub use storage::QuotaFilesystem;
pub use tls::TlsConfig;
pub use unftp::FtpServer;
