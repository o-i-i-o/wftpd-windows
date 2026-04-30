//! FTP server implementation using libunftp
//!
//! Provides FTP/FTPS server with:
//! - Shared UserManager, QuotaManager, Fail2BanManager
//! - UPnP port mapping support
//! - Unified tracing logging
//! - Graceful shutdown support
//! - Per-user home directory support via unftp-sbe-rooter
//! - Permission control via unftp-sbe-restrict
//! - IP security and connection limits

use anyhow::Result;
use libunftp::ServerBuilder;
use libunftp::options::{FailedLoginsBlock, FailedLoginsPolicy, PassiveHost, Shutdown};
use parking_lot::Mutex;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;
use unftp_sbe_fs::{Filesystem, Meta};
use unftp_sbe_restrict::RestrictingVfs;

use crate::core::config::{Config, get_program_data_path};
use crate::core::fail2ban::{Fail2BanConfig, Fail2BanManager};
use crate::core::quota::QuotaManager;
use crate::core::users::UserManager;

use super::auth::{SessionTracker, WftpdAuthenticator, WftpdUser, WftpdUserDetailProvider};
use super::cert_gen;
use super::passive_mode::{format_listen_addresses, get_local_ipv4_addresses, is_wildcard_bind};
use super::upnp_manager::UpnpManager;
use super::{FtpDataListener, FtpPresenceListener, QuotaFilesystem, UpnpBinderBuilder};

struct FtpServerConfig {
    bind_ip: String,
    ftp_port: u16,
    welcome_msg: String,
    passive_ports: (u16, u16),
    idle_timeout: u64,
    ftps_enabled: bool,
    ftps_cert_path: Option<String>,
    ftps_key_path: Option<String>,
    ftps_require_ssl: bool,
    pooled_listener_mode: bool,
    upnp_enabled: bool,
    masquerade_address: Option<String>,
    allow_nat_clients: bool,
    ftp_root: Option<String>,
}

struct FtpServerResources {
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    fail2ban_manager: Arc<Fail2BanManager>,
    upnp_manager: Arc<UpnpManager>,
    session_tracker: Arc<SessionTracker>,
    users_path: std::path::PathBuf,
    config: Arc<Mutex<Config>>,
}

