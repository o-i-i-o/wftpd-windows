use anyhow::Result;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use std::net::ToSocketAddrs;

use crate::core::config::Config;
use crate::core::users::UserManager;
use crate::core::quota::QuotaManager;
use crate::core::path_utils::{safe_resolve_path, to_ftp_path, resolve_directory_path, PathResolveError, path_starts_with_ignore_case};

use super::commands::FtpCommand;
use super::passive::PassiveManager;
use super::transfer;
use super::tls::{TlsConfig, AsyncTlsTcpStream};

const MAX_COMMAND_LENGTH: usize = 8192;

/// 判断是否为域名（简单的启发式判断）
fn is_domain_name(s: &str) -> bool {
    // 如果包含字母且不是纯 IP 地址格式，则认为是域名
    s.chars().any(|c| c.is_ascii_alphabetic()) && !s.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// 尝试将域名解析为 IP 地址
fn resolve_domain_to_ip(domain: &str) -> Option<String> {
    match (domain, 21).to_socket_addrs() {
        Ok(mut addrs) => addrs.next().map(|addr| addr.ip().to_string()),
        Err(_) => None,
    }
}

pub enum ControlStream {
    Plain(Option<TcpStream>),
    Tls(Box<AsyncTlsTcpStream>),
}

impl ControlStream {
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            ControlStream::Plain(Some(stream)) => stream.read(buf).await,
            ControlStream::Plain(None) => Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "No stream")),
            ControlStream::Tls(stream) => stream.read(buf).await,
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            ControlStream::Plain(Some(stream)) => stream.write_all(buf).await,
            ControlStream::Plain(None) => Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "No stream")),
            ControlStream::Tls(stream) => stream.write_all(buf).await,
        }
    }

    pub async fn write_response(&mut self, buf: &[u8], context: &str) {
        if let Err(e) = self.write_all(buf).await {
            tracing::warn!("Failed to write FTP response ({}): {}", context, e);
        }
    }

    pub async fn upgrade_to_tls(&mut self, acceptor: &tokio_native_tls::TlsAcceptor) -> Result<()> {
        if let ControlStream::Plain(stream_opt) = self
            && let Some(stream) = stream_opt.take() {
                let tls_stream = acceptor.accept(stream).await?;
                *self = ControlStream::Tls(Box::new(tls_stream));
            }
        Ok(())
    }
}

pub struct SessionState {
    pub current_user: Option<String>,
    pub authenticated: bool,
    pub cwd: String,
    pub home_dir: String,
    pub transfer_mode: String,
    pub passive_mode: bool,
    pub rest_offset: u64,
    pub rename_from: Option<String>,
    pub abort_flag: Arc<std::sync::atomic::AtomicBool>,
    pub passive_manager: PassiveManager,
    pub data_port: Option<u16>,
    pub data_addr: Option<String>,
    pub client_ip: String,
    pub tls_enabled: bool,
    pub data_protection: bool,
    pub pbsz_set: bool,
    pub file_structure: FileStructure,
    pub transfer_mode_type: TransferModeType,
}

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

impl SessionState {
    pub fn new(client_ip: &str) -> Self {
        SessionState {
            current_user: None,
            authenticated: false,
            cwd: String::new(),
            home_dir: String::new(),
            transfer_mode: "binary".to_string(),
            passive_mode: true,
            rest_offset: 0,
            rename_from: None,
            abort_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            passive_manager: PassiveManager::new(),
            data_port: None,
            data_addr: None,
            client_ip: client_ip.to_string(),
            tls_enabled: false,
            data_protection: false,
            pbsz_set: false,
            file_structure: FileStructure::File,
            transfer_mode_type: TransferModeType::Stream,
        }
    }

    pub fn resolve_path(&self, path: &str) -> Result<PathBuf, PathResolveError> {
        safe_resolve_path(&self.cwd, &self.home_dir, path)
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
                && num != client_num {
                    tracing::warn!("PORT security: IP mismatch - expected {}, got {} in position {}", 
                        client_num, num, i);
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
                    self.client_ip, e
                );
                return false;
            }
        };
        
        let eprt_ip: std::net::IpAddr = match net_addr.parse() {
            Ok(ip) => ip,
            Err(e) => {
                tracing::warn!(
                    "EPRT security: Failed to parse EPRT IP '{}': {}",
                    net_addr, e
                );
                return false;
            }
        };
        
        match (&client_ip, &eprt_ip) {
            (std::net::IpAddr::V4(client), std::net::IpAddr::V4(eprt)) => {
                if client != eprt {
                    tracing::warn!(
                        "EPRT security: IPv4 mismatch - expected {}, got {}",
                        client, eprt
                    );
                    return false;
                }
            }
            (std::net::IpAddr::V6(client), std::net::IpAddr::V6(eprt)) => {
                if client != eprt {
                    tracing::warn!(
                        "EPRT security: IPv6 mismatch - expected {}, got {}",
                        client, eprt
                    );
                    return false;
                }
            }
            _ => {
                tracing::warn!(
                    "EPRT security: IP version mismatch - client is {}, eprt is {}",
                    client_ip, eprt_ip
                );
                return false;
            }
        }
        
        true
    }
}

struct SessionConfig {
    welcome_msg: String,
    allow_anonymous: bool,
    anonymous_home: Option<String>,
    default_transfer_mode: String,
    default_passive_mode: bool,
    ip_allowed: bool,
    tls_config: TlsConfig,
    require_ssl: bool,
}

impl SessionConfig {
    fn from_config(config: &Config, client_ip: &str) -> Self {
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
            ip_allowed: config.is_ip_allowed(client_ip),
            tls_config,
            require_ssl: config.ftp.ftps.require_ssl,
        }
    }
}

