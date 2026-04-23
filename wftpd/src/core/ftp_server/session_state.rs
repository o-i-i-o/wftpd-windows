//! FTP session state management
//!
//! Defines and controls FTP session connection state, transfer mode and file structure

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result as AnyhowResult;

use crate::core::path_utils::{PathResolveError, safe_resolve_path};

use super::passive::PassiveManager;
use super::reply::Reply;
use super::tls::{AsyncTlsTcpStream, TlsConfig};
use super::upnp_manager::UpnpManager;

#[derive(Debug, Clone, PartialEq)]
pub enum FileStructure {
    File,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransferModeType {
    Stream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtpSessionState {
    New,
    WaitPass,
    WaitCmd,
}

pub enum ControlStream {
    Plain(Option<tokio::net::TcpStream>),
    Tls(Box<AsyncTlsTcpStream>),
}

impl ControlStream {
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            ControlStream::Plain(Some(stream)) => stream.read(buf).await,
            ControlStream::Plain(None) => Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "No stream",
            )),
            ControlStream::Tls(stream) => stream.read(buf).await,
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            ControlStream::Plain(Some(stream)) => stream.write_all(buf).await,
            ControlStream::Plain(None) => Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "No stream",
            )),
            ControlStream::Tls(stream) => stream.write_all(buf).await,
        }
    }

    pub async fn write_response(&mut self, buf: &[u8], context: &str) {
        if let Err(e) = self.write_all(buf).await {
            tracing::warn!("Failed to write FTP response ({}): {}", context, e);
        }
    }

    pub async fn send_reply(&mut self, reply: &Reply) {
        let data = reply.to_bytes();
        if !data.is_empty()
            && let Err(e) = self.write_all(&data).await
        {
            tracing::warn!("Failed to send FTP reply: {:?}", e);
        }
    }

    pub async fn upgrade_to_tls(
        &mut self,
        acceptor: &tokio_rustls::TlsAcceptor,
    ) -> AnyhowResult<()> {
        let plain_stream = match self {
            ControlStream::Plain(stream_opt) => stream_opt.take(),
            ControlStream::Tls(_) => return Ok(()),
        };

        let stream = plain_stream
            .ok_or_else(|| anyhow::anyhow!("No plain stream available for TLS upgrade"))?;

        match acceptor.accept(stream).await {
            Ok(tls_stream) => {
                *self = ControlStream::Tls(Box::new(tls_stream));
                Ok(())
            }
            Err(e) => {
                *self = ControlStream::Plain(None);
                Err(e.into())
            }
        }
    }

    pub fn local_ip(&self) -> Option<std::net::IpAddr> {
        match self {
            ControlStream::Plain(Some(stream)) => stream.local_addr().ok().map(|a| a.ip()),
            ControlStream::Plain(None) => None,
            ControlStream::Tls(stream) => {
                let (inner, _) = stream.get_ref();
                inner.local_addr().ok().map(|a| a.ip())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_state() -> SessionState {
        SessionState::new("192.168.1.100", "192.168.1.1", true, None)
    }

    #[test]
    fn test_ftp_session_state_initial() {
        let state = create_test_state();
        assert_eq!(state.ftp_state, FtpSessionState::New);
        assert!(!state.authenticated);
        assert!(state.current_user.is_none());
        assert_eq!(state.login_attempts, 0);
    }

    #[test]
    fn test_ftp_session_state_transitions() {
        let mut state = create_test_state();
        assert_eq!(state.ftp_state, FtpSessionState::New);

        state.ftp_state = FtpSessionState::WaitPass;
        assert_eq!(state.ftp_state, FtpSessionState::WaitPass);

        state.ftp_state = FtpSessionState::WaitCmd;
        assert_eq!(state.ftp_state, FtpSessionState::WaitCmd);
    }

    #[test]
    fn test_ftp_session_state_equality() {
        assert_eq!(FtpSessionState::New, FtpSessionState::New);
        assert_eq!(FtpSessionState::WaitPass, FtpSessionState::WaitPass);
        assert_eq!(FtpSessionState::WaitCmd, FtpSessionState::WaitCmd);
        assert_ne!(FtpSessionState::New, FtpSessionState::WaitPass);
        assert_ne!(FtpSessionState::WaitPass, FtpSessionState::WaitCmd);
        assert_ne!(FtpSessionState::New, FtpSessionState::WaitCmd);
    }

    #[test]
    fn test_validate_port_ip_valid() {
        let state = create_test_state();
        assert!(state.validate_port_ip("192,168,1,100,4,1"));
    }

    #[test]
    fn test_validate_port_ip_wrong_ip() {
        let state = create_test_state();
        assert!(!state.validate_port_ip("10,0,0,1,4,1"));
    }

    #[test]
    fn test_validate_port_ip_partial_mismatch() {
        let state = create_test_state();
        assert!(!state.validate_port_ip("192,168,1,200,4,1"));
    }

    #[test]
    fn test_validate_port_ip_too_few_parts() {
        let state = create_test_state();
        assert!(!state.validate_port_ip("192,168,1,100"));
    }

    #[test]
    fn test_validate_port_ip_too_many_parts() {
        let state = create_test_state();
        assert!(!state.validate_port_ip("192,168,1,100,4,1,2"));
    }

    #[test]
    fn test_validate_port_ip_empty() {
        let state = create_test_state();
        assert!(!state.validate_port_ip(""));
    }

    #[test]
    fn test_validate_eprt_ip_matching_ipv4() {
        let state = create_test_state();
        assert!(state.validate_eprt_ip("192.168.1.100"));
    }

    #[test]
    fn test_validate_eprt_ip_mismatched_ipv4() {
        let state = create_test_state();
        assert!(!state.validate_eprt_ip("10.0.0.1"));
    }

    #[test]
    fn test_validate_eprt_ip_invalid_ip() {
        let state = create_test_state();
        assert!(!state.validate_eprt_ip("not-an-ip"));
    }

    #[test]
    fn test_validate_eprt_ip_ipv6_matching() {
        let state = SessionState::new("::1", "::1", true, None);
        assert!(state.validate_eprt_ip("::1"));
    }

    #[test]
    fn test_validate_eprt_ip_ipv6_mismatch() {
        let state = SessionState::new("::1", "::1", true, None);
        assert!(!state.validate_eprt_ip("::2"));
    }

    #[test]
    fn test_validate_eprt_ip_version_mismatch() {
        let state = create_test_state();
        assert!(!state.validate_eprt_ip("::1"));
    }

    #[test]
    fn test_file_structure_default() {
        let state = create_test_state();
        assert_eq!(state.file_structure, FileStructure::File);
    }

    #[test]
    fn test_transfer_mode_type_default() {
        let state = create_test_state();
        assert_eq!(state.transfer_mode_type, TransferModeType::Stream);
    }

    #[test]
    fn test_session_state_default_values() {
        let state = create_test_state();
        assert_eq!(state.transfer_mode, "binary");
        assert_eq!(state.encoding, "UTF-8");
        assert!(state.passive_mode);
        assert_eq!(state.rest_offset, 0);
        assert!(state.rename_from.is_none());
        assert!(!state.tls_enabled);
        assert!(!state.data_protection);
        assert!(!state.pbsz_set);
        assert!(state.allow_symlinks);
        assert!(state.data_port.is_none());
        assert!(state.data_addr.is_none());
    }
}

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct SessionState {
    pub current_user: Option<String>,
    pub authenticated: bool,
    pub login_attempts: u32,
    pub cwd: String,
    pub home_dir: String,
    pub transfer_mode: String,
    pub encoding: String,
    pub passive_mode: bool,
    pub rest_offset: u64,
    pub rename_from: Option<String>,
    pub abort_flag: Arc<std::sync::atomic::AtomicBool>,
    pub passive_manager: PassiveManager,
    pub data_port: Option<u16>,
    pub data_addr: Option<String>,
    pub client_ip: String,
    pub server_local_ip: String,
    pub tls_enabled: bool,
    pub data_protection: bool,
    pub pbsz_set: bool,
    pub file_structure: FileStructure,
    pub transfer_mode_type: TransferModeType,
    pub allow_symlinks: bool,
    pub ftp_state: FtpSessionState,
}

impl SessionState {
    pub fn new(
        client_ip: &str,
        server_local_ip: &str,
        allow_symlinks: bool,
        upnp_manager: Option<Arc<UpnpManager>>,
    ) -> Self {
        SessionState {
            current_user: None,
            authenticated: false,
            login_attempts: 0,
            cwd: String::new(),
            home_dir: String::new(),
            transfer_mode: "binary".to_string(),
            encoding: "UTF-8".to_string(),
            passive_mode: true,
            rest_offset: 0,
            rename_from: None,
            abort_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            passive_manager: PassiveManager::new(upnp_manager),
            data_port: None,
            data_addr: None,
            client_ip: client_ip.to_string(),
            server_local_ip: server_local_ip.to_string(),
            tls_enabled: false,
            data_protection: false,
            pbsz_set: false,
            file_structure: FileStructure::File,
            transfer_mode_type: TransferModeType::Stream,
            allow_symlinks,
            ftp_state: FtpSessionState::New,
        }
    }

    pub fn resolve_path(&self, path: &str) -> std::result::Result<PathBuf, PathResolveError> {
        safe_resolve_path(&self.cwd, &self.home_dir, path, self.allow_symlinks)
    }

    pub fn validate_port_ip(&self, port_ip: &str) -> bool {
        let parts: Vec<&str> = port_ip.split(',').collect();
        if parts.len() != 6 {
            return false;
        }

        let client_ip_parts: Vec<&str> = self.client_ip.split('.').collect();
        if client_ip_parts.len() != 4 {
            tracing::warn!(
                "PORT security: non-IPv4 client '{}' cannot use PORT command, use EPRT instead",
                self.client_ip
            );
            return false;
        }

        let port_ip_parts = &parts[0..4];
        for (i, part) in port_ip_parts.iter().enumerate() {
            if let Ok(num) = part.parse::<u8>()
                && let Ok(client_num) = client_ip_parts[i].parse::<u8>()
                && num != client_num
            {
                tracing::warn!(
                    "PORT security: IP mismatch - expected {}, got {} in position {}",
                    client_num,
                    num,
                    i
                );
                return false;
            }
        }
        true
    }

    pub fn validate_eprt_ip(&self, net_addr: &str) -> bool {
        let client_ip: std::net::IpAddr = match self.client_ip.parse() {
            Ok(ip) => ip,
            Err(e) => {
                tracing::error!(
                    "EPRT security: Failed to parse client IP '{}': {}",
                    self.client_ip,
                    e
                );
                return false;
            }
        };

        let eprt_ip: std::net::IpAddr = match net_addr.parse() {
            Ok(ip) => ip,
            Err(e) => {
                tracing::warn!(
                    "EPRT security: Failed to parse EPRT IP '{}': {}",
                    net_addr,
                    e
                );
                return false;
            }
        };

        match (&client_ip, &eprt_ip) {
            (std::net::IpAddr::V4(client), std::net::IpAddr::V4(eprt)) => {
                if client != eprt {
                    tracing::warn!(
                        "EPRT security: IPv4 mismatch - expected {}, got {}",
                        client,
                        eprt
                    );
                    return false;
                }
            }
            (std::net::IpAddr::V6(client), std::net::IpAddr::V6(eprt)) => {
                if client != eprt {
                    tracing::warn!(
                        "EPRT security: IPv6 mismatch - expected {}, got {}",
                        client,
                        eprt
                    );
                    return false;
                }
            }
            _ => {
                tracing::warn!(
                    "EPRT security: IP version mismatch - client is {}, eprt is {}",
                    client_ip,
                    eprt_ip
                );
                return false;
            }
        }

        true
    }
}

pub struct SessionConfig {
    pub welcome_msg: String,
    pub allow_anonymous: bool,
    pub anonymous_home: Option<String>,
    pub default_transfer_mode: String,
    pub default_passive_mode: bool,
    pub encoding: String,
    pub ip_allowed: bool,
    pub tls_config: TlsConfig,
    pub require_ssl: bool,
    pub connection_timeout: u64,
    pub idle_timeout: u64,
}

impl SessionConfig {
    pub fn from_config(config: &crate::core::config::Config, client_ip: &str) -> Self {
        let tls_config = if config.ftp.ftps.enabled {
            TlsConfig::new(
                config.ftp.ftps.cert_path.as_deref(),
                config.ftp.ftps.key_path.as_deref(),
                config.ftp.ftps.require_ssl,
            )
        } else {
            TlsConfig { acceptor: None }
        };

        SessionConfig {
            welcome_msg: config.ftp.welcome_message.clone(),
            allow_anonymous: config.ftp.allow_anonymous,
            anonymous_home: config.ftp.anonymous_home.clone(),
            default_transfer_mode: config.ftp.default_transfer_mode.clone(),
            default_passive_mode: config.ftp.default_passive_mode,
            encoding: config.ftp.encoding.clone(),
            ip_allowed: config.is_ip_allowed(client_ip),
            tls_config,
            require_ssl: config.ftp.ftps.enabled && config.ftp.ftps.require_ssl,
            connection_timeout: config.ftp.connection_timeout,
            idle_timeout: config.ftp.idle_timeout,
        }
    }
}
