//! 服务器配置模块
//!
//! 提供 FTP/SFTP 服务器的配置管理和连接数统计功能

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::error::ConfigError;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(skip)]
    pub server: Arc<ServerConfig>,
    pub ftp: FtpConfig,
    pub sftp: SftpConfig,
    pub security: SecurityConfig,
    pub logging: LoggingConfig,
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Config {
            server: Arc::clone(&self.server),
            ftp: self.ftp.clone(),
            sftp: self.sftp.clone(),
            security: self.security.clone(),
            logging: self.logging.clone(),
        }
    }
}

/// 服务器配置
///
/// 包含全局连接数和每 IP 连接数统计，使用原子操作和无锁数据结构确保线程安全
#[derive(Debug, Serialize)]
pub struct ServerConfig {
    /// 全局连接计数器（原子操作）
    #[serde(skip)]
    pub global_connection_count: AtomicUsize,
    /// 每 IP 连接数统计（使用 parking_lot Mutex 保证线程安全）
    #[serde(skip)]
    pub connection_count_per_ip: parking_lot::Mutex<HashMap<String, usize>>,
}

// 手动实现 Deserialize，因为原子类型和 Mutex 不支持自动反序列化
impl<'de> Deserialize<'de> for ServerConfig {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // 反序列化时总是创建新的空实例
        Ok(ServerConfig::new())
    }
}

// 手动实现 Clone，避免不必要的锁拷贝
impl Clone for ServerConfig {
    fn clone(&self) -> Self {
        ServerConfig {
            global_connection_count: AtomicUsize::new(self.get_global_count()),
            connection_count_per_ip: parking_lot::Mutex::new(
                self.connection_count_per_ip.lock().clone(),
            ),
        }
    }
}

