use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result as AnyhowResult;

use crate::core::path_utils::{safe_resolve_path, PathResolveError};

use super::passive::PassiveManager;
use super::tls::{AsyncTlsTcpStream, TlsConfig};

#[derive(Debug, Clone, PartialEq)]
pub enum FileStructure {
    File,
    Record,
    Page,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransferModeType {
    Stream,
    Block,
    Compressed,
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

    pub async fn upgrade_to_tls(&mut self, acceptor: &tokio_rustls::TlsAcceptor) -> AnyhowResult<()> {
        if let ControlStream::Plain(stream_opt) = self
            && let Some(stream) = stream_opt.take()
        {
            let tls_stream = acceptor.accept(stream).await?;
            *self = ControlStream::Tls(Box::new(tls_stream));
        }
        Ok(())
    }
}

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct SessionState {
    pub current_user: Option<String>,
    pub authenticated: bool,
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
}

impl SessionState {
    pub fn new(client_ip: &str, server_local_ip: &str, allow_symlinks: bool) -> Self {
        SessionState {
            current_user: None,
            authenticated: false,
            cwd: String::new(),
            home_dir: String::new(),
            transfer_mode: "binary".to_string(),
            encoding: "UTF-8".to_string(),
            passive_mode: true,
            rest_offset: 0,
            rename_from: None,
            abort_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            passive_manager: PassiveManager::new(),
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
            return true;
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
}

impl SessionConfig {
    pub fn from_config(config: &crate::core::config::Config, client_ip: &str) -> Self {
        let tls_config = TlsConfig::new(
            config.ftp.ftps.cert_path.as_deref(),
            config.ftp.ftps.key_path.as_deref(),
            config.ftp.ftps.require_ssl,
        );

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
        }
    }
}
