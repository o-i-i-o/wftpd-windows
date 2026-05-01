//! Configuration type definitions

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(skip)]
    pub server: std::sync::Arc<ServerConfig>,
    pub ftp: FtpConfig,
    pub sftp: SftpConfig,
    pub security: SecurityConfig,
    pub logging: LoggingConfig,
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Config {
            server: std::sync::Arc::clone(&self.server),
            ftp: self.ftp.clone(),
            sftp: self.sftp.clone(),
            security: self.security.clone(),
            logging: self.logging.clone(),
        }
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
        let old_global = self.global_connection_count.fetch_sub(1, Ordering::SeqCst);
        if old_global == 0 {
            self.global_connection_count.fetch_add(1, Ordering::SeqCst);
            tracing::warn!("Connection count underflow prevented during unregister");
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
    #[serde(default = "default_masquerade_address")]
    pub masquerade_address: Option<String>,
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
    #[serde(default = "default_pooled_listener_mode")]
    pub pooled_listener_mode: bool,
    #[serde(default = "default_allow_nat_clients")]
    pub allow_nat_clients: bool,
    #[serde(default = "default_ftp_root")]
    pub ftp_root: Option<String>,
}

pub fn default_ftp_port() -> u16 {
    21
}

pub fn default_connection_timeout() -> u64 {
    15
}

pub fn default_idle_timeout() -> u64 {
    15
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FtpsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub require_ssl: bool,
    #[serde(default)]
    pub cert_path: Option<String>,
    #[serde(default)]
    pub key_path: Option<String>,
}

pub fn default_bind_ip() -> String {
    "0.0.0.0".to_string()
}

pub fn default_encoding() -> String {
    "UTF-8".to_string()
}

pub fn default_transfer_mode() -> String {
    "binary".to_string()
}

pub fn default_passive_mode() -> bool {
    true
}

pub fn default_anonymous_home() -> Option<String> {
    Some("".to_string())
}

pub fn default_masquerade_address() -> Option<String> {
    Some("".to_string())
}

pub fn default_upnp_enabled() -> bool {
    false
}

pub fn default_pooled_listener_mode() -> bool {
    true
}

pub fn default_allow_nat_clients() -> bool {
    true
}

pub fn default_ftp_root() -> Option<String> {
    None
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

pub fn default_sftp_port() -> u16 {
    2222
}

pub fn default_max_sessions_per_user() -> u32 {
    5
}

pub fn default_key_rotation_days() -> u32 {
    0
}

pub fn default_log_level() -> String {
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
    #[serde(default = "default_fail2ban_enabled")]
    pub fail2ban_enabled: bool,
    #[serde(default = "default_fail2ban_threshold")]
    pub fail2ban_threshold: u32,
    #[serde(default = "default_fail2ban_ban_time")]
    pub fail2ban_ban_time: u64,
    #[serde(default = "default_allow_symlinks")]
    pub allow_symlinks: bool,
    #[serde(default = "default_max_login_attempts")]
    pub max_login_attempts: u32,
}

pub fn default_allow_symlinks() -> bool {
    false
}

pub fn default_max_login_attempts() -> u32 {
    5
}

pub fn default_fail2ban_enabled() -> bool {
    false
}

pub fn default_fail2ban_threshold() -> u32 {
    5
}

pub fn default_fail2ban_ban_time() -> u64 {
    3600
}

pub fn default_max_connections() -> usize {
    100
}

pub fn default_max_connections_per_ip() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub log_dir: String,
    pub log_level: String,
    pub max_log_files: usize,
}
