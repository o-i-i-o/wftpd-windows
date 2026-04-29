//! FTP server core module
//!
//! Provides FTP server using libunftp with:
//! - Explicit FTPS (FTPES) support
//! - UPnP port mapping
//! - Shared UserManager, QuotaManager, Fail2BanManager
//! - Per-user home directory support

mod auth;
mod binder;
pub(crate) mod cert_gen;
mod listeners;
mod storage;
mod unftp;
pub mod upnp_manager;

pub use auth::{SessionTracker, WftpdAuthenticator, WftpdUser, WftpdUserDetailProvider};
pub use binder::{UpnpBinder, UpnpBinderBuilder};
pub use listeners::{FtpDataListener, FtpPresenceListener};
pub use storage::QuotaFilesystem;
pub use unftp::FtpServer;