impl ServerConfig {
    /// 创建新的服务器配置
    pub fn new() -> Self {
        ServerConfig {
            global_connection_count: AtomicUsize::new(0),
            connection_count_per_ip: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// 增加全局连接数（原子操作）
    ///
    /// # Returns
    /// 返回增加前的连接数
    pub fn increment_global(&self) -> usize {
        self.global_connection_count.fetch_add(1, Ordering::SeqCst)
    }

    /// 减少全局连接数（原子操作）
    pub fn decrement_global(&self) {
        self.global_connection_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// 获取当前全局连接数
    pub fn get_global_count(&self) -> usize {
        self.global_connection_count.load(Ordering::SeqCst)
    }

    /// 增加指定 IP 的连接数
    ///
    /// # Arguments
    /// * `ip` - 客户端 IP 地址
    ///
    /// # Returns
    /// 返回增加后的连接数
    pub fn increment_ip(&self, ip: &str) -> usize {
        let mut map = self.connection_count_per_ip.lock();
        let count = map.entry(ip.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    /// 减少指定 IP 的连接数
    ///
    /// # Arguments
    /// * `ip` - 客户端 IP 地址
    pub fn decrement_ip(&self, ip: &str) {
        let mut map = self.connection_count_per_ip.lock();
        if let Some(count) = map.get_mut(ip) {
            if *count > 0 {
                *count -= 1;
            }
            if *count == 0 {
                map.remove(ip);
            }
        }
    }

    /// 获取指定 IP 的当前连接数
    pub fn get_ip_count(&self, ip: &str) -> usize {
        let map = self.connection_count_per_ip.lock();
        *map.get(ip).unwrap_or(&0)
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// FTP 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpConfig {
    pub enabled: bool,
    #[serde(default = "default_bind_ip")]
    pub bind_ip: String,
    #[serde(default = "default_ftp_port")]
    pub port: u16,
    pub welcome_message: String,
    #[serde(default = "default_encoding")]
    pub encoding: String,
    #[serde(default = "default_transfer_mode")]
    pub default_transfer_mode: String,
    #[serde(default = "default_passive_mode")]
    pub default_passive_mode: bool,
    pub allow_anonymous: bool,
    #[serde(default = "default_anonymous_home")]
    pub anonymous_home: Option<String>,
    pub passive_ports: (u16, u16),
    #[serde(default)]
    pub max_speed_kbps: u64,
    #[serde(default = "default_passive_ip_override")]
    pub passive_ip_override: Option<String>,
    #[serde(default = "default_masquerade_address")]
    pub masquerade_address: Option<String>,
    #[serde(default = "default_masquerade_map")]
    pub masquerade_map: HashMap<String, String>,
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u64,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: u64,
    #[serde(default)]
    pub hide_version_info: bool,
    #[serde(default)]
    pub ftps: FtpsConfig,
    #[serde(default = "default_upnp_enabled")]
    pub upnp_enabled: bool,
}

fn default_ftp_port() -> u16 {
    21
}

fn default_connection_timeout() -> u64 {
    300
}

fn default_idle_timeout() -> u64 {
    600
}

/// FTPS (FTP over SSL/TLS) 配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FtpsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub require_ssl: bool,
    #[serde(default)]
    pub implicit_ssl: bool,
    #[serde(default = "default_ftps_port")]
    pub implicit_ssl_port: u16,
    #[serde(default)]
    pub cert_path: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
}

fn default_ftps_port() -> u16 {
    990
}

fn default_bind_ip() -> String {
    "0.0.0.0".to_string()
}

fn default_encoding() -> String {
    "UTF-8".to_string()
}

fn default_transfer_mode() -> String {
    "binary".to_string()
}

fn default_passive_mode() -> bool {
    true
}

fn default_anonymous_home() -> Option<String> {
    Some("".to_string())
}

fn default_passive_ip_override() -> Option<String> {
    Some("".to_string())
}

fn default_masquerade_address() -> Option<String> {
    Some("".to_string())
}

fn default_masquerade_map() -> HashMap<String, String> {
    HashMap::new()
}

fn default_upnp_enabled() -> bool {
    false
}

/// SFTP 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpConfig {
    pub enabled: bool,
    #[serde(default = "default_bind_ip")]
    pub bind_ip: String,
    #[serde(default = "default_sftp_port")]
    pub port: u16,
    pub host_key_path: String,
    pub max_auth_attempts: u32,
    pub auth_timeout: u64,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_max_sessions_per_user")]
    pub max_sessions_per_user: u32,
    #[serde(default = "default_key_rotation_days")]
    pub host_key_rotation_days: u32,
}

fn default_sftp_port() -> u16 {
    2222
}

fn default_max_sessions_per_user() -> u32 {
    5
}

fn default_key_rotation_days() -> u32 {
    0
}

fn default_log_level() -> String {
    "info".to_string()
}

/// 安全配置
///
/// 包含 IP 白名单/黑名单、连接数限制、Fail2Ban 等
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_max_connections_per_ip")]
    pub max_connections_per_ip: usize,
    pub allowed_ips: Vec<String>,
    pub denied_ips: Vec<String>,
    #[serde(default = "default_fail2ban_enabled")]
    pub fail2ban_enabled: bool,
    #[serde(default = "default_fail2ban_threshold")]
    pub fail2ban_threshold: u32,
    #[serde(default = "default_fail2ban_ban_time")]
    pub fail2ban_ban_time: u64,
    // 符号链接安全配置
    #[serde(default = "default_allow_symlinks")]
    pub allow_symlinks: bool,
    #[serde(default = "default_max_login_attempts")]
    pub max_login_attempts: u32,
}

fn default_max_login_attempts() -> u32 {
    5
}

fn default_fail2ban_ban_time() -> u64 {
    3600
}

fn default_max_connections() -> usize {
    100
}

fn default_max_connections_per_ip() -> usize {
    10
}

fn default_fail2ban_enabled() -> bool {
    false
}

fn default_fail2ban_threshold() -> u32 {
    5
}

fn default_allow_symlinks() -> bool {
    false // 默认禁用符号链接以提高安全性
}

/// 日志配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub log_dir: String,
    pub log_level: String,
    pub max_log_files: usize,
}

pub fn get_program_data_path() -> PathBuf {
    let program_data = env::var("PROGRAMDATA").unwrap_or("C:\\ProgramData".to_string());
    PathBuf::from(&program_data).join("wftpg")
}

fn get_default_paths() -> (String, String) {
    let base_path = get_program_data_path();

    let log_dir = base_path.join("logs").to_string_lossy().to_string();
    let host_key_path = base_path
        .join("ssh\\ssh_host_rsa_key")
        .to_string_lossy()
        .to_string();

    (log_dir, host_key_path)
}

impl Default for Config {
    fn default() -> Self {
        let (log_dir, host_key_path) = get_default_paths();
        let base_path = get_program_data_path();
        let cert_path = base_path
            .join("certs\\server.crt")
            .to_string_lossy()
            .to_string();
        let key_path = base_path
            .join("certs\\server.key")
            .to_string_lossy()
            .to_string();

        Config {
            server: Arc::new(ServerConfig::new()),
            ftp: FtpConfig {
                enabled: true,
                bind_ip: "0.0.0.0".to_string(),
                port: 21,
                passive_ports: (50000, 50100),
                welcome_message: "Welcome to WFTPG FTP Server".to_string(),
                allow_anonymous: false,
                anonymous_home: Some("".to_string()),
                max_speed_kbps: 0,
                encoding: "UTF-8".to_string(),
                default_transfer_mode: "binary".to_string(),
                default_passive_mode: true,
                ftps: FtpsConfig {
                    enabled: false,
                    require_ssl: false,
                    cert_path: Some(cert_path),
                    key_path: Some(key_path),
                    implicit_ssl: false,
                    implicit_ssl_port: 990,
                },
                passive_ip_override: Some("".to_string()),
                masquerade_address: Some("".to_string()),
                masquerade_map: HashMap::new(),
                connection_timeout: 300,
                idle_timeout: 600,
                hide_version_info: false,
                upnp_enabled: false,
            },
            sftp: SftpConfig {
                enabled: true,
                bind_ip: "0.0.0.0".to_string(),
                port: 2222,
                host_key_path,
                max_auth_attempts: 3,
                auth_timeout: 60,
                log_level: "info".to_string(),
                max_sessions_per_user: 5,
                host_key_rotation_days: 0,
            },
            security: SecurityConfig {
                allowed_ips: vec!["0.0.0.0/0".to_string()],
                denied_ips: vec![],
                max_connections: 100,
                max_connections_per_ip: 10,
                fail2ban_enabled: false,
                fail2ban_threshold: 5,
                fail2ban_ban_time: 3600,
                allow_symlinks: false,
                max_login_attempts: 5,
            },
            logging: LoggingConfig {
                log_dir,
                log_level: "info".to_string(),
                max_log_files: 10,
            },
        }
    }
}

impl Config {
    /// 从文件加载配置
    ///
    /// 如果文件不存在，则创建默认配置并保存
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            let config = Self::default();
            if let Err(e) = config.save(path) {
                tracing::warn!("Failed to save default config: {}", e);
            }
            return Ok(config);
        }

        let content = fs::read_to_string(path).map_err(ConfigError::ReadFailed)?;

        let mut config: Config = toml::from_str(&content).map_err(ConfigError::ParseFailed)?;
        config.server = Arc::new(ServerConfig::new());

        config.ftp.bind_ip = Self::normalize_bind_ip(&config.ftp.bind_ip);
        config.sftp.bind_ip = Self::normalize_bind_ip(&config.sftp.bind_ip);

        Ok(config)
    }

