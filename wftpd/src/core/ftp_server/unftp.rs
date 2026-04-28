//! FTP server implementation using libunftp
//!
//! Provides FTP/FTPS server with:
//! - Shared UserManager, QuotaManager, Fail2BanManager
//! - UPnP port mapping support
//! - Unified tracing logging
//! - Graceful shutdown support

use anyhow::Result;
use libunftp::options::{FailedLoginsPolicy, Shutdown};
use libunftp::ServerBuilder;
use parking_lot::Mutex;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;
use unftp_sbe_fs::Filesystem;

use crate::core::config::{get_program_data_path, Config};
use crate::core::fail2ban::{Fail2BanConfig, Fail2BanManager};
use crate::core::quota::QuotaManager;
use crate::core::users::UserManager;

use super::upnp_manager::UpnpManager;
use super::{
    LoggingPresenceListener, QuotaDataListener, QuotaFilesystem, UpnpBinderBuilder, WftpdAuthenticator,
};

pub struct FtpServer {
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    fail2ban_manager: Arc<Fail2BanManager>,
    upnp_manager: Arc<UpnpManager>,
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
            running: Arc::new(Mutex::new(false)),
            shutdown_tx: Arc::new(TokioMutex::new(None)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let (
            bind_ip,
            ftp_port,
            warnings,
            welcome_msg,
            passive_ports,
            idle_timeout,
            ftps_enabled,
            ftps_cert_path,
            ftps_key_path,
            ftps_require_ssl,
        ) = {
            let cfg = self.config.lock();
            let warnings = cfg.validate_paths();
            (
                cfg.ftp.bind_ip.clone(),
                cfg.ftp.port,
                warnings,
                cfg.ftp.welcome_message.clone(),
                cfg.ftp.passive_ports,
                cfg.ftp.idle_timeout,
                cfg.ftp.ftps.enabled,
                cfg.ftp.ftps.cert_path.clone(),
                cfg.ftp.ftps.key_path.clone(),
                cfg.ftp.ftps.require_ssl,
            )
        };

        if !warnings.is_empty() {
            for warning in &warnings {
                tracing::error!("Configuration validation failed: {}", warning);
            }
            return Err(anyhow::anyhow!(
                "Configuration path validation failed: {}",
                warnings.join("; ")
            ));
        }

        let bind_ip_log = bind_ip.clone();
        tracing::info!("FTP server starting on {}:{}", bind_ip_log, ftp_port);

        Arc::clone(&self.fail2ban_manager).start_cleanup_task();

        let upnp_init = Arc::clone(&self.upnp_manager);
        tokio::spawn(async move {
            if let Err(e) = upnp_init.initialize().await {
                tracing::warn!("UPnP initialization failed: {}", e);
            }
        });

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        {
            let mut tx = self.shutdown_tx.lock().await;
            *tx = Some(shutdown_tx);
        }

        {
            let mut running = self.running.lock();
            *running = true;
        }

        let user_manager_clone = Arc::clone(&self.user_manager);
        let quota_manager_clone = Arc::clone(&self.quota_manager);
        let fail2ban_clone = Arc::clone(&self.fail2ban_manager);
        let upnp_clone = Arc::clone(&self.upnp_manager);
        let running_clone = Arc::clone(&self.running);

        let users_path = get_program_data_path().join("users.json");

        tokio::spawn(async move {
            let result = Self::run_ftp_server(
                bind_ip_log,
                ftp_port,
                welcome_msg,
                passive_ports,
                idle_timeout,
                ftps_enabled,
                ftps_cert_path,
                ftps_key_path,
                ftps_require_ssl,
                user_manager_clone,
                quota_manager_clone,
                fail2ban_clone,
                upnp_clone,
                users_path,
                shutdown_rx,
            )
            .await;

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
        bind_ip: String,
        ftp_port: u16,
        welcome_msg: String,
        passive_ports: (u16, u16),
        idle_timeout: u64,
        ftps_enabled: bool,
        ftps_cert_path: Option<String>,
        ftps_key_path: Option<String>,
        ftps_require_ssl: bool,
        user_manager: Arc<Mutex<UserManager>>,
        quota_manager: Arc<QuotaManager>,
        fail2ban_manager: Arc<Fail2BanManager>,
        upnp_manager: Arc<UpnpManager>,
        users_path: std::path::PathBuf,
        shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<()> {
        let home_dir = {
            let users = user_manager.lock();
            users
                .get_users()
                .values()
                .next()
                .map(|u| u.home_dir.clone())
                .unwrap_or_else(|| {
                    std::env::var("USERPROFILE")
                        .or_else(|_| std::env::var("HOME"))
                        .unwrap_or_else(|_| ".".to_string())
                })
        };

        let local_ip = Self::get_local_ip_for_bind(&bind_ip);

        let user_mgr_clone = Arc::clone(&user_manager);
        let quota_mgr_clone = Arc::clone(&quota_manager);

        let storage_factory = Box::new(move || {
            let fs = Filesystem::new(home_dir.clone()).expect("Failed to create filesystem storage");
            QuotaFilesystem::new(fs, quota_mgr_clone.clone(), user_mgr_clone.clone())
        });

        let authenticator = Arc::new(WftpdAuthenticator::new(
            user_manager,
            users_path,
            Some(Arc::clone(&fail2ban_manager)),
        ));

        let shutdown_indicator = async {
            let _ = shutdown_rx.await;
            tracing::info!("FTP server shutdown signal received");
            Shutdown::new().grace_period(Duration::from_secs(10))
        };

        let mut server_builder = ServerBuilder::new(storage_factory)
            .authenticator(authenticator)
            .greeting(Box::leak(welcome_msg.into_boxed_str()))
            .passive_ports(passive_ports.0..=passive_ports.1)
            .idle_session_timeout(idle_timeout)
            .notify_data(QuotaDataListener::new())
            .notify_presence(LoggingPresenceListener::new())
            .failed_logins_policy(FailedLoginsPolicy::default())
            .shutdown_indicator(shutdown_indicator);

        if let Some(ip) = local_ip {
            server_builder = server_builder.passive_host(ip);
        }

        if let Some(binder) = UpnpBinderBuilder::new()
            .upnp_manager(upnp_manager)
            .local_ip(local_ip.unwrap_or(Ipv4Addr::new(127, 0, 0, 1)))
            .build()
        {
            server_builder = server_builder.binder(binder);
        }

        if ftps_enabled {
            if let (Some(cert_path), Some(key_path)) = (ftps_cert_path, ftps_key_path) {
                server_builder = server_builder
                    .ftps(&cert_path, &key_path)
                    .ftps_required(ftps_require_ssl, ftps_require_ssl);
                tracing::info!("FTPS enabled with certificate: {}", cert_path);
            } else {
                tracing::warn!("FTPS enabled but certificate or key path not configured");
            }
        }

        let server = server_builder.build()?;

        let bind_addr = format!("{}:{}", bind_ip, ftp_port);

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
        socket.connect("8.8.8.8:80")?;
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
