//! 配置管理器
//! IP 白名单/黑名单配置
//! 负责加载、验证和管理服务器配置，支持热重载


use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub ftp: FtpConfig,
    pub sftp: SftpConfig,
    pub security: SecurityConfig,
    pub logging: LoggingConfig,
}

impl Clone for Config {
    fn clone(&self) -> Self {
        let global_count = self.server.get_global_count();
        let ip_counts = self.server.get_all_ip_counts();

        let new_config = Config {
            server: ServerConfig::new(),
            ftp: self.ftp.clone(),
            sftp: self.sftp.clone(),
            security: self.security.clone(),
            logging: self.logging.clone(),
        };

        // 恢复连接计数（避免 reload 时丢失活跃连接统计）
        new_config.server.restore_counts(global_count, &ip_counts);
        new_config
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(skip)]
    pub global_connection_count: AtomicUsize,
    #[serde(skip)]
    pub connection_count_per_ip: parking_lot::Mutex<std::collections::HashMap<String, usize>>,
}

impl ServerConfig {
    pub fn new() -> Self {
        ServerConfig {
            global_connection_count: AtomicUsize::new(0),
            connection_count_per_ip: parking_lot::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn decrement_global(&self) {
        self.global_connection_count.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn get_global_count(&self) -> usize {
        self.global_connection_count.load(Ordering::SeqCst)
    }

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

    pub fn get_ip_count(&self, ip: &str) -> usize {
        let map = self.connection_count_per_ip.lock();
        *map.get(ip).unwrap_or(&0)
    }

    pub fn get_all_ip_counts(&self) -> std::collections::HashMap<String, usize> {
        let map = self.connection_count_per_ip.lock();
        map.clone()
    }

    pub fn restore_counts(
        &self,
        global_count: usize,
        ip_counts: &std::collections::HashMap<String, usize>,
    ) {
        self.global_connection_count
            .store(global_count, Ordering::SeqCst);
        let mut map = self.connection_count_per_ip.lock();
        map.clear();
        for (ip, count) in ip_counts {
            if *count > 0 {
                map.insert(ip.clone(), *count);
            }
        }
    }

    pub fn try_register(&self, client_ip: &str, max_global: usize, max_per_ip: usize) -> bool {
        let mut map = self.connection_count_per_ip.lock();
        
        let global_count = self.global_connection_count.load(Ordering::SeqCst);
        if global_count >= max_global {
            return false;
        }

        let ip_count = *map.get(client_ip).unwrap_or(&0);
        if ip_count >= max_per_ip {
            return false;
        }

        self.global_connection_count.fetch_add(1, Ordering::SeqCst);
        *map.entry(client_ip.to_string()).or_insert(0) += 1;
        true
    }

    pub fn unregister(&self, client_ip: &str) {
        let global = self.global_connection_count.fetch_sub(1, Ordering::SeqCst);
        if global == 0 {
            self.global_connection_count.fetch_add(1, Ordering::SeqCst);
        }
        
        let mut map = self.connection_count_per_ip.lock();
        if let Some(count) = map.get_mut(client_ip) {
            if *count > 0 {
                *count -= 1;
            }
            if *count == 0 {
                map.remove(client_ip);
            }
        }
    }

    pub fn get_counts(&self, client_ip: &str) -> (usize, usize) {
        let map = self.connection_count_per_ip.lock();
        let global = self.global_connection_count.load(Ordering::SeqCst);
        let per_ip = *map.get(client_ip).unwrap_or(&0);
        (global, per_ip)
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self::new()
    }
}

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
    false  // 默认禁用，需要时手动启用
}

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
    0  // 默认不自动轮换，设置为 0 表示禁用
}

fn default_log_level() -> String {
    "info".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_max_connections_per_ip")]
    pub max_connections_per_ip: usize,
    pub allowed_ips: Vec<String>,
    pub denied_ips: Vec<String>,
    // Fail2Ban 集成配置
    #[serde(default = "default_fail2ban_enabled")]
    pub fail2ban_enabled: bool,
    #[serde(default = "default_fail2ban_threshold")]
    pub fail2ban_threshold: u32,
    #[serde(default = "default_fail2ban_ban_time")]
    pub fail2ban_ban_time: u64,
    // 符号链接安全配置
    #[serde(default = "default_allow_symlinks")]
    pub allow_symlinks: bool,
}

fn default_allow_symlinks() -> bool {
    false  // 默认禁用符号链接以提高安全性
}

fn default_fail2ban_enabled() -> bool {
    false  // 默认禁用，需要时手动启用
}

fn default_fail2ban_threshold() -> u32 {
    5  // 5 次失败后封禁
}

fn default_fail2ban_ban_time() -> u64 {
    3600  // 默认封禁 1 小时
}

fn default_max_connections() -> usize {
    100
}

fn default_max_connections_per_ip() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub log_dir: String,
    pub log_level: String,
    pub max_log_size: u64,
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
            server: ServerConfig::new(),
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
            },
            logging: LoggingConfig {
                log_dir,
                log_level: "info".to_string(),
                max_log_size: 10 * 1024 * 1024,
                max_log_files: 10,
            },
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            let config = Self::default();
            if let Err(e) = config.save(path) {
                tracing::warn!("Failed to save default config: {}", e);
            }
            return Ok(config);
        }

        let content = fs::read_to_string(path).context("Failed to read config file")?;

        let config: Config = toml::from_str(&content).context("Failed to parse config file")?;

        Ok(config)
    }

    pub fn validate_paths(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.ftp.allow_anonymous {
            match &self.ftp.anonymous_home {
                None => {
                    warnings.push("匿名用户已启用，但未配置匿名用户主目录".to_string());
                }
                Some(anon_home) => {
                    if let Err(e) = Self::validate_home_path(anon_home, "FTP匿名用户主目录")
                    {
                        warnings.push(e);
                    }
                }
            }
        }

        if self.ftp.ftps.enabled {
            // 证书会自动生成，不需要检查是否存在
            if let Some(cert_path) = &self.ftp.ftps.cert_path {
                if cert_path.is_empty() {
                    warnings.push("FTPS 已启用，但未配置证书路径".to_string());
                }
            } else {
                warnings.push("FTPS 已启用，但未配置证书路径".to_string());
            }

            if let Some(key_path) = &self.ftp.ftps.key_path {
                if key_path.is_empty() {
                    warnings.push("FTPS 已启用，但未配置私钥路径".to_string());
                }
            } else {
                warnings.push("FTPS 已启用，但未配置私钥路径".to_string());
            }
        }

        {
            let log_dir = &self.logging.log_dir;
            let log_path = Path::new(log_dir);
            if !log_path.exists() {
                warnings.push(format!("日志目录不存在: {}", log_dir));
            } else {
                match fs::metadata(log_path) {
                    Ok(m) => {
                        if m.permissions().readonly() {
                            warnings.push(format!("日志目录不可写: {}", log_dir));
                        }
                    }
                    Err(e) => {
                        warnings.push(format!("无法访问日志目录 '{}': {}", log_dir, e));
                    }
                }
            }
        }

        warnings
    }

    fn validate_home_path(path: &str, name: &str) -> Result<(), String> {
        let p = Path::new(path);
        if !p.exists() {
            return Err(format!("{}不存在: {}", name, path));
        }
        if !p.is_dir() {
            return Err(format!("{}不是目录: {}", name, path));
        }
        if p.canonicalize().is_err() {
            return Err(format!("{}路径无法规范化: {}", name, path));
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;

        fs::write(path, content).context("Failed to write config file")?;

        Ok(())
    }

    pub fn get_config_path() -> PathBuf {
        get_program_data_path().join("config.toml")
    }

    pub fn get_users_path() -> PathBuf {
        get_program_data_path().join("users.json")
    }

    pub fn get_default_log_dir() -> String {
        get_program_data_path().join("logs").to_string_lossy().to_string()
    }

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

    pub fn check_connection_limits(&self, client_ip: &str) -> bool {
        let (global_count, ip_count) = self.server.get_counts(client_ip);

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

    pub fn try_register_connection(&self, client_ip: &str) -> bool {
        let success = self.server.try_register(
            client_ip,
            self.security.max_connections,
            self.security.max_connections_per_ip,
        );

        if !success {
            let (global_count, ip_count) = self.server.get_counts(client_ip);
            if global_count >= self.security.max_connections {
                tracing::warn!(
                    "Connection limit reached: {} global connections (max: {}) - rejected {}",
                    global_count,
                    self.security.max_connections,
                    client_ip
                );
            } else {
                tracing::warn!(
                    "Per-IP connection limit reached for {}: {} connections (max: {}) - rejected",
                    client_ip,
                    ip_count,
                    self.security.max_connections_per_ip
                );
            }
        } else {
            let (global_count, ip_count) = self.server.get_counts(client_ip);
            tracing::debug!(
                "Connection registered: {} (global: {}, per-IP: {})",
                client_ip,
                global_count,
                ip_count
            );
        }

        success
    }

    pub fn unregister_connection(&self, client_ip: &str) {
        self.server.unregister(client_ip);
        let (global_count, ip_count) = self.server.get_counts(client_ip);
        tracing::debug!(
            "Connection unregistered: {} (global: {}, per-IP: {})",
            client_ip,
            global_count,
            ip_count
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