    /// 验证配置路径的有效性
    ///
    /// # Returns
    /// 返回警告信息列表，如果为空则表示所有路径均有效
    pub fn validate_paths(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.ftp.allow_anonymous {
            match &self.ftp.anonymous_home {
                None => {
                    warnings.push(
                        "Anonymous user enabled but no home directory configured".to_string(),
                    );
                }
                Some(anon_home) => {
                    if let Err(e) =
                        Self::validate_home_path(anon_home, "FTP anonymous home directory")
                    {
                        warnings.push(e);
                    }
                }
            }
        }

        if self.ftp.ftps.enabled {
            if let Some(cert_path) = &self.ftp.ftps.cert_path {
                if cert_path.is_empty() {
                    warnings.push("FTPS enabled but certificate path is empty".to_string());
                }
            } else {
                warnings.push("FTPS enabled but certificate path not configured".to_string());
            }

            if let Some(key_path) = &self.ftp.ftps.key_path {
                if key_path.is_empty() {
                    warnings.push("FTPS enabled but private key path is empty".to_string());
                }
            } else {
                warnings.push("FTPS enabled but private key path not configured".to_string());
            }
        }

        {
            let log_dir = &self.logging.log_dir;
            let log_path = Path::new(log_dir);
            if !log_path.exists() {
                warnings.push(format!("Log directory does not exist: {}", log_dir));
            } else {
                match fs::metadata(log_path) {
                    Ok(m) => {
                        if m.permissions().readonly() {
                            warnings.push(format!("Log directory is not writable: {}", log_dir));
                        }
                    }
                    Err(e) => {
                        warnings.push(format!("Cannot access log directory '{}': {}", log_dir, e));
                    }
                }
            }
        }

        warnings
    }

    fn validate_home_path(path: &str, name: &str) -> Result<(), String> {
        let p = Path::new(path);
        if !p.exists() {
            return Err(format!("{} does not exist: {}", name, path));
        }
        if !p.is_dir() {
            return Err(format!("{} is not a directory: {}", name, path));
        }
        if p.canonicalize().is_err() {
            return Err(format!("{} path cannot be canonicalized: {}", name, path));
        }
        Ok(())
    }