pub async fn handle_session(
    mut socket: TcpStream,
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    client_ip: String,
) -> Result<()> {
    let session_config = {
        let cfg = config.lock();
        SessionConfig::from_config(&cfg, &client_ip)
    };

    if !session_config.ip_allowed {
        tracing::warn!("Connection rejected from {} by IP filter", client_ip);
        if let Err(e) = socket.write_all(b"530 Connection denied by IP filter\r\n").await {
            tracing::debug!("Failed to send IP filter rejection to {}: {}", client_ip, e);
        }
        return Ok(());
    }

    let mut control_stream = ControlStream::Plain(Some(socket));
    control_stream.write_response(format!("220 {}\r\n", session_config.welcome_msg).as_bytes(), "welcome message").await;

    let mut state = SessionState::new(&client_ip);
    state.transfer_mode = session_config.default_transfer_mode;
    state.passive_mode = session_config.default_passive_mode;

    let mut cmd_buffer: Vec<u8> = Vec::with_capacity(MAX_COMMAND_LENGTH);
    let mut read_buffer = [0u8; 4096];

    loop {
        let conn_timeout = {
            let cfg = config.lock();
            cfg.ftp.connection_timeout
        };

        let timeout_result = tokio::time::timeout(
            std::time::Duration::from_secs(conn_timeout),
            control_stream.read(&mut read_buffer)
        ).await;

        match timeout_result {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                cmd_buffer.extend_from_slice(&read_buffer[..n]);
                
                if cmd_buffer.len() > MAX_COMMAND_LENGTH {
                    control_stream.write_response(b"500 Command too long\r\n", "command too long").await;
                    cmd_buffer.clear();
                    continue;
                }

                while let Some(crlf_pos) = cmd_buffer.windows(2).position(|w| w == b"\r\n") {
                    let command_bytes: Vec<u8> = cmd_buffer.drain(..crlf_pos + 2).collect();
                    let command = String::from_utf8_lossy(&command_bytes[..command_bytes.len().saturating_sub(2)])
                        .trim()
                        .to_string();

                    let parts: Vec<&str> = command.splitn(2, ' ').collect();
                    let cmd = parts[0].to_uppercase();
                    let arg = parts.get(1).map(|s| s.trim());

                    let ftp_cmd = FtpCommand::parse(&cmd, arg);
                    
                    if !handle_command(
                        &mut control_stream,
                        &ftp_cmd,
                        &mut state,
                        &config,
                        &user_manager,
                        &quota_manager,
                        &client_ip,
                        &session_config.allow_anonymous,
                        &session_config.anonymous_home,
                        &session_config.tls_config,
                        session_config.require_ssl,
                    ).await? {
                        return Ok(());
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::debug!("读取错误: {}", e);
                break;
            }
            Err(_) => {
                control_stream.write_response(b"421 Connection timed out\r\n", "connection timeout").await;
                break;
            }
        }
    }

    Ok(())
}

pub async fn handle_session_tls(
    socket: AsyncTlsTcpStream,
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    client_ip: String,
) -> Result<()> {
    let session_config = {
        let cfg = config.lock();
        SessionConfig::from_config(&cfg, &client_ip)
    };

    if !session_config.ip_allowed {
        tracing::warn!("Connection rejected from {} by IP filter", client_ip);
        return Ok(());
    }

    let mut control_stream = ControlStream::Tls(Box::new(socket));
    control_stream.write_response(format!("220 {}\r\n", session_config.welcome_msg).as_bytes(), "welcome message (TLS)").await;

    let mut state = SessionState::new(&client_ip);
    state.transfer_mode = session_config.default_transfer_mode;
    state.passive_mode = session_config.default_passive_mode;
    state.tls_enabled = true;

    let mut cmd_buffer: Vec<u8> = Vec::with_capacity(MAX_COMMAND_LENGTH);
    let mut read_buffer = [0u8; 4096];

    loop {
        let conn_timeout = {
            let cfg = config.lock();
            cfg.ftp.connection_timeout
        };

        let timeout_result = tokio::time::timeout(
            std::time::Duration::from_secs(conn_timeout),
            control_stream.read(&mut read_buffer)
        ).await;

        match timeout_result {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                cmd_buffer.extend_from_slice(&read_buffer[..n]);
                
                if cmd_buffer.len() > MAX_COMMAND_LENGTH {
                    control_stream.write_response(b"500 Command too long\r\n", "command too long (TLS)").await;
                    cmd_buffer.clear();
                    continue;
                }
                
                while let Some(pos) = cmd_buffer.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = cmd_buffer.drain(..=pos).collect();
                    let line_str = String::from_utf8_lossy(&line);
                    let line_str = line_str.trim_end_matches('\r').trim_end_matches('\n');
                    
                    if line_str.is_empty() {
                        continue;
                    }
                    
                    let cmd = {
                        let parts: Vec<&str> = line_str.splitn(2, ' ').collect();
                        let cmd_str = parts[0].to_uppercase();
                        let arg = parts.get(1).map(|s| s.trim());
                        FtpCommand::parse(&cmd_str, arg)
                    };
                    
                    let (allow_anonymous, anonymous_home, tls_config, require_ssl) = {
                        let cfg = config.lock();
                        (
                            cfg.ftp.allow_anonymous,
                            cfg.ftp.anonymous_home.clone(),
                            TlsConfig::new(
                                cfg.ftp.ftps.cert_path.as_deref(),
                                cfg.ftp.ftps.key_path.as_deref(),
                                cfg.ftp.ftps.require_ssl
                            ),
                            cfg.ftp.ftps.require_ssl
                        )
                    };
                    
                    let should_continue = handle_command(
                        &mut control_stream,
                        &cmd,
                        &mut state,
                        &config,
                        &user_manager,
                        &quota_manager,
                        &client_ip,
                        &allow_anonymous,
                        &anonymous_home,
                        &tls_config,
                        require_ssl,
                    ).await?;
                    
                    if !should_continue {
                        return Ok(());
                    }
                }
            }
            Ok(Err(_)) | Err(_) => {
                control_stream.write_response(b"421 Connection timed out\r\n", "connection timeout (TLS)").await;
                break;
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_command(
    control_stream: &mut ControlStream,
    cmd: &FtpCommand,
    state: &mut SessionState,
    config: &Arc<Mutex<Config>>,
    user_manager: &Arc<Mutex<UserManager>>,
    quota_manager: &Arc<QuotaManager>,
    client_ip: &str,
    allow_anonymous: &bool,
    anonymous_home: &Option<String>,
    tls_config: &TlsConfig,
    require_ssl: bool,
) -> Result<bool> {
    use super::commands::FtpCommand::*;

    match cmd {
        AUTH(tls_type) => {
            let tls_type = tls_type.as_deref().unwrap_or("TLS");
            let tls_upper = tls_type.to_uppercase();
            
            if tls_config.is_tls_available() {
                if tls_upper == "TLS" || tls_upper == "TLS-C" || tls_upper == "SSL" {
                    control_stream.write_response(b"234 AUTH command OK; starting TLS connection\r\n", "FTP response").await;
                    
                    if let Some(acceptor) = &tls_config.acceptor {
                        match control_stream.upgrade_to_tls(acceptor).await {
                            Ok(()) => {
                                state.tls_enabled = true;
                                tracing::info!("TLS connection established for {}", client_ip);
                            }
                            Err(e) => {
                                tracing::error!("TLS upgrade failed: {}", e);
                                control_stream.write_response(b"431 Unable to negotiate TLS connection\r\n", "FTP response").await;
                            }
                        }
                    }
                } else {
                    control_stream.write_response(format!("504 AUTH {} not supported\r\n", tls_type).as_bytes(), "FTP response").await;
                }
            } else {
                control_stream.write_response(b"502 TLS not configured on server\r\n", "FTP response").await;
            }
        }

        PBSZ(size) => {
            if state.tls_enabled {
                if let Some(size_str) = size {
                    if let Ok(size_val) = size_str.parse::<u64>() {
                        state.pbsz_set = true;
                        control_stream.write_response(format!("200 PBSZ={} OK\r\n", size_val).as_bytes(), "FTP response").await;
                    } else {
                        control_stream.write_response(b"501 Invalid PBSZ value\r\n", "FTP response").await;
                    }
                } else {
                    state.pbsz_set = true;
                    control_stream.write_response(b"200 PBSZ=0 OK\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"503 PBSZ requires AUTH first\r\n", "FTP response").await;
            }
        }

        PROT(level) => {
            if state.tls_enabled && state.pbsz_set {
                if let Some(level) = level {
                    match level.to_uppercase().as_str() {
                        "P" => {
                            state.data_protection = true;
                            control_stream.write_response(b"200 PROT Private OK\r\n", "FTP response").await;
                        }
                        "C" => {
                            state.data_protection = false;
                            control_stream.write_response(b"200 PROT Clear OK\r\n", "FTP response").await;
                        }
                        "S" => {
                            control_stream.write_response(b"536 PROT Safe not supported\r\n", "FTP response").await;
                        }
                        "E" => {
                            control_stream.write_response(b"536 PROT Confidential not supported\r\n", "FTP response").await;
                        }
                        _ => {
                            control_stream.write_response(b"504 Unknown PROT level\r\n", "FTP response").await;
                        }
                    }
                } else {
                    control_stream.write_response(b"501 PROT requires parameter (C/P/S/E)\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"503 PROT requires PBSZ first\r\n", "FTP response").await;
            }
        }

        CCC => {
            if state.tls_enabled {
                control_stream.write_response(b"200 CCC OK - reverting to clear text\r\n", "FTP response").await;
            } else {
                control_stream.write_response(b"533 CCC not available - not in TLS mode\r\n", "FTP response").await;
            }
        }

        // RFC 2228 Security Commands
        ADAT(_data) => {
            if state.tls_enabled {
                // ADAT用于Kerberos等认证机制的安全数据交换
                // 在TLS模式下，我们已经有了加密通道，所以返回不支持
                tracing::debug!("ADAT received but not implemented (TLS already provides security)");
                control_stream.write_response(b"504 ADAT not implemented - TLS provides security\r\n", "FTP response").await;
            } else {
                control_stream.write_response(b"503 ADAT requires AUTH first\r\n", "FTP response").await;
            }
        }

        MIC(data) => {
            if state.tls_enabled {
                // MIC用于完整性保护的命令
                // 在TLS模式下，TLS已经提供了完整性保护
                if let Some(data) = data {
                    tracing::debug!("MIC command received: {} (TLS already provides integrity)", data);
                    // 返回200表示接受，但实际不处理（TLS已提供完整性）
                    control_stream.write_response(b"200 MIC accepted - integrity provided by TLS\r\n", "FTP response").await;
                } else {
                    control_stream.write_response(b"501 MIC requires data parameter\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"503 MIC requires AUTH first\r\n", "FTP response").await;
            }
        }

        CONF(data) => {
            if state.tls_enabled {
                // CONF用于机密性保护的命令
                // 在TLS模式下，TLS已经提供了机密性
                if let Some(data) = data {
                    tracing::debug!("CONF command received: {} (TLS already provides confidentiality)", data);
                    // 返回200表示接受，但实际不处理（TLS已提供机密性）
                    control_stream.write_response(b"200 CONF accepted - confidentiality provided by TLS\r\n", "FTP response").await;
                } else {
                    control_stream.write_response(b"501 CONF requires data parameter\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"503 CONF requires AUTH first\r\n", "FTP response").await;
            }
        }

        ENC(data) => {
            if state.tls_enabled {
                // ENC用于加密保护的命令
                // 在TLS模式下，TLS已经提供了加密
                if let Some(data) = data {
                    tracing::debug!("ENC command received: {} (TLS already provides encryption)", data);
                    // 返回200表示接受，但实际不处理（TLS已提供加密）
                    control_stream.write_response(b"200 ENC accepted - encryption provided by TLS\r\n", "FTP response").await;
                } else {
                    control_stream.write_response(b"501 ENC requires data parameter\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"503 ENC requires AUTH first\r\n", "FTP response").await;
            }
        }

        USER(username) => {
            if require_ssl && !state.tls_enabled {
                control_stream.write_response(b"530 SSL required for login\r\n", "FTP response").await;
                return Ok(true);
            }
            
            let username_lower = username.to_lowercase();
            if username_lower == "anonymous" || username_lower == "ftp" {
                if *allow_anonymous {
                    state.current_user = Some("anonymous".to_string());
                    control_stream.write_response(b"331 Anonymous login okay, send email as password\r\n", "FTP response").await;
                } else {
                    control_stream.write_response(b"530 Anonymous access not allowed\r\n", "FTP response").await;
                }
            } else {
                state.current_user = Some(username.to_string());
                control_stream.write_response(b"331 User name okay, need password\r\n", "FTP response").await;
            }
        }

        PASS(password) => {
            if require_ssl && !state.tls_enabled {
                control_stream.write_response(b"530 SSL required for login\r\n", "FTP response").await;
                return Ok(true);
            }
            
            if let Some(ref username) = state.current_user {
                if username == "anonymous" {
                    if *allow_anonymous {
                        if let Some(anon_home) = anonymous_home {
                            match PathBuf::from(anon_home).canonicalize() {
                                Ok(home_canon) => {
                                    state.cwd = home_canon.to_string_lossy().to_string();
                                    state.home_dir = state.cwd.clone();
                                    state.authenticated = true;
                                    control_stream.write_response(b"230 Anonymous user logged in\r\n", "FTP response").await;
                                    tracing::info!(
                                        client_ip = %client_ip,
                                        username = "anonymous",
                                        action = "LOGIN",
                                        protocol = "FTP",
                                        "Anonymous user logged in"
                                    );
                                }
                                Err(e) => {
                                    tracing::error!("PASS failed: cannot canonicalize anonymous home directory '{}': {}", anon_home, e);
                                    control_stream.write_response(b"550 Anonymous home directory not found\r\n", "FTP response").await;
                                    state.current_user = None;
                                }
                            }
                        } else {
                            tracing::error!("PASS failed: anonymous access allowed but no anonymous_home configured");
                            control_stream.write_response(b"530 Anonymous home directory not configured\r\n", "FTP response").await;
                            state.current_user = None;
                        }
                    } else {
                        control_stream.write_response(b"530 Anonymous access not allowed\r\n", "FTP response").await;
                    }
                } else {
                    let password = password.as_deref().unwrap_or("");
                    let (auth_result, home_dir_opt) = {
                        let mut users = user_manager.lock();
                        if users.get_user(username).is_none() {
                            let _ = users.reload(&Config::get_users_path());
                        }
                        let result = users.authenticate(username, password);
                        let home = users.get_user(username).map(|u| u.home_dir.clone());
                        (result, home)
                    };

                    match auth_result {
                        Ok(true) => {
                            state.authenticated = true;
                            if let Some(home_dir) = home_dir_opt {
                                match PathBuf::from(&home_dir).canonicalize() {
                                    Ok(home_canon) => {
                                        state.cwd = home_canon.to_string_lossy().to_string();
                                        state.home_dir = state.cwd.clone();
                                    }
                                    Err(e) => {
                                        tracing::error!("PASS failed: cannot canonicalize user home directory '{}': {}", home_dir, e);
                                        control_stream.write_response(b"550 Home directory not found\r\n", "FTP response").await;
                                        state.authenticated = false;
                                        state.current_user = None;
                                        return Ok(true);
                                    }
                                }
                            }
                            control_stream.write_response(b"230 User logged in\r\n", "FTP response").await;
                            tracing::info!(
                                client_ip = %client_ip,
                                username = %username,
                                action = "LOGIN",
                                protocol = "FTP",
                                "User {} logged in", username
                            );
                        }
                        Ok(false) => {
                            tracing::warn!(
                                client_ip = %client_ip,
                                username = %username,
                                action = "AUTH_FAIL",
                                protocol = "FTP",
                                "Authentication failed for user {}", username
                            );
                            control_stream.write_response(b"530 Not logged in, user cannot be authenticated\r\n", "FTP response").await;
                        }
                        Err(e) => {
                            tracing::error!(
                                client_ip = %client_ip,
                                username = %username,
                                action = "AUTH_ERROR",
                                "Authentication error for user {}: {}", username, e
                            );
                            control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                        }
                    }
                }
            } else {
                control_stream.write_response(b"530 Please login with USER and PASS\r\n", "FTP response").await;
            }
        }

        QUIT => {
            control_stream.write_response(b"221 Goodbye\r\n", "FTP response").await;
            return Ok(false);
        }

        SYST => {
            control_stream.write_response(b"215 UNIX Type: L8\r\n", "FTP response").await;
        }

        FEAT => {
            let mut features = "211-Features:\r\n SIZE\r\n MDTM\r\n REST STREAM\r\n PASV\r\n EPSV\r\n EPRT\r\n PORT\r\n MLST\r\n MLSD\r\n MODE S\r\n STRU F\r\n UTF8\r\n TVFS\r\n".to_string();
            if tls_config.is_tls_available() {
                features.push_str(" AUTH TLS\r\n PBSZ\r\n PROT\r\n CCC\r\n");
                // RFC 2228 Security Extensions
                features.push_str(" MIC\r\n CONF\r\n ENC\r\n");
            }
            features.push_str("211 End\r\n");
            control_stream.write_response(features.as_bytes(), "FTP response").await;
        }

        NOOP => {
            control_stream.write_response(b"200 OK\r\n", "FTP response").await;
        }

        PWD | XPWD => {
            match to_ftp_path(std::path::Path::new(&state.cwd), std::path::Path::new(&state.home_dir)) {
                Ok(ftp_path) => {
                    control_stream.write_response(format!("257 \"{}\"\r\n", ftp_path).as_bytes(), "FTP response").await;
                }
                Err(e) => {
                    tracing::error!("PWD failed: {}", e);
                    control_stream.write_response(b"550 Failed to get current directory\r\n", "FTP response").await;
                }
            }
        }

        CWD(dir) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }
            if let Some(dir) = dir {
                match state.resolve_path(dir) {
                    Ok(new_path) => {
                        if new_path.exists() && new_path.is_dir() && path_starts_with_ignore_case(&new_path, &state.home_dir) {
                            state.cwd = new_path.to_string_lossy().to_string();
                            control_stream.write_response(b"250 Directory successfully changed\r\n", "FTP response").await;
                        } else {
                            control_stream.write_response(b"550 Failed to change directory\r\n", "FTP response").await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("CWD failed for '{}': {}", dir, e);
                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                    }
                }
            } else {
                control_stream.write_response(b"501 Syntax error: CWD requires directory parameter\r\n", "FTP response").await;
            }
        }

        CDUP | XCUP => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }
            match state.resolve_path("..") {
                Ok(new_path) => {
                    if path_starts_with_ignore_case(&new_path, &state.home_dir) && new_path.exists() {
                        state.cwd = new_path.to_string_lossy().to_string();
                        control_stream.write_response(b"250 Directory changed\r\n", "FTP response").await;
                    } else {
                        control_stream.write_response(b"550 Cannot change to parent directory: Permission denied\r\n", "FTP response").await;
                    }
                }
                Err(e) => {
                    tracing::warn!("CDUP failed: {}", e);
                    control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                }
            }
        }

        TYPE(type_code) => {
            if let Some(type_code) = type_code {
                let type_upper = type_code.to_uppercase();
                let parts: Vec<&str> = type_upper.split_whitespace().collect();
                let main_type = parts.first().copied().unwrap_or("");
                let sub_type = parts.get(1).copied().unwrap_or("N");

                match main_type {
                    "I" => {
                        state.transfer_mode = "binary".to_string();
                        control_stream.write_response(b"200 Type set to I (Binary)\r\n", "FTP response").await;
                    }
                    "L" => {
                        if sub_type == "8" {
                            state.transfer_mode = "binary".to_string();
                            control_stream.write_response(b"200 Type set to L 8 (Local byte size 8)\r\n", "FTP response").await;
                        } else {
                            control_stream.write_response(b"504 Only L 8 is supported\r\n", "FTP response").await;
                        }
                    }
                    "A" => {
                        match sub_type {
                            "N" | "" => {
                                state.transfer_mode = "ascii".to_string();
                                control_stream.write_response(b"200 Type set to A (ASCII Non-print)\r\n", "FTP response").await;
                            }
                            "T" => {
                                state.transfer_mode = "ascii".to_string();
                                control_stream.write_response(b"200 Type set to A T (ASCII Telnet format)\r\n", "FTP response").await;
                            }
                            "C" => {
                                control_stream.write_response(b"504 ASA carriage control not supported\r\n", "FTP response").await;
                            }
                            _ => {
                                control_stream.write_response(b"501 Unknown subtype\r\n", "FTP response").await;
                            }
                        }
                    }
                    "E" => {
                        control_stream.write_response(b"504 EBCDIC not supported, use A or I\r\n", "FTP response").await;
                    }
                    _ => {
                        control_stream.write_response(b"501 Unknown type\r\n", "FTP response").await;
                    }
                }
            } else {
                let type_str = match state.transfer_mode.as_str() {
                    "binary" => "200 Type is I (Binary)\r\n",
                    "ascii" => "200 Type is A (ASCII)\r\n",
                    _ => "200 Type set\r\n",
                };
                control_stream.write_response(type_str.as_bytes(), "FTP response").await;
            }
        }

        MODE(mode) => {
            if let Some(mode) = mode {
                match mode.to_uppercase().as_str() {
                    "S" => {
                        state.transfer_mode_type = TransferModeType::Stream;
                        control_stream.write_response(b"200 Mode set to Stream\r\n", "FTP response").await;
                    }
                    "B" => {
                        state.transfer_mode_type = TransferModeType::Block;
                        control_stream.write_response(b"200 Mode set to Block\r\n", "FTP response").await;
                    }
                    "C" => {
                        state.transfer_mode_type = TransferModeType::Compressed;
                        control_stream.write_response(b"200 Mode set to Compressed\r\n", "FTP response").await;
                    }
                    _ => {
                        control_stream.write_response(b"501 Unknown mode\r\n", "FTP response").await;
                    }
                }
            } else {
                control_stream.write_response(b"501 Syntax error: MODE requires parameter\r\n", "FTP response").await;
            }
        }

        STRU(structure) => {
            if let Some(structure) = structure {
                match structure.to_uppercase().as_str() {
                    "F" => {
                        state.file_structure = FileStructure::File;
                        control_stream.write_response(b"200 Structure set to File\r\n", "FTP response").await;
                    }
                    "R" => {
                        state.file_structure = FileStructure::Record;
                        control_stream.write_response(b"200 Structure set to Record\r\n", "FTP response").await;
                    }
                    "P" => {
                        state.file_structure = FileStructure::Page;
                        control_stream.write_response(b"200 Structure set to Page\r\n", "FTP response").await;
                    }
                    _ => {
                        control_stream.write_response(b"501 Unknown structure\r\n", "FTP response").await;
                    }
                }
            } else {
                control_stream.write_response(b"501 Syntax error: STRU requires parameter\r\n", "FTP response").await;
            }
        }

        ALLO => {
            control_stream.write_response(b"200 ALLO command successful\r\n", "FTP response").await;
        }

        OPTS(opts_arg) => {
            if let Some(opts_arg) = opts_arg {
                let opts_upper = opts_arg.to_uppercase();
                if opts_upper.starts_with("UTF8") || opts_upper.starts_with("UTF-8") {
                    control_stream.write_response(b"200 UTF8 enabled\r\n", "FTP response").await;
                } else if opts_upper.starts_with("MODE") {
                    control_stream.write_response(b"200 Mode set\r\n", "FTP response").await;
                } else {
                    control_stream.write_response(b"200 Options set\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"200 Options set\r\n", "FTP response").await;
            }
        }

        REST(offset_str) => {
            if let Some(offset_str) = offset_str {
                if let Ok(offset) = offset_str.parse::<u64>() {
                    state.rest_offset = offset;
                    control_stream.write_response(format!("350 Restarting at {}\r\n", offset).as_bytes(), "FTP response").await;
                    tracing::debug!(
                        client_ip = %client_ip,
                        username = ?state.current_user.as_deref(),
                        action = "REST",
                        "REST command: offset {}", offset
                    );
                } else {
                    control_stream.write_response(b"501 Syntax error in REST parameter\r\n", "FTP response").await;
                }
            } else {
                state.rest_offset = 0;
                control_stream.write_response(b"350 Restarting at 0\r\n", "FTP response").await;
            }
        }

        PASV => {
            let ((port_min, port_max), bind_ip, passive_ip_override, masquerade_address) = {
                let cfg = config.lock();
                (cfg.ftp.passive_ports, cfg.ftp.bind_ip.clone(), cfg.ftp.passive_ip_override.clone(), cfg.ftp.masquerade_address.clone())
            };

            let passive_port = match state.passive_manager.try_bind_port(port_min, port_max, &bind_ip).await {
                Ok(port) => port,
                Err(e) => {
                    control_stream.write_response(format!("425 Could not enter passive mode: {}\r\n", e).as_bytes(), "FTP response").await;
                    return Ok(true);
                }
            };

            state.passive_mode = true;
            state.data_port = Some(passive_port);

            // 优先级：masquerade_address > passive_ip_override > bind_ip/client_ip
            let response_ip = if let Some(ref masq_addr) = masquerade_address {
                // 如果配置了伪装地址（域名或 IP），优先使用
                // 尝试解析域名获取 IP 地址
                if is_domain_name(masq_addr.as_str()) {
                    // 如果是域名，尝试解析
                    resolve_domain_to_ip(masq_addr).unwrap_or_else(|| masq_addr.clone())
                } else {
                    // 直接是 IP 地址
                    masq_addr.clone()
                }
            } else if let Some(ref override_ip) = passive_ip_override {
                // 其次使用被动模式 IP 覆盖
                override_ip.clone()
            } else if bind_ip == "0.0.0.0" || bind_ip.is_empty() {
                // 如果绑定的是 0.0.0.0，使用客户端 IP
                client_ip.to_string()
            } else {
                // 否则使用绑定的 IP
                bind_ip.clone()
            };

            let ip_parts: Vec<&str> = response_ip.split('.').collect();
            if ip_parts.len() != 4 {
                control_stream.write_response(b"425 Invalid IP address format\r\n", "FTP response").await;
                return Ok(true);
            }

            let p1 = passive_port >> 8;
            let p2 = passive_port & 0xFF;

            control_stream.write_response(
                format!(
                    "227 Entering Passive Mode ({},{},{},{},{},{}).\r\n",
                    ip_parts[0], ip_parts[1], ip_parts[2], ip_parts[3], p1, p2
                )
            .as_bytes(), "PASV response").await;

            tracing::info!(
                client_ip = %client_ip,
                username = ?state.current_user.as_deref(),
                action = "PASV",
                protocol = "FTP",
                "PASV mode: port {}", passive_port
            );
        }

        EPSV => {
            let ((port_min, port_max), bind_ip) = {
                let cfg = config.lock();
                (cfg.ftp.passive_ports, cfg.ftp.bind_ip.clone())
            };

            let passive_port = match state.passive_manager.try_bind_port(port_min, port_max, &bind_ip).await {
                Ok(port) => port,
                Err(e) => {
                    control_stream.write_response(format!("425 Could not enter extended passive mode: {}\r\n", e).as_bytes(), "FTP response").await;
                    return Ok(true);
                }
            };

            state.passive_mode = true;
            state.data_port = Some(passive_port);

            control_stream.write_response(
                format!("229 Entering Extended Passive Mode (|||{}|)\r\n", passive_port).as_bytes(),
            "EPSV response").await;
        }

        PORT(data) => {
            if let Some(data) = data {
                let parts: Vec<u16> = data.split(',').filter_map(|s| s.parse().ok()).collect();
                if parts.len() == 6 {
                    if !state.validate_port_ip(data) {
                        control_stream.write_response(b"500 PORT command rejected: IP address must match control connection\r\n", "FTP response").await;
                        return Ok(true);
                    }
                    
                    let port = parts[4] * 256 + parts[5];
                    let addr = format!("{}.{}.{}.{}:{}", parts[0], parts[1], parts[2], parts[3], port);
                    state.data_port = Some(port);
                    state.data_addr = Some(addr);
                    state.passive_mode = false;
                    control_stream.write_response(b"200 PORT command successful\r\n", "FTP response").await;
                } else {
                    control_stream.write_response(b"501 Syntax error in parameters or arguments\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"501 Syntax error: PORT requires parameters\r\n", "FTP response").await;
            }
        }

        EPRT(data) => {
            if let Some(data) = data {
                let parts: Vec<&str> = data.split('|').collect();
                if parts.len() >= 4 {
                    let net_proto = parts[1];
                    let net_addr = parts[2];
                    let tcp_port = parts[3];

                    match net_proto {
                        "1" => {
                            if let Ok(port) = tcp_port.parse::<u16>() {
                                if !state.validate_eprt_ip(net_addr) {
                                    control_stream.write_response(b"500 EPRT command rejected: IP address must match control connection\r\n", "FTP response").await;
                                    return Ok(true);
                                }
                                state.data_port = Some(port);
                                state.data_addr = Some(format!("{}:{}", net_addr, port));
                                state.passive_mode = false;
                                control_stream.write_response(b"200 EPRT command successful\r\n", "FTP response").await;
                            } else {
                                control_stream.write_response(b"501 Invalid port number\r\n", "FTP response").await;
                            }
                        }
                        "2" => {
                            if let Ok(port) = tcp_port.parse::<u16>() {
                                if !state.validate_eprt_ip(net_addr) {
                                    control_stream.write_response(b"500 EPRT command rejected: IP address must match control connection\r\n", "FTP response").await;
                                    return Ok(true);
                                }
                                state.data_port = Some(port);
                                state.data_addr = Some(format!("[{}]:{}", net_addr, port));
                                state.passive_mode = false;
                                control_stream.write_response(b"200 EPRT command successful (IPv6)\r\n", "FTP response").await;
                            } else {
                                control_stream.write_response(b"501 Invalid port number\r\n", "FTP response").await;
                            }
                        }
                        _ => {
                            control_stream.write_response(b"522 Protocol not supported, use (1,2)\r\n", "FTP response").await;
                        }
                    }
                } else {
                    control_stream.write_response(b"501 Syntax error in EPRT parameters\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"501 Syntax error: EPRT requires parameters\r\n", "FTP response").await;
            }
        }

        ABOR => {
            state.abort_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            state.rest_offset = 0;
            control_stream.write_response(b"426 Connection closed; transfer aborted\r\n", "FTP response").await;
            control_stream.write_response(b"226 Abort successful\r\n", "FTP response").await;
        }

        REIN => {
            state.authenticated = false;
            state.current_user = None;
            state.cwd = String::new();
            state.home_dir = String::new();
            state.data_port = None;
            state.data_addr = None;
            state.rest_offset = 0;
            state.rename_from = None;
            state.data_protection = false;
            state.pbsz_set = false;
            control_stream.write_response(b"220 Service ready for new user\r\n", "FTP response").await;
        }

        ACCT => {
            control_stream.write_response(b"202 Account not required\r\n", "FTP response").await;
        }

        HELP(cmd) => {
            if let Some(cmd) = cmd {
                let help_text = match cmd.to_uppercase().as_str() {
                    "USER" => "214 USER <username>: Specify user name for authentication. Use 'anonymous' or 'ftp' for anonymous access.\r\n",
                    "PASS" => "214 PASS <password>: Specify password for authentication. For anonymous access, use email as password.\r\n",
                    "ACCT" => "214 ACCT <account>: Send account information (not required by this server).\r\n",
                    "CWD" => "214 CWD <directory>: Change working directory to the specified path. Supports relative and absolute paths.\r\n",
                    "CDUP" => "214 CDUP: Change to parent directory (same as CWD ..).\r\n",
                    "XCUP" => "214 XCUP: Change to parent directory (deprecated, use CDUP).\r\n",
                    "PWD" => "214 PWD: Print current working directory path.\r\n",
                    "XPWD" => "214 XPWD: Print current working directory (deprecated, use PWD).\r\n",
                    "LIST" => "214 LIST [<path>]: List directory contents in Unix format. If no path specified, lists current directory.\r\n",
                    "NLST" => "214 NLST [<path>]: List directory names only (no details). Useful for automated scripts.\r\n",
                    "MLSD" => "214 MLSD [<path>]: List directory contents with machine-readable facts (RFC 3659).\r\n",
                    "MLST" => "214 MLST [<path>]: Show facts for a single file/directory (RFC 3659).\r\n",
                    "RETR" => "214 RETR <filename>: Retrieve/download a file from the server. Supports REST for resume.\r\n",
                    "STOR" => "214 STOR <filename>: Store/upload a file to the server. Overwrites existing files.\r\n",
                    "STOU" => "214 STOU: Store file with unique name (server generates filename). Returns the generated name.\r\n",
                    "APPE" => "214 APPE <filename>: Append data to existing file, or create if not exists.\r\n",
                    "DELE" => "214 DELE <filename>: Delete a file from the server.\r\n",
                    "MKD" => "214 MKD <directory>: Create a new directory.\r\n",
                    "XMKD" => "214 XMKD <directory>: Create directory (deprecated, use MKD).\r\n",
                    "RMD" => "214 RMD <directory>: Remove an empty directory.\r\n",
                    "XRMD" => "214 XRMD <directory>: Remove directory (deprecated, use RMD).\r\n",
                    "RNFR" => "214 RNFR <filename>: Specify rename-from filename (first part of rename sequence).\r\n",
                    "RNTO" => "214 RNTO <filename>: Specify rename-to filename (second part of rename sequence).\r\n",
                    "PASV" => "214 PASV: Enter passive mode for data transfer. Server opens a port for client to connect.\r\n",
                    "EPSV" => "214 EPSV: Enter extended passive mode (supports IPv6, RFC 2428).\r\n",
                    "PORT" => "214 PORT <h1,h2,h3,h4,p1,p2>: Enter active mode. Client IP must match control connection.\r\n",
                    "EPRT" => "214 EPRT |<netproto>|<netaddr>|<tcpport>|: Extended active mode (supports IPv6, RFC 2428).\r\n",
                    "TYPE" => "214 TYPE <type>: Set transfer type. A=ASCII, I=Binary(Image), L 8=Local byte size 8.\r\n",
                    "MODE" => "214 MODE <mode>: Set transfer mode. S=Stream, B=Block, C=Compressed.\r\n",
                    "STRU" => "214 STRU <structure>: Set file structure. F=File, R=Record, P=Page.\r\n",
                    "REST" => "214 REST <offset>: Set restart marker for resuming transfers. Use before RETR or STOR.\r\n",
                    "SIZE" => "214 SIZE <filename>: Get file size in bytes (RFC 3659).\r\n",
                    "MDTM" => "214 MDTM <filename>: Get file modification time in YYYYMMDDHHMMSS format (RFC 3659).\r\n",
                    "ABOR" => "214 ABOR: Abort current data transfer and close data connection.\r\n",
                    "QUIT" => "214 QUIT: Disconnect from server and close control connection.\r\n",
                    "REIN" => "214 REIN: Reinitialize connection, reset all parameters (stay connected).\r\n",
                    "SYST" => "214 SYST: Return system type (returns 'UNIX Type: L8').\r\n",
                    "FEAT" => "214 FEAT: List server-supported features and extensions.\r\n",
                    "STAT" => "214 STAT [<path>]: Without parameter: show server status. With parameter: show file/directory info.\r\n",
                    "HELP" => "214 HELP [<command>]: Show help information. Without parameter: list all commands.\r\n",
                    "NOOP" => "214 NOOP: No operation, returns 200 OK. Used to keep connection alive.\r\n",
                    "SITE" => "214 SITE <command>: Execute server-specific commands (CHMOD, IDLE, HELP).\r\n",
                    "AUTH" => "214 AUTH <type>: Initiate TLS/SSL authentication. Type can be TLS, TLS-C, or SSL.\r\n",
                    "PBSZ" => "214 PBSZ <size>: Set protection buffer size (must be 0 for TLS). Use after AUTH.\r\n",
                    "PROT" => "214 PROT <level>: Set data channel protection level. C=Clear, P=Private(encrypted).\r\n",
                    "CCC" => "214 CCC: Clear command channel (revert to unencrypted control connection).\r\n",
                    // RFC 2228 Security Commands
                    "ADAT" => "214 ADAT <data>: Authentication/Security Data (RFC 2228). Used for Kerberos/GSSAPI.\r\n",
                    "MIC" => "214 MIC <data>: Integrity Protected Command (RFC 2228). Command with integrity protection.\r\n",
                    "CONF" => "214 CONF <data>: Confidentiality Protected Command (RFC 2228). Encrypted command.\r\n",
                    "ENC" => "214 ENC <data>: Privacy Protected Command (RFC 2228). Fully encrypted command.\r\n",
                    "OPTS" => "214 OPTS <option>: Set options (e.g., OPTS UTF8 ON).\r\n",
                    "ALLO" => "214 ALLO <size>: Allocate storage space (no-op on this server, returns success).\r\n",
                    _ => "214 Unknown command or no help available\r\n",
                };
                control_stream.write_response(help_text.as_bytes(), "FTP response").await;
            } else {
                control_stream.write_response(b"214-The following commands are recognized:\r\n", "FTP response").await;
                control_stream.write_response(b"214-Connection and Authentication:\r\n", "FTP response").await;
                control_stream.write_response(b"214-  USER PASS ACCT AUTH PBSZ PROT CCC QUIT REIN\r\n", "FTP response").await;
                control_stream.write_response(b"214-RFC 2228 Security Extensions (requires TLS):\r\n", "FTP response").await;
                control_stream.write_response(b"214-  ADAT MIC CONF ENC\r\n", "FTP response").await;
                control_stream.write_response(b"214-Directory Operations:\r\n", "FTP response").await;
                control_stream.write_response(b"214-  CWD CDUP XCUP PWD XPWD MKD XMKD RMD XRMD\r\n", "FTP response").await;
                control_stream.write_response(b"214-File Operations:\r\n", "FTP response").await;
                control_stream.write_response(b"214-  RETR STOR STOU APPE DELE RNFR RNTO REST SIZE MDTM\r\n", "FTP response").await;
                control_stream.write_response(b"214-Directory Listing:\r\n", "FTP response").await;
                control_stream.write_response(b"214-  LIST NLST MLSD MLST STAT\r\n", "FTP response").await;
                control_stream.write_response(b"214-Transfer Settings:\r\n", "FTP response").await;
                control_stream.write_response(b"214-  TYPE MODE STRU PASV EPSV PORT EPRT\r\n", "FTP response").await;
                control_stream.write_response(b"214-Miscellaneous:\r\n", "FTP response").await;
                control_stream.write_response(b"214-  SYST FEAT HELP NOOP SITE OPTS ALLO ABOR\r\n", "FTP response").await;
                control_stream.write_response(b"214-Use 'HELP <command>' for detailed information on a specific command.\r\n", "FTP response").await;
                control_stream.write_response(b"214 Direct comments to admin\r\n", "FTP response").await;
            }
        }

        STAT => {
            if let Some(ref username) = state.current_user {
                control_stream.write_response(b"211-FTP server status:\r\n", "FTP response").await;
                control_stream.write_response(format!("211-Connected to: {}\r\n", client_ip).as_bytes(), "FTP response").await;
                control_stream.write_response(format!("211-Logged in as: {}\r\n", username).as_bytes(), "FTP response").await;
                control_stream.write_response(format!("211-Current directory: {}\r\n", state.cwd).as_bytes(), "FTP response").await;
                control_stream.write_response(format!("211-Transfer mode: {}\r\n", if state.passive_mode { "Passive" } else { "Active" }).as_bytes(), "FTP response").await;
                control_stream.write_response(format!("211-Transfer type: {}\r\n", state.transfer_mode.to_uppercase()).as_bytes(), "FTP response").await;
                control_stream.write_response(format!("211-File structure: {:?}\r\n", state.file_structure).as_bytes(), "FTP response").await;
                control_stream.write_response(format!("211-Transfer mode type: {:?}\r\n", state.transfer_mode_type).as_bytes(), "FTP response").await;
                control_stream.write_response(format!("211-TLS: {}\r\n", if state.tls_enabled { "Enabled" } else { "Disabled" }).as_bytes(), "FTP response").await;
                if state.tls_enabled {
                    control_stream.write_response(format!("211-Data protection: {}\r\n", if state.data_protection { "Private (encrypted)" } else { "Clear" }).as_bytes(), "FTP response").await;
                }
                if state.rest_offset > 0 {
                    control_stream.write_response(format!("211-Restart offset: {}\r\n", state.rest_offset).as_bytes(), "FTP response").await;
                }
                if let Some(data_port) = state.data_port {
                    control_stream.write_response(format!("211-Data port: {}\r\n", data_port).as_bytes(), "FTP response").await;
                }
                if let Some(ref rename_from) = state.rename_from {
                    control_stream.write_response(format!("211-Rename from: {}\r\n", rename_from).as_bytes(), "FTP response").await;
                }
                control_stream.write_response(b"211 End\r\n", "FTP response").await;
            } else {
                control_stream.write_response(b"211 FTP server status - Not logged in\r\n", "FTP response").await;
            }
        }

        SITE(cmd) => {
            if let Some(site_cmd) = cmd {
                let site_parts: Vec<&str> = site_cmd.splitn(2, ' ').collect();
                let site_action = site_parts[0].to_uppercase();
                let site_arg = site_parts.get(1).map(|s| s.trim());

                match site_action.as_str() {
                    "HELP" => {
                        control_stream.write_response(b"214-The following SITE commands are recognized:\r\n", "FTP response").await;
                        control_stream.write_response(b"214-CHMOD IDLE HELP\r\n", "FTP response").await;
                        control_stream.write_response(b"214 End\r\n", "FTP response").await;
                    }
                    "IDLE" => {
                        if let Some(secs_str) = site_arg {
                            if let Ok(secs) = secs_str.parse::<u64>() {
                                control_stream.write_response(format!("200 Idle timeout set to {} seconds\r\n", secs).as_bytes(), "FTP response").await;
                            } else {
                                control_stream.write_response(b"501 Invalid idle time\r\n", "FTP response").await;
                            }
                        } else {
                            control_stream.write_response(b"501 SITE IDLE requires time parameter\r\n", "FTP response").await;
                        }
                    }
                    "CHMOD" => {
                        if !state.authenticated {
                            control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                            return Ok(true);
                        }
                        
                        if let Some(chmod_args) = site_arg {
                            let chmod_parts: Vec<&str> = chmod_args.splitn(2, ' ').collect();
                            if chmod_parts.len() == 2 {
                                let mode = chmod_parts[0];
                                let target = chmod_parts[1];
                                
                                let target_path = match state.resolve_path(target) {
                                    Ok(p) => p,
                                    Err(e) => {
                                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                                        return Ok(true);
                                    }
                                };
                                
                                if !path_starts_with_ignore_case(&target_path, &state.home_dir) {
                                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                                    return Ok(true);
                                }
                                
                                if let Ok(_mode_val) = u32::from_str_radix(mode, 8) {
                                    #[cfg(windows)]
                                    {
                                        control_stream.write_response(b"200 CHMOD command accepted (Windows: permissions managed by ACL)\r\n", "FTP response").await;
                                    }
                                    #[cfg(not(windows))]
                                    {
                                        use std::os::unix::fs::PermissionsExt;
                                        match std::fs::set_permissions(&target_path, std::fs::Permissions::from_mode(mode_val)) {
                                            Ok(()) => {
                                                control_stream.write_response(format!("200 CHMOD {} {}\r\n", mode, target).as_bytes(), "FTP response").await;
                                            }
                                            Err(e) => {
                                                control_stream.write_response(format!("550 CHMOD failed: {}\r\n", e).as_bytes(), "FTP response").await;
                                            }
                                        }
                                    }
                                } else {
                                    control_stream.write_response(b"501 Invalid mode format\r\n", "FTP response").await;
                                }
                            } else {
                                control_stream.write_response(b"501 SITE CHMOD requires mode and filename\r\n", "FTP response").await;
                            }
                        } else {
                            control_stream.write_response(b"501 SITE CHMOD requires parameters\r\n", "FTP response").await;
                        }
                    }
                    _ => {
                        control_stream.write_response(format!("500 Unknown SITE command: {}\r\n", site_action).as_bytes(), "FTP response").await;
                    }
                }
            } else {
                control_stream.write_response(b"501 SITE command requires parameter\r\n", "FTP response").await;
            }
        }

        LIST(path) | NLST(path) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }

            let can_list = if state.current_user.as_deref() == Some("anonymous") {
                true
            } else {
                let users = user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_list)
            };

            if !can_list {
                control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                return Ok(true);
            }

            let list_path = if let Some(path_arg) = path {
                match resolve_directory_path(&state.cwd, &state.home_dir, path_arg) {
                    Ok(path) => path,
                    Err(PathResolveError::PathEscape) => {
                        control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                        return Ok(true);
                    }
                    Err(PathResolveError::NotADirectory) => {
                        control_stream.write_response(b"550 Not a directory\r\n", "FTP response").await;
                        return Ok(true);
                    }
                    Err(PathResolveError::NotFound) => {
                        control_stream.write_response(b"550 Directory not found\r\n", "FTP response").await;
                        return Ok(true);
                    }
                    Err(_) => {
                        control_stream.write_response(b"550 Failed to resolve path\r\n", "FTP response").await;
                        return Ok(true);
                    }
                }
            } else {
                PathBuf::from(&state.cwd)
            };

            control_stream.write_response(b"150 Here comes the directory listing\r\n", "FTP response").await;

            let current_username = state.current_user.clone().unwrap_or_else(|| "anonymous".to_string());
            let is_ascii = state.transfer_mode == "ascii";
            
            if let Ok(mut data_stream) = transfer::get_data_connection(
                state.passive_mode,
                state.data_port,
                &state.data_addr,
                client_ip,
                &mut state.passive_manager,
            ).await {
                let is_nlst = matches!(cmd, NLST(_));
                if let Err(e) = transfer::send_directory_listing(
                    &mut data_stream,
                    &list_path,
                    &current_username,
                    is_nlst,
                    is_ascii,
                ).await {
                    tracing::warn!("LIST/NLST transfer error: {}", e);
                }
            }

            if state.passive_mode
                && let Some(port) = state.data_port {
                    state.passive_manager.remove_listener(port);
                }

            control_stream.write_response(b"226 Transfer complete\r\n", "FTP response").await;
        }

        MLSD(path) | MLST(path) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }

            let can_list = {
                let users = user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_none_or(|u| u.permissions.can_list)
            };

            if !can_list {
                control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                return Ok(true);
            }

            let target_path = if let Some(path_arg) = path {
                match resolve_directory_path(&state.cwd, &state.home_dir, path_arg) {
                    Ok(path) => path,
                    Err(_) => {
                        control_stream.write_response(b"550 Failed to resolve path\r\n", "FTP response").await;
                        return Ok(true);
                    }
                }
            } else {
                PathBuf::from(&state.cwd)
            };

            if matches!(cmd, MLST(_)) {
                if target_path.exists() && path_starts_with_ignore_case(&target_path, &state.home_dir) {
                    if let Ok(metadata) = tokio::fs::metadata(&target_path).await {
                        let owner = state.current_user.as_deref().unwrap_or("anonymous");
                        let facts = transfer::build_mlst_facts(&metadata, owner);
                        match to_ftp_path(&target_path, std::path::Path::new(&state.home_dir)) {
                            Ok(ftp_path) => {
                                let name = target_path.file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| target_path.to_string_lossy().to_string());
                                control_stream.write_response(format!("250-Listing {}\r\n {} {}\r\n250 End\r\n", ftp_path, facts, name).as_bytes(), "FTP response").await;
                            }
                            Err(e) => {
                                tracing::error!("MLST failed: {}", e);
                                control_stream.write_response(b"550 Failed to get file path\r\n", "FTP response").await;
                            }
                        }
                    } else {
                        control_stream.write_response(b"550 Failed to get file info\r\n", "FTP response").await;
                    }
                } else {
                    control_stream.write_response(b"550 File not found\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"150 Here comes the directory listing\r\n", "FTP response").await;

                let mlst_owner = state.current_user.clone().unwrap_or_else(|| "anonymous".to_string());
                
                if let Ok(mut data_stream) = transfer::get_data_connection(
                    state.passive_mode,
                    state.data_port,
                    &state.data_addr,
                    client_ip,
                    &mut state.passive_manager,
                ).await
                    && let Err(e) = transfer::send_mlsd_listing(
                        &mut data_stream,
                        &target_path,
                        &mlst_owner,
                    ).await {
                        tracing::warn!("MLSD transfer error: {}", e);
                    }

                if state.passive_mode
                    && let Some(port) = state.data_port {
                        state.passive_manager.remove_listener(port);
                    }

                control_stream.write_response(b"226 Transfer complete\r\n", "FTP response").await;
            }
        }

        RETR(filename) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }

            if let Some(filename) = filename {
                let file_path = match state.resolve_path(filename) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("RETR failed for '{}': {}", filename, e);
                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                        return Ok(true);
                    }
                };

                let normalized_home_dir = state.home_dir.replace('/', "\\");
                let normalized_file_path_str = file_path.to_string_lossy().replace('/', "\\");
                let starts_with_home = path_starts_with_ignore_case(&file_path, &state.home_dir) || normalized_file_path_str.to_lowercase().starts_with(&normalized_home_dir.to_lowercase());

                if !file_path.exists() || !file_path.is_file() || !starts_with_home {
                    tracing::warn!("RETR denied: path='{}', home='{}', exists={}, is_file={}, starts_with={}", 
                        file_path.display(), state.home_dir, file_path.exists(), file_path.is_file(), starts_with_home);
                    control_stream.write_response(b"550 File not found\r\n", "FTP response").await;
                    return Ok(true);
                }

                let (can_read, speed_limit_kbps) = {
                    let users = user_manager.lock();
                    let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                    (
                        user.is_none_or(|u| u.permissions.can_read),
                        user.and_then(|u| u.permissions.speed_limit_kbps)
                    )
                };

                if !can_read {
                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                    return Ok(true);
                }

                let file_metadata = match tokio::fs::metadata(&file_path).await {
                    Ok(m) => m,
                    Err(e) => {
                        control_stream.write_response(format!("450 File unavailable: {}\r\n", e).as_bytes(), "FTP response").await;
                        return Ok(true);
                    }
                };
                
                let file_size = file_metadata.len();
                let remaining = if state.rest_offset > 0 && state.rest_offset < file_size {
                    file_size - state.rest_offset
                } else {
                    file_size
                };

                if state.rest_offset > 0 {
                    control_stream.write_response(format!("110 Restart marker at {}\r\n", state.rest_offset).as_bytes(), "FTP response").await;
                }

                control_stream.write_response(
                    format!("150 Opening BINARY mode data connection ({} bytes)\r\n", remaining)
                        .as_bytes(), "RETR opening"
                ).await;

                let is_ascii = state.transfer_mode == "ascii";
                let rate_limiter = speed_limit_kbps.map(crate::core::rate_limiter::RateLimiter::new);
                
                if let Ok(mut data_stream) = transfer::get_data_connection(
                    state.passive_mode,
                    state.data_port,
                    &state.data_addr,
                    client_ip,
                    &mut state.passive_manager,
                ).await {
                    let abort = Arc::clone(&state.abort_flag);
                    if let Err(e) = transfer::send_file_with_limits(&mut data_stream, &file_path, state.rest_offset, abort, is_ascii, rate_limiter.as_ref()).await {
                        tracing::warn!("RETR transfer error: {}", e);
                    }
                }

                if state.passive_mode
                    && let Some(port) = state.data_port {
                        state.passive_manager.remove_listener(port);
                    }

                control_stream.write_response(b"226 Transfer complete\r\n", "FTP response").await;

                let final_size = tokio::fs::metadata(&file_path).await.map(|m| m.len()).unwrap_or(remaining);
                crate::file_op_log!(
                    download,
                    state.current_user.as_deref().unwrap_or("anonymous"),
                    client_ip,
                    &file_path.to_string_lossy(),
                    final_size,
                    "FTP"
                );

                state.rest_offset = 0;
            }
        }

        STOR(filename) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }

            if let Some(filename) = filename {
                let (can_write, quota_mb, speed_limit_kbps) = {
                    let users = user_manager.lock();
                    let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                    (
                        user.is_some_and(|u| u.permissions.can_write),
                        user.and_then(|u| u.permissions.quota_mb),
                        user.and_then(|u| u.permissions.speed_limit_kbps),
                    )
                };

                if !can_write {
                    tracing::warn!("STOR denied: user {} lacks write permission", state.current_user.as_deref().unwrap_or("unknown"));
                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                    return Ok(true);
                }

                let is_abs = filename.starts_with('/');
                tracing::debug!("STOR: raw_filename='{}', is_absolute={}, cwd='{}', home='{}', passive_mode={}, data_port={:?}", 
                    filename, is_abs, state.cwd, state.home_dir, state.passive_mode, state.data_port);
                
                let file_path = match state.resolve_path(filename) {
                    Ok(p) => {
                        tracing::debug!("STOR: resolved_path='{}'", p.display());
                        p
                    },
                    Err(e) => {
                        tracing::warn!("STOR failed for '{}': {}", filename, e);
                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                        return Ok(true);
                    }
                };

                let normalized_home_dir = state.home_dir.replace('/', "\\");
                let normalized_file_path_str = file_path.to_string_lossy().replace('/', "\\");
                let starts_with_home = path_starts_with_ignore_case(&file_path, &state.home_dir) || normalized_file_path_str.to_lowercase().starts_with(&normalized_home_dir.to_lowercase());
                
                tracing::debug!("STOR: resolved='{}', normalized_home='{}', starts_with={}", 
                    file_path.display(), normalized_home_dir, starts_with_home);
                if !starts_with_home {
                    tracing::warn!("STOR denied: path outside home - {} (home: {})", file_path.display(), state.home_dir);
                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                    return Ok(true);
                }

                if let Some(quota) = quota_mb {
                    let current_usage = quota_manager.get_usage(state.current_user.as_deref().unwrap_or("anonymous")).await;
                    let quota_bytes = quota * 1024 * 1024;
                    if current_usage >= quota_bytes {
                        control_stream.write_response(b"552 Quota exceeded\r\n", "FTP response").await;
                        tracing::warn!(
                            client_ip = %client_ip,
                            username = ?state.current_user.as_deref(),
                            action = "QUOTA_EXCEEDED",
                            "Upload denied: quota exceeded for user {}", state.current_user.as_deref().unwrap_or("unknown")
                        );
                        return Ok(true);
                    }
                }

                let file_existed = file_path.exists();
                control_stream.write_response(b"150 Opening BINARY mode data connection\r\n", "FTP response").await;

                let mut transfer_success = false;
                let mut total_written: u64 = 0;
                let is_ascii = state.transfer_mode == "ascii";
                let rate_limiter = speed_limit_kbps.map(crate::core::rate_limiter::RateLimiter::new);

                if let Ok(mut data_stream) = transfer::get_data_connection(
                    state.passive_mode,
                    state.data_port,
                    &state.data_addr,
                    client_ip,
                    &mut state.passive_manager,
                ).await {
                    let abort = Arc::clone(&state.abort_flag);
                    let result = transfer::receive_file_with_limits(
                        &mut data_stream,
                        &file_path,
                        state.rest_offset,
                        abort,
                        is_ascii,
                        rate_limiter.as_ref(),
                    ).await;
                    match result {
                        Ok(written) => {
                            transfer_success = true;
                            total_written = written;
                        }
                        Err(e) => {
                            tracing::error!("STOR transfer error: {}", e);
                        }
                    }
                } else {
                    tracing::error!("STOR failed to get data connection for file: {}", file_path.display());
                }

                if state.passive_mode
                    && let Some(port) = state.data_port {
                        state.passive_manager.remove_listener(port);
                    }

                if transfer_success {
                    control_stream.write_response(b"226 Transfer complete\r\n", "FTP response").await;

                    let uploaded_size = tokio::fs::metadata(&file_path).await.map(|m| m.len()).unwrap_or(total_written);
                    
                    if quota_mb.is_some()
                        && let Err(e) = quota_manager.add_usage(state.current_user.as_deref().unwrap_or("anonymous"), uploaded_size).await {
                            tracing::error!("Failed to update quota usage: {}", e);
                    }
                    
                    if file_existed {
                        crate::file_op_log!(
                            update,
                            state.current_user.as_deref().unwrap_or("anonymous"),
                            client_ip,
                            &file_path.to_string_lossy(),
                            uploaded_size,
                            "FTP"
                        );
                    } else {
                        crate::file_op_log!(
                            upload,
                            state.current_user.as_deref().unwrap_or("anonymous"),
                            client_ip,
                            &file_path.to_string_lossy(),
                            uploaded_size,
                            "FTP"
                        );
                    }
                } else {
                    control_stream.write_response(b"451 Transfer failed\r\n", "FTP response").await;
                    crate::file_op_log!(
                        failed,
                        state.current_user.as_deref().unwrap_or("anonymous"),
                        client_ip,
                        "UPLOAD",
                        &file_path.to_string_lossy(),
                        "FTP",
                        "Transfer failed"
                    );
                }

                state.rest_offset = 0;
            } else {
                control_stream.write_response(b"501 Syntax error: STOR requires filename\r\n", "FTP response").await;
            }
        }

        APPE(filename) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }

            if let Some(filename) = filename {
                let can_append = {
                    let users = user_manager.lock();
                    let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                    user.is_some_and(|u| u.permissions.can_append)
                };

                if !can_append {
                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                    return Ok(true);
                }

                let file_path = match state.resolve_path(filename) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("APPE failed for '{}': {}", filename, e);
                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                        return Ok(true);
                    }
                };
                if !path_starts_with_ignore_case(&file_path, &state.home_dir) {
                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                    return Ok(true);
                }
                control_stream.write_response(b"150 Opening BINARY mode data connection for append\r\n", "FTP response").await;

                let is_ascii = state.transfer_mode == "ascii";

                if let Ok(mut data_stream) = transfer::get_data_connection(
                    state.passive_mode,
                    state.data_port,
                    &state.data_addr,
                    client_ip,
                    &mut state.passive_manager,
                ).await {
                    let abort = Arc::clone(&state.abort_flag);
                    if let Err(e) = transfer::receive_file_append(&mut data_stream, &file_path, abort, is_ascii).await {
                        tracing::warn!("APPE transfer error: {}", e);
                    }
                }

                if state.passive_mode
                    && let Some(port) = state.data_port {
                        state.passive_manager.remove_listener(port);
                    }

                control_stream.write_response(b"226 Transfer complete\r\n", "FTP response").await;

                let appended_size = tokio::fs::metadata(&file_path).await.map(|m| m.len()).unwrap_or(0);
                crate::file_op_log!(
                    state.current_user.as_deref().unwrap_or("anonymous"),
                    client_ip,
                    "APPEND",
                    &file_path.to_string_lossy(),
                    appended_size,
                    "FTP",
                    true,
                    "文件追加成功"
                );
            }
        }

        DELE(filename) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }

            let can_delete = {
                let users = user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_delete)
            };

            if !can_delete {
                control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                return Ok(true);
            }

            if let Some(filename) = filename {
                let file_path = match state.resolve_path(filename) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("DELE failed for '{}': {}", filename, e);
                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                        return Ok(true);
                    }
                };
                if !path_starts_with_ignore_case(&file_path, &state.home_dir) {
                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                    return Ok(true);
                }
                
                if !file_path.exists() {
                    control_stream.write_response(b"450 File unavailable: file not found\r\n", "FTP response").await;
                    return Ok(true);
                }
                
                if tokio::fs::remove_file(&file_path).await.is_ok() {
                    control_stream.write_response(b"250 File deleted\r\n", "FTP response").await;
                    crate::file_op_log!(
                        delete,
                        state.current_user.as_deref().unwrap_or("anonymous"),
                        client_ip,
                        &file_path.to_string_lossy(),
                        "FTP"
                    );
                } else {
                    control_stream.write_response(b"450 File unavailable: delete operation failed\r\n", "FTP response").await;
                }
            }
        }

        MKD(dirname) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }

            let can_mkdir = {
                let users = user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_mkdir)
            };

            if !can_mkdir {
                control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                return Ok(true);
            }

            if let Some(dirname) = dirname {
                let dir_path = match state.resolve_path(dirname) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("MKD failed for '{}': {}", dirname, e);
                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                        return Ok(true);
                    }
                };
                if !path_starts_with_ignore_case(&dir_path, &state.home_dir) {
                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                    return Ok(true);
                }
                if tokio::fs::create_dir_all(&dir_path).await.is_ok() {
                    match to_ftp_path(&dir_path, std::path::Path::new(&state.home_dir)) {
                        Ok(ftp_path) => {
                            control_stream.write_response(format!("257 \"{}\" created\r\n", ftp_path).as_bytes(), "FTP response").await;
                        }
                        Err(e) => {
                            tracing::error!("MKD failed to get ftp path: {}", e);
                            control_stream.write_response(b"257 Directory created\r\n", "FTP response").await;
                        }
                    }
                    crate::file_op_log!(
                        mkdir,
                        state.current_user.as_deref().unwrap_or("anonymous"),
                        client_ip,
                        &dir_path.to_string_lossy(),
                        "FTP"
                    );
                } else {
                    control_stream.write_response(b"550 Create directory operation failed\r\n", "FTP response").await;
                }
            }
        }

        RMD(dirname) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }

            let can_rmdir = {
                let users = user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_rmdir)
            };

            if !can_rmdir {
                control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                return Ok(true);
            }

            if let Some(dirname) = dirname {
                let dir_path = match state.resolve_path(dirname) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("RMD failed for '{}': {}", dirname, e);
                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                        return Ok(true);
                    }
                };
                if !path_starts_with_ignore_case(&dir_path, &state.home_dir) {
                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                    return Ok(true);
                }
                if tokio::fs::remove_dir_all(&dir_path).await.is_ok() {
                    control_stream.write_response(b"250 Directory removed\r\n", "FTP response").await;
                    crate::file_op_log!(
                        rmdir,
                        state.current_user.as_deref().unwrap_or("anonymous"),
                        client_ip,
                        &dir_path.to_string_lossy(),
                        "FTP"
                    );
                } else {
                    control_stream.write_response(b"550 Remove directory operation failed\r\n", "FTP response").await;
                }
            }
        }

        RNFR(from_name) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }

            let can_rename = {
                let users = user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_rename)
            };

            if !can_rename {
                control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                return Ok(true);
            }

            if let Some(from_name) = from_name {
                let from_path = match state.resolve_path(from_name) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("RNFR failed for '{}': {}", from_name, e);
                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                        return Ok(true);
                    }
                };
                tracing::debug!("RNFR: raw='{}', resolved='{}', exists={}, starts_with={}", 
                    from_name, from_path.display(), from_path.exists(), path_starts_with_ignore_case(&from_path, &state.home_dir));
                if from_path.exists() && path_starts_with_ignore_case(&from_path, &state.home_dir) {
                    state.rename_from = Some(from_path.to_string_lossy().to_string());
                    control_stream.write_response(b"350 File exists, ready for destination name\r\n", "FTP response").await;
                    tracing::debug!(
                        client_ip = %client_ip,
                        username = ?state.current_user.as_deref(),
                        action = "RNFR",
                        "RNFR: {}", from_path.display()
                    );
                } else {
                    tracing::warn!("RNFR failed: file not found or outside home - raw='{}', resolved='{}'", from_name, from_path.display());
                    control_stream.write_response(b"450 File unavailable: file not found\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"501 Syntax error: RNFR requires filename\r\n", "FTP response").await;
            }
        }

        RNTO(to_name) => {
            if let Some(ref from_path) = state.rename_from {
                if let Some(to_name) = to_name {
                    let to_path = match state.resolve_path(to_name) {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::warn!("RNTO failed for '{}': {}", to_name, e);
                            control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                            state.rename_from = None;
                            return Ok(true);
                        }
                    };
                    tracing::debug!("RNTO: raw='{}', resolved='{}', from='{}'", to_name, to_path.display(), from_path);
                    if !path_starts_with_ignore_case(&to_path, &state.home_dir) {
                        tracing::warn!("RNTO failed: destination outside home - {}", to_path.display());
                        control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                        state.rename_from = None;
                        return Ok(true);
                    }
                    let from_path_buf = PathBuf::from(from_path);
                    match tokio::fs::rename(&from_path_buf, &to_path).await {
                        Ok(()) => {
                            control_stream.write_response(b"250 Rename successful\r\n", "FTP response").await;
                            // 判断是重命名还是移动：检查父目录是否相同
                            let from_parent = from_path_buf.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                            let to_parent = to_path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                            if from_parent == to_parent {
                                crate::file_op_log!(
                                    rename,
                                    state.current_user.as_deref().unwrap_or("anonymous"),
                                    client_ip,
                                    from_path,
                                    &to_path.to_string_lossy(),
                                    "FTP"
                                );
                            } else {
                                crate::file_op_log!(
                                    move,
                                    state.current_user.as_deref().unwrap_or("anonymous"),
                                    client_ip,
                                    from_path,
                                    &to_path.to_string_lossy(),
                                    "FTP"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!("Rename failed: {} -> {}: {} (os error {})", from_path, to_path.display(), e, e.raw_os_error().unwrap_or(0));
                            control_stream.write_response(b"550 Rename failed\r\n", "FTP response").await;
                        }
                    }
                } else {
                    control_stream.write_response(b"501 Syntax error: RNTO requires filename\r\n", "FTP response").await;
                }
            } else {
                control_stream.write_response(b"503 Bad sequence of commands\r\n", "FTP response").await;
            }
            state.rename_from = None;
        }

        SIZE(filename) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }
            if let Some(filename) = filename {
                let file_path = match state.resolve_path(filename) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("SIZE failed for '{}': {}", filename, e);
                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                        return Ok(true);
                    }
                };
                if path_starts_with_ignore_case(&file_path, &state.home_dir) {
                    if let Ok(metadata) = tokio::fs::metadata(&file_path).await {
                        control_stream.write_response(format!("213 {}\r\n", metadata.len()).as_bytes(), "FTP response").await;
                    } else {
                        control_stream.write_response(b"450 File unavailable: file not found\r\n", "FTP response").await;
                    }
                } else {
                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                }
            }
        }

        MDTM(filename) => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }
            if let Some(filename) = filename {
                let file_path = match state.resolve_path(filename) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("MDTM failed for '{}': {}", filename, e);
                        control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                        return Ok(true);
                    }
                };
                if path_starts_with_ignore_case(&file_path, &state.home_dir) {
                    if let Ok(metadata) = tokio::fs::metadata(&file_path).await {
                        let mtime = transfer::get_file_mtime_raw(&metadata);
                        control_stream.write_response(format!("213 {}\r\n", mtime).as_bytes(), "FTP response").await;
                    } else {
                        control_stream.write_response(b"450 File unavailable: file not found\r\n", "FTP response").await;
                    }
                } else {
                    control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                }
            }
        }

        STOU => {
            if !state.authenticated {
                control_stream.write_response(b"530 Not logged in\r\n", "FTP response").await;
                return Ok(true);
            }

            let can_write = {
                let users = user_manager.lock();
                let user = state.current_user.as_ref().and_then(|u| users.get_user(u));
                user.is_some_and(|u| u.permissions.can_write)
            };

            if !can_write {
                control_stream.write_response(b"550 Permission denied\r\n", "FTP response").await;
                return Ok(true);
            }

            let file_path = match generate_unique_filename(state, 100).await {
                Ok(path) => path,
                Err(e) => {
                    tracing::warn!("STOU failed: {}", e);
                    control_stream.write_response(format!("550 {}\r\n", e).as_bytes(), "FTP response").await;
                    return Ok(true);
                }
            };

            let unique_name = file_path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            control_stream.write_response(format!("150 FILE: {}\r\n", unique_name).as_bytes(), "FTP response").await;

            let is_ascii = state.transfer_mode == "ascii";

            if let Ok(mut data_stream) = transfer::get_data_connection(
                state.passive_mode,
                state.data_port,
                &state.data_addr,
                client_ip,
                &mut state.passive_manager,
            ).await {
                let abort = Arc::clone(&state.abort_flag);
                if let Err(e) = transfer::receive_file(&mut data_stream, &file_path, 0, abort, is_ascii).await {
                    tracing::warn!("STOU transfer error: {}", e);
                }
            }

            if state.passive_mode
                && let Some(port) = state.data_port {
                    state.passive_manager.remove_listener(port);
                }

            control_stream.write_response(b"226 Transfer complete\r\n", "FTP response").await;

            let uploaded_size = tokio::fs::metadata(&file_path).await.map(|m| m.len()).unwrap_or(0);
            crate::file_op_log!(
                upload,
                state.current_user.as_deref().unwrap_or("anonymous"),
                client_ip,
                &file_path.to_string_lossy(),
                uploaded_size,
                "FTP"
            );
        }

        Unknown(cmd_str) => {
            control_stream.write_response(format!("202 Command not implemented: {}\r\n", cmd_str).as_bytes(), "FTP response").await;
        }
    }

    Ok(true)
}

async fn generate_unique_filename(state: &SessionState, max_attempts: u32) -> Result<PathBuf, String> {
    let base_name = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    for attempt in 0..max_attempts {
        let unique_name = if attempt == 0 {
            format!("stou_{}_{:04x}", base_name, rand::random::<u16>())
        } else {
            format!("stou_{}_{:04x}_{}", base_name, rand::random::<u16>(), attempt)
        };
        
        let file_path = match state.resolve_path(&unique_name) {
            Ok(p) => p,
            Err(e) => return Err(format!("Path resolution error: {}", e)),
        };
        
        if !path_starts_with_ignore_case(&file_path, &state.home_dir) {
            return Err("Generated path outside home directory".to_string());
        }
        
        if !file_path.exists() {
            return Ok(file_path);
        }
        
        tracing::debug!("STOU filename collision detected, retrying: {}", unique_name);
    }
    
    Err(format!("Could not generate unique filename after {} attempts", max_attempts))
}
