//! Configuration manager
//! IP whitelist/blacklist configuration
//! Responsible for loading, validating and managing server configuration, supports hot reload

use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[path = "config_types.rs"]
mod config_types;
pub use config_types::{
    default_bind_ip, default_connection_timeout, default_encoding, default_fail2ban_ban_time,
    default_fail2ban_enabled, default_fail2ban_threshold, default_idle_timeout,
    default_key_rotation_days, default_log_level, default_log_level as default_sftp_log_level,
    default_max_connections, default_max_connections_per_ip, default_max_login_attempts,
    default_max_sessions_per_user, default_passive_ip_override, default_passive_mode,
    default_pooled_listener_mode, default_sftp_port, default_transfer_mode, default_upnp_enabled,
    Config, FtpConfig, FtpsConfig, LoggingConfig, SecurityConfig, ServerConfig, SftpConfig,
};

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
            server: std::sync::Arc::new(ServerConfig::new()),
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
                },
                passive_ip_override: Some("".to_string()),
                masquerade_address: Some("".to_string()),
                masquerade_map: std::collections::HashMap::new(),
                connection_timeout: 300,
                idle_timeout: 600,
                hide_version_info: false,
                upnp_enabled: false,
                pooled_listener_mode: true,
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

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            let config = Self::default();
            if let Err(e) = config.save(path) {
                tracing::warn!("Failed to save default config: {}", e);
            }
            return Ok(config);
        }

        let content = fs::read_to_string(path).context("Failed to read config file")?;

        let mut config: Config = toml::from_str(&content).context("Failed to parse config file")?;
        config.server = std::sync::Arc::new(ServerConfig::new());

        config.ftp.bind_ip = Self::normalize_bind_ip(&config.ftp.bind_ip);
        config.sftp.bind_ip = Self::normalize_bind_ip(&config.sftp.bind_ip);

        Ok(config)
    }

    pub fn validate_paths(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.ftp.allow_anonymous {
            match &self.ftp.anonymous_home {
                None => {
                    warnings.push(
                        "Anonymous user is enabled but anonymous home directory is not configured"
                            .to_string(),
                    );
                }
                Some(anon_home) => {
                    if let Err(e) =
                        Self::validate_home_path(anon_home, "FTP anonymous user home directory")
                    {
                        warnings.push(e);
                    }
                }
            }
        }

        if self.ftp.ftps.enabled {
            if let Some(cert_path) = &self.ftp.ftps.cert_path {
                if cert_path.is_empty() {
                    warnings.push("FTPS enabled but certificate path not configured".to_string());
                }
            } else {
                warnings.push("FTPS enabled but certificate path not configured".to_string());
            }

            if let Some(key_path) = &self.ftp.ftps.key_path {
                if key_path.is_empty() {
                    warnings.push("FTPS enabled but private key path not configured".to_string());
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
        get_program_data_path()
            .join("logs")
            .to_string_lossy()
            .to_string()
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

    pub fn validate(&self) -> Result<(), String> {
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

        if self.logging.max_log_files == 0 {
            return Err("max_log_files must be greater than 0".to_string());
        }

        for cidr in &self.security.allowed_ips {
            if !cidr.is_empty() && ip_matches_cidr("127.0.0.1", cidr).is_err() {
                return Err(format!("Invalid allowed IP/CIDR format: {}", cidr));
            }
        }

        for cidr in &self.security.denied_ips {
            if !cidr.is_empty() && ip_matches_cidr("127.0.0.1", cidr).is_err() {
                return Err(format!("Invalid denied IP/CIDR format: {}", cidr));
            }
        }

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_connection_counting() {
        let config = ServerConfig::new();

        assert_eq!(config.get_global_count(), 0);
        assert_eq!(config.get_ip_count("192.168.1.1"), 0);

        assert!(config.try_register("192.168.1.1", 10, 5));
        assert_eq!(config.get_global_count(), 1);
        assert_eq!(config.get_ip_count("192.168.1.1"), 1);

        assert!(config.try_register("192.168.1.1", 10, 5));
        assert_eq!(config.get_ip_count("192.168.1.1"), 2);

        assert!(config.try_register("192.168.1.2", 10, 5));
        assert_eq!(config.get_global_count(), 3);

        config.unregister("192.168.1.1");
        assert_eq!(config.get_ip_count("192.168.1.1"), 1);
        assert_eq!(config.get_global_count(), 2);

        config.unregister("192.168.1.1");
        assert_eq!(config.get_ip_count("192.168.1.1"), 0);

        config.decrement_ip("192.168.1.2");
        assert_eq!(config.get_ip_count("192.168.1.2"), 0);
    }

    #[test]
    fn test_server_config_limits() {
        let config = ServerConfig::new();

        assert!(config.try_register("192.168.1.1", 2, 1));
        assert!(!config.try_register("192.168.1.1", 2, 1));

        assert!(config.try_register("192.168.1.2", 2, 5));
        assert!(!config.try_register("192.168.1.3", 2, 5));
    }

    #[test]
    fn test_server_config_unregister_underflow() {
        let config = ServerConfig::new();
        config.unregister("192.168.1.1");
        assert_eq!(config.get_global_count(), 0);
    }

    #[test]
    fn test_server_config_get_counts() {
        let config = ServerConfig::new();
        config.try_register("192.168.1.1", 100, 10);
        config.try_register("192.168.1.1", 100, 10);

        let (global, per_ip) = config.get_counts("192.168.1.1");
        assert_eq!(global, 2);
        assert_eq!(per_ip, 2);
    }

    #[test]
    fn test_server_config_get_all_ip_counts() {
        let config = ServerConfig::new();
        config.try_register("192.168.1.1", 100, 10);
        config.try_register("192.168.1.2", 100, 10);

        let counts = config.get_all_ip_counts();
        assert_eq!(counts.len(), 2);
        assert_eq!(counts.get("192.168.1.1"), Some(&1));
        assert_eq!(counts.get("192.168.1.2"), Some(&1));
    }

    #[test]
    fn test_config_default_values() {
        let config = Config::default();

        assert!(config.ftp.enabled);
        assert_eq!(config.ftp.port, 21);
        assert_eq!(config.ftp.bind_ip, "0.0.0.0");
        assert_eq!(config.ftp.passive_ports.0, 50000);
        assert_eq!(config.ftp.passive_ports.1, 50100);
        assert_eq!(config.ftp.connection_timeout, 300);
        assert_eq!(config.ftp.idle_timeout, 600);
        assert!(!config.ftp.allow_anonymous);

        assert!(config.sftp.enabled);
        assert_eq!(config.sftp.port, 2222);
        assert_eq!(config.sftp.bind_ip, "0.0.0.0");
        assert_eq!(config.sftp.max_auth_attempts, 3);
        assert_eq!(config.sftp.auth_timeout, 60);

        assert_eq!(config.security.max_connections, 100);
        assert_eq!(config.security.max_connections_per_ip, 10);
        assert!(!config.security.fail2ban_enabled);
        assert_eq!(config.security.max_login_attempts, 5);
        assert!(!config.security.allow_symlinks);

        assert_eq!(config.logging.max_log_files, 10);
    }

    #[test]
    fn test_config_normalize_bind_ip() {
        assert_eq!(Config::normalize_bind_ip("0.0.0.0"), "0.0.0.0");
        assert_eq!(Config::normalize_bind_ip("[::]"), "[::]");
        assert_eq!(Config::normalize_bind_ip("::1"), "[::1]");
        assert_eq!(Config::normalize_bind_ip("2001:db8::1"), "[2001:db8::1]");
        assert_eq!(Config::normalize_bind_ip("192.168.1.1"), "192.168.1.1");
        assert_eq!(Config::normalize_bind_ip("  0.0.0.0  "), "0.0.0.0");
    }

    #[test]
    fn test_config_validate_pass() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validate_ftp_port_zero() {
        let mut config = Config::default();
        config.ftp.port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_sftp_port_zero() {
        let mut config = Config::default();
        config.sftp.port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_invalid_passive_range() {
        let mut config = Config::default();
        config.ftp.passive_ports = (50100, 50000);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_zero_max_connections() {
        let mut config = Config::default();
        config.security.max_connections = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_zero_max_per_ip() {
        let mut config = Config::default();
        config.security.max_connections_per_ip = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_fail2ban_threshold_zero() {
        let mut config = Config::default();
        config.security.fail2ban_enabled = true;
        config.security.fail2ban_threshold = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_fail2ban_ban_time_zero() {
        let mut config = Config::default();
        config.security.fail2ban_enabled = true;
        config.security.fail2ban_ban_time = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_zero_log_files() {
        let mut config = Config::default();
        config.logging.max_log_files = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_empty_cidr_skipped() {
        let mut config = Config::default();
        config.security.allowed_ips = vec!["".to_string()];
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_is_ip_allowed() {
        let mut config = Config::default();
        config.security.allowed_ips = vec!["192.168.1.0/24".to_string()];
        config.security.denied_ips = vec![];

        assert!(config.is_ip_allowed("192.168.1.100"));
        assert!(!config.is_ip_allowed("10.0.0.1"));
    }

    #[test]
    fn test_config_is_ip_allowed_denied() {
        let mut config = Config::default();
        config.security.allowed_ips = vec!["0.0.0.0/0".to_string()];
        config.security.denied_ips = vec!["10.0.0.0/8".to_string()];

        assert!(!config.is_ip_allowed("10.0.0.1"));
        assert!(config.is_ip_allowed("192.168.1.1"));
    }

    #[test]
    fn test_config_check_connection_limits() {
        let config = Config::default();
        assert!(config.check_connection_limits("192.168.1.1"));
    }

    #[test]
    fn test_ip_matches_cidr_exact() {
        assert!(ip_matches_cidr("192.168.1.1", "192.168.1.1").unwrap());
        assert!(!ip_matches_cidr("192.168.1.1", "192.168.1.2").unwrap());
    }

    #[test]
    fn test_ip_matches_cidr_ipv4_net() {
        assert!(ip_matches_cidr("192.168.1.1", "192.168.1.0/24").unwrap());
        assert!(!ip_matches_cidr("192.168.2.1", "192.168.1.0/24").unwrap());
    }

    #[test]
    fn test_ip_matches_cidr_wildcard() {
        assert!(ip_matches_cidr("192.168.1.1", "0.0.0.0/0").unwrap());
        assert!(ip_matches_cidr("::1", "::/0").unwrap());
    }

    #[test]
    fn test_ip_matches_cidr_ipv6() {
        assert!(ip_matches_cidr("2001:db8::1", "2001:db8::/32").unwrap());
        assert!(!ip_matches_cidr("2001:db9::1", "2001:db8::/32").unwrap());
    }

    #[test]
    fn test_config_clone() {
        let config = Config::default();
        let cloned = config.clone();
        assert_eq!(config.ftp.port, cloned.ftp.port);
        assert_eq!(config.sftp.port, cloned.sftp.port);
    }

    #[test]
    fn test_ftps_config_default() {
        let ftps = FtpsConfig::default();
        assert!(!ftps.enabled);
        assert!(!ftps.require_ssl);
    }

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.get_global_count(), 0);
    }

    #[test]
    fn test_ip_matches_cidr_invalid_ip() {
        assert!(!ip_matches_cidr("not-an-ip", "192.168.1.0/24").unwrap());
    }

    #[test]
    fn test_ip_matches_cidr_invalid_cidr() {
        assert!(!ip_matches_cidr("192.168.1.1", "invalid").unwrap());
    }

    #[test]
    fn test_ip_matches_cidr_empty() {
        assert!(!ip_matches_cidr("", "192.168.1.0/24").unwrap());
        assert!(!ip_matches_cidr("192.168.1.1", "").unwrap());
    }

    #[test]
    fn test_config_connection_counting() {
        let config = Config::default();

        assert!(config.try_register_connection("192.168.1.1"));
        assert!(config.try_register_connection("192.168.1.1"));

        config.unregister_connection("192.168.1.1");
        config.unregister_connection("192.168.1.1");
        assert_eq!(config.server.get_global_count(), 0);
    }

    #[test]
    fn test_config_connection_per_ip_limit() {
        let mut config = Config::default();
        config.security.max_connections_per_ip = 1;

        assert!(config.try_register_connection("192.168.1.1"));
        assert!(!config.try_register_connection("192.168.1.1"));
        assert!(config.try_register_connection("192.168.1.2"));
    }

    #[test]
    fn test_config_is_ip_allowed_both_empty() {
        let config = Config::default();
        assert!(config.is_ip_allowed("192.168.1.1"));
        assert!(config.is_ip_allowed("10.0.0.1"));
    }

    #[test]
    fn test_config_is_ip_allowed_only_denied() {
        let mut config = Config::default();
        config.security.denied_ips = vec!["10.0.0.0/8".to_string()];
        assert!(!config.is_ip_allowed("10.0.0.1"));
        assert!(config.is_ip_allowed("192.168.1.1"));
    }

    #[test]
    fn test_config_validate_large_port() {
        let mut config = Config::default();
        config.ftp.port = 65535;
        config.sftp.port = 65534;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validate_sftp_port_different_from_ftp() {
        let mut config = Config::default();
        config.ftp.port = 21;
        config.sftp.port = 22;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let config = Config::default();
        config.save(&path).unwrap();

        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.ftp.port, config.ftp.port);
        assert_eq!(loaded.sftp.port, config.sftp.port);
    }

    #[test]
    fn test_config_load_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");

        let config = Config::load(&path).unwrap();
        assert_eq!(config.ftp.port, 21);
    }

    #[test]
    fn test_config_validate_paths_anonymous_no_home() {
        let mut config = Config::default();
        config.ftp.allow_anonymous = true;
        config.ftp.anonymous_home = None;
        let warnings = config.validate_paths();
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("anonymous home directory is not configured"));
    }

    #[test]
    fn test_config_validate_paths_ftps_no_cert() {
        let mut config = Config::default();
        config.ftp.ftps.enabled = true;
        config.ftp.ftps.cert_path = None;
        let warnings = config.validate_paths();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.contains("certificate path")));
    }

    #[test]
    fn test_config_get_users_path() {
        let path = Config::get_users_path();
        assert!(path.to_string_lossy().contains("users.json"));
    }

    #[test]
    fn test_config_get_config_path() {
        let path = Config::get_config_path();
        assert!(path.to_string_lossy().contains("config.toml"));
    }
}