    /// 保存配置到文件
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ConfigError::WriteFailed)?;
        }

        let content = toml::to_string_pretty(self).map_err(ConfigError::SerializeFailed)?;

        fs::write(path, content).map_err(ConfigError::WriteFailed)?;

        Ok(())
    }

    pub fn get_config_path() -> PathBuf {
        get_program_data_path().join("config.toml")
    }

    pub fn get_users_path() -> PathBuf {
        get_program_data_path().join("users.json")
    }

    fn normalize_bind_ip(ip: &str) -> String {
        let trimmed = ip.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            return trimmed.to_string();
        }
        if trimmed.contains(':') && trimmed.matches(':').count() > 1 {
            format!("[{}]", trimmed)
        } else {
            trimmed.to_string()
        }
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.ftp.enabled && self.ftp.port == 0 {
            return Err("FTP port cannot be 0".to_string());
        }

        if self.sftp.enabled && self.sftp.port == 0 {
            return Err("SFTP port cannot be 0".to_string());
        }

        if self.ftp.passive_ports.0 > self.ftp.passive_ports.1 {
            return Err(format!(
                "Invalid passive port range: {} > {}",
                self.ftp.passive_ports.0, self.ftp.passive_ports.1
            ));
        }

        if self.security.max_connections == 0 {
            return Err("max_connections must be greater than 0".to_string());
        }

        if self.security.max_connections_per_ip == 0 {
            return Err("max_connections_per_ip must be greater than 0".to_string());
        }

        if self.security.fail2ban_enabled && self.security.fail2ban_threshold == 0 {
            return Err("fail2ban_threshold must be greater than 0 when enabled".to_string());
        }

        if self.security.fail2ban_enabled && self.security.fail2ban_ban_time == 0 {
            return Err("fail2ban_ban_time must be greater than 0 when enabled".to_string());
        }

        if self.security.max_login_attempts == 0 {
            return Err("max_login_attempts must be greater than 0".to_string());
        }

        if self.logging.max_log_files == 0 {
            return Err("max_log_files must be greater than 0".to_string());
        }

        Ok(())
    }

    /// 检查 IP 地址是否在允许列表中
    pub fn is_ip_allowed(&self, ip: &str) -> bool {
        if self
            .security
            .denied_ips
            .iter()
            .any(|cidr| ip_matches_cidr(ip, cidr).unwrap_or(false))
        {
            return false;
        }

        if self.security.allowed_ips.is_empty() {
            return true;
        }

        self.security
            .allowed_ips
            .iter()
            .any(|cidr| ip_matches_cidr(ip, cidr).unwrap_or(false))
    }

    /// 检查连接数限制
    ///
    /// # Arguments
    /// * `client_ip` - 客户端 IP 地址
    ///
    /// # Returns
    /// 如果未超过限制返回 true，否则返回 false
    pub fn check_connection_limits(&self, client_ip: &str) -> bool {
        let global_count = self.server.get_global_count();
        let ip_count = self.server.get_ip_count(client_ip);

        if global_count >= self.security.max_connections {
            tracing::warn!(
                "Connection limit reached: {} global connections (max: {})",
                global_count,
                self.security.max_connections
            );
            return false;
        }

        if ip_count >= self.security.max_connections_per_ip {
            tracing::warn!(
                "Per-IP connection limit reached for {}: {} connections (max: {})",
                client_ip,
                ip_count,
                self.security.max_connections_per_ip
            );
            return false;
        }

        true
    }

    /// 注册新连接（增加计数器）
    ///
    /// # Arguments
    /// * `client_ip` - 客户端 IP 地址
    pub fn register_connection(&self, client_ip: &str) {
        self.server.increment_global();
        self.server.increment_ip(client_ip);
        tracing::debug!(
            "Connection registered: {} (global: {}, per-IP: {})",
            client_ip,
            self.server.get_global_count(),
            self.server.get_ip_count(client_ip)
        );
    }

    /// 注销连接（减少计数器）
    ///
    /// # Arguments
    /// * `client_ip` - 客户端 IP 地址
    pub fn unregister_connection(&self, client_ip: &str) {
        self.server.decrement_global();
        self.server.decrement_ip(client_ip);
        tracing::debug!(
            "Connection unregistered: {} (global: {}, per-IP: {})",
            client_ip,
            self.server.get_global_count(),
            self.server.get_ip_count(client_ip)
        );
    }
}

fn ip_matches_cidr(ip: &str, cidr: &str) -> Result<bool> {
    use ipnet::{Ipv4Net, Ipv6Net};
    use std::net::{Ipv4Addr, Ipv6Addr};

    if cidr == "0.0.0.0/0" || cidr == "::/0" {
        return Ok(true);
    }

    if let Ok(ipv4) = ip.parse::<Ipv4Addr>()
        && let Ok(net) = cidr.parse::<Ipv4Net>()
    {
        return Ok(net.contains(&ipv4));
    }

    if let Ok(ipv6) = ip.parse::<Ipv6Addr>()
        && let Ok(net) = cidr.parse::<Ipv6Net>()
    {
        return Ok(net.contains(&ipv6));
    }

    Ok(ip == cidr)
}
