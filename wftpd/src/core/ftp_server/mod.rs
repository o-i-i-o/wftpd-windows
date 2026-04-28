//! FTP server core module
//!
//! Provides FTP server using libunftp with:
//! - Explicit FTPS (FTPES) support
//! - UPnP port mapping
//! - Shared UserManager, QuotaManager, Fail2BanManager

mod auth;
mod binder;
mod cert_gen;
mod listeners;
mod unftp;
mod storage;
mod tls;
pub mod upnp_manager;

pub use auth::WftpdAuthenticator;
pub use binder::{UpnpBinder, UpnpBinderBuilder};
pub use listeners::{LoggingPresenceListener, QuotaDataListener};
pub use storage::QuotaFilesystem;
pub use unftp::FtpServer;
pub use tls::TlsConfig;