static GREETING: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub struct FtpServer {
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    fail2ban_manager: Arc<Fail2BanManager>,
    upnp_manager: Arc<UpnpManager>,
    session_tracker: Arc<SessionTracker>,
    running: Arc<Mutex<bool>>,
    shutdown_tx: Arc<TokioMutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl FtpServer {
    pub fn new(config: Arc<Mutex<Config>>, user_manager: Arc<Mutex<UserManager>>) -> Self {
        let quota_manager = QuotaManager::new(&get_program_data_path());

        let fail2ban_config_inner = {
            let cfg = config.lock();
            Fail2BanConfig {
                enabled: cfg.security.fail2ban_enabled,
                threshold: cfg.security.fail2ban_threshold,
                ban_time: cfg.security.fail2ban_ban_time,
                find_time: 600,
            }
        };
        let fail2ban_manager = Arc::new(Fail2BanManager::new(fail2ban_config_inner));

        let upnp_enabled = {
            let cfg = config.lock();
            cfg.ftp.upnp_enabled
        };
        let upnp_manager = Arc::new(UpnpManager::new(upnp_enabled));

        FtpServer {
            config,
            user_manager,
            quota_manager: Arc::new(quota_manager),
            fail2ban_manager,
            upnp_manager,
            session_tracker: Arc::new(SessionTracker::new()),
            running: Arc::new(Mutex::new(false)),
            shutdown_tx: Arc::new(TokioMutex::new(None)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let server_config = {
            let cfg = self.config.lock();
            let warnings = cfg.validate_paths();

            if !warnings.is_empty() {
                for warning in &warnings {
                    tracing::error!("Configuration validation failed: {}", warning);
                }
                return Err(anyhow::anyhow!(
                    "Configuration path validation failed: {}",
                    warnings.join("; ")
                ));
            }

            FtpServerConfig {
                bind_ip: cfg.ftp.bind_ip.clone(),
                ftp_port: cfg.ftp.port,
                welcome_msg: cfg.ftp.welcome_message.clone(),
                passive_ports: cfg.ftp.passive_ports,
                idle_timeout: cfg.ftp.idle_timeout,
                ftps_enabled: cfg.ftp.ftps.enabled,
                ftps_cert_path: cfg.ftp.ftps.cert_path.clone(),
                ftps_key_path: cfg.ftp.ftps.key_path.clone(),
                ftps_require_ssl: cfg.ftp.ftps.require_ssl,
                pooled_listener_mode: cfg.ftp.pooled_listener_mode,
                upnp_enabled: cfg.ftp.upnp_enabled,
                masquerade_address: cfg.ftp.masquerade_address.clone(),
                allow_nat_clients: cfg.ftp.allow_nat_clients,
                ftp_root: cfg.ftp.ftp_root.clone(),
            }
        };

        let listen_info = format_listen_addresses(&server_config.bind_ip, server_config.ftp_port);
        tracing::info!(
            "FTP server starting - listening on {} (allow_nat_clients: {}, upnp_enabled: {})",
            listen_info,
            server_config.allow_nat_clients,
            server_config.upnp_enabled
        );

        Arc::clone(&self.fail2ban_manager).start_cleanup_task();

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        {
            let mut tx = self.shutdown_tx.lock().await;
            *tx = Some(shutdown_tx);
        }

        let resources = FtpServerResources {
            user_manager: Arc::clone(&self.user_manager),
            quota_manager: Arc::clone(&self.quota_manager),
            fail2ban_manager: Arc::clone(&self.fail2ban_manager),
            upnp_manager: Arc::clone(&self.upnp_manager),
            session_tracker: Arc::clone(&self.session_tracker),
            users_path: get_program_data_path().join("users.json"),
            config: Arc::clone(&self.config),
        };

        {
            let mut running = self.running.lock();
            *running = true;
        }

        let running_clone = Arc::clone(&self.running);

        tokio::spawn(async move {
            let result = Self::run_ftp_server(server_config, resources, shutdown_rx).await;

            if let Err(e) = result {
                tracing::error!("FTP server error: {}", e);
            }

            let mut running = running_clone.lock();
            *running = false;
        });

        tracing::info!("FTP server started");
        Ok(())
    }

    async fn run_ftp_server(
        config: FtpServerConfig,
        resources: FtpServerResources,
        shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<()> {
        let fallback_root = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());

        let fallback_path = std::path::Path::new(&fallback_root);
        if !fallback_path.exists() {
            return Err(anyhow::anyhow!(
                "FTP server fallback root directory does not exist: {}",
                fallback_root
            ));
        }

        if let Err(e) = resources.upnp_manager.initialize().await {
            tracing::warn!("UPnP initialization failed: {}", e);
        }

        let local_ip = Self::get_local_ip_for_bind(&config.bind_ip);

        let user_mgr_clone = Arc::clone(&resources.user_manager);
        let quota_mgr_clone = Arc::clone(&resources.quota_manager);

        let ftp_root = config
            .ftp_root
            .clone()
            .unwrap_or_else(|| fallback_root.clone());

        if !std::path::Path::new(&ftp_root).exists() {
            return Err(anyhow::anyhow!(
                "FTP server root directory does not exist: {}",
                ftp_root
            ));
        }

        let ftp_root_clone = ftp_root.clone();
        let storage_factory = Box::new(move || {
            let fs = Filesystem::new(&ftp_root_clone).unwrap_or_else(|e| {
                tracing::error!(
                    "Failed to create filesystem storage for '{}': {}",
                    ftp_root_clone,
                    e
                );
                Filesystem::new(".").unwrap_or_else(|_| {
                    std::process::exit(1);
                })
            });
            let quota_fs =
                QuotaFilesystem::new(fs, quota_mgr_clone.clone(), user_mgr_clone.clone());
            RestrictingVfs::<_, WftpdUser, Meta>::new(quota_fs)
        });

        let config_clone = Arc::clone(&resources.config);
        let authenticator = Arc::new(WftpdAuthenticator::new(
            resources.user_manager.clone(),
            resources.users_path.clone(),
            Some(Arc::clone(&resources.fail2ban_manager)),
            Some(config_clone),
            Arc::clone(&resources.session_tracker),
        ));

        let user_detail_provider = Arc::new(WftpdUserDetailProvider::new(
            resources.user_manager,
            resources.users_path,
        ));

        let shutdown_indicator = async {
            let _ = shutdown_rx.await;
            tracing::info!("FTP server shutdown signal received");
            Shutdown::new().grace_period(Duration::from_secs(10))
        };

        let _ = GREETING.set(config.welcome_msg);
        let greeting: &'static str = GREETING.get().map(|s| s.as_str()).unwrap_or("Welcome");

        let presence_listener = FtpPresenceListener::new(
            Arc::clone(&resources.session_tracker),
            Arc::clone(&resources.config),
        );

        let mut server_builder =
            ServerBuilder::with_user_detail_provider(storage_factory, user_detail_provider)
                .authenticator(authenticator)
                .greeting(greeting)
                .passive_ports(config.passive_ports.0..=config.passive_ports.1)
                .idle_session_timeout(config.idle_timeout)
                .notify_data(FtpDataListener::new())
                .notify_presence(presence_listener)
                .failed_logins_policy(FailedLoginsPolicy::new(
                    u32::MAX,
                    Duration::MAX,
                    FailedLoginsBlock::IP,
                ))
                .shutdown_indicator(shutdown_indicator);

        if config.pooled_listener_mode {
            tracing::info!("Enabling pooled listener mode for high performance");
            server_builder = server_builder.pooled_listener_mode();
        }

        let upnp_external_ip = if config.upnp_enabled {
            resources.upnp_manager.get_external_ip().await
        } else {
            None
        };

        let masquerade_ip = config.masquerade_address.as_ref().and_then(|s| {
            if s.is_empty() {
                None
            } else {
                s.parse::<Ipv4Addr>().ok()
            }
        });

        let bind_address: std::net::IpAddr = config
            .bind_ip
            .parse()
            .unwrap_or(std::net::IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));

        let server_local_ips = get_local_ipv4_addresses();
        tracing::debug!("Server local IPv4 addresses: {:?}", server_local_ips);

        let passive_host = if config.upnp_enabled
            && let Some(ref upnp_ip) = upnp_external_ip
            && let Ok(ip) = upnp_ip.parse::<Ipv4Addr>()
        {
            tracing::info!("FTP passive host: {} (UPnP external IP)", ip);
            PassiveHost::Ip(ip)
        } else if let Some(masq_ip) = masquerade_ip
            && !masq_ip.is_unspecified()
        {
            tracing::info!("FTP passive host: {} (masquerade address)", masq_ip);
            PassiveHost::Ip(masq_ip)
        } else if !is_wildcard_bind(&bind_address) {
            if let std::net::IpAddr::V4(ipv4) = bind_address {
                tracing::info!("FTP passive host: {} (bind address)", ipv4);
                PassiveHost::Ip(ipv4)
            } else {
                let fallback_ip = server_local_ips
                    .iter()
                    .find(|ip| !ip.is_loopback() && !ip.is_link_local())
                    .copied()
                    .unwrap_or_else(|| local_ip.unwrap_or(Ipv4Addr::new(127, 0, 0, 1)));
                tracing::info!("FTP passive host: {} (IPv6 bind fallback)", fallback_ip);
                PassiveHost::Ip(fallback_ip)
            }
        } else {
            tracing::info!("FTP passive host: FromConnection (TCP destination IP)");
            PassiveHost::FromConnection
        };
        server_builder = server_builder.passive_host(passive_host);

        if let Some(binder) = UpnpBinderBuilder::new()
            .upnp_manager(resources.upnp_manager)
            .local_ip(local_ip.unwrap_or(Ipv4Addr::new(127, 0, 0, 1)))
            .passive_ports(config.passive_ports.0..=config.passive_ports.1)
            .build()
        {
            server_builder = server_builder.binder(binder);
        }

        if config.ftps_enabled {
            if let (Some(cert_path), Some(key_path)) = (
                config.ftps_cert_path.as_deref(),
                config.ftps_key_path.as_deref(),
            ) {
                if cert_path.is_empty() || key_path.is_empty() {
                    return Err(anyhow::anyhow!(
                        "FTPS enabled but certificate or key path is empty"
                    ));
                }

                match cert_gen::ensure_cert_exists(cert_path, key_path) {
                    Ok(true) => tracing::info!("Generated self-signed certificate for FTPS"),
                    Ok(false) => tracing::info!("Using existing FTPS certificate"),
                    Err(e) => {
                        tracing::error!("Certificate check/generation failed: {}", e);
                        return Err(anyhow::anyhow!(
                            "Certificate check/generation failed: {}",
                            e
                        ));
                    }
                }

                server_builder = server_builder
                    .ftps(cert_path, key_path)
                    .ftps_required(config.ftps_require_ssl, config.ftps_require_ssl);
                tracing::info!("FTPS enabled with certificate: {}", cert_path);
            } else {
                return Err(anyhow::anyhow!(
                    "FTPS enabled but certificate or key path not configured"
                ));
            }
        }

        let server = server_builder.build()?;

        let bind_addr = format!("{}:{}", config.bind_ip, config.ftp_port);

        tracing::info!("FTP server listening on {}", bind_addr);

        if let Err(e) = server.listen(&bind_addr).await {
            tracing::error!("FTP server listen error: {}", e);
            return Err(e.into());
        }

        tracing::info!("FTP server shutdown complete");
        Ok(())
    }

    fn get_local_ip_for_bind(bind_ip: &str) -> Option<Ipv4Addr> {
        if bind_ip == "0.0.0.0" || bind_ip == "::" {
            if let Ok(local_ip) = Self::get_local_ip() {
                return Some(local_ip);
            }
            return Some(Ipv4Addr::new(127, 0, 0, 1));
        }
        bind_ip.parse().ok()
    }

    fn get_local_ip() -> std::io::Result<Ipv4Addr> {
        use std::net::UdpSocket;

        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.connect(("223.5.5.5", 53))?;
        let local_addr = socket.local_addr()?;
        match local_addr.ip() {
            std::net::IpAddr::V4(ipv4) => Ok(ipv4),
            _ => Err(std::io::Error::other("Not an IPv4 address")),
        }
    }

    pub async fn stop(&self) {
        {
            let mut tx = self.shutdown_tx.lock().await;
            if let Some(sender) = tx.take()
                && let Err(e) = sender.send(())
            {
                tracing::warn!("Failed to send FTP shutdown signal: {:?}", e);
            }
        }
        tracing::info!("FTP server stop signal sent");
    }

    pub fn is_running(&self) -> bool {
        *self.running.lock()
    }
}
