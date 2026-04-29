//! SFTP server core module
//!
//! Provides main structure and state management for SFTP server

use anyhow::Result;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use russh::MethodKind;
use russh::keys::*;
use russh::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::Mutex as TokioMutex;

use crate::core::config::{Config, get_program_data_path};
use crate::core::fail2ban::{Fail2BanConfig, Fail2BanManager};
use crate::core::quota::QuotaManager;
use crate::core::users::UserManager;

mod attr_ops;
mod cmd_dispatch;
mod dir_ops;
mod extended;
mod file_ops;
pub mod handler;
mod link_ops;
mod lock_ops;
mod state;
mod types;

pub use state::SftpState;
pub use types::{
    DirEntry, HANDLE_TIMEOUT_SECS, MAX_BUFFER_SIZE, MAX_HANDLES, MAX_PACKET_SIZE,
    SFTP_READ_BUFFER_SIZE, SFTP_WRITE_FLUSH_THRESHOLD, SSH_FXF_APPEND, SSH_FXF_CREAT, SSH_FXF_EXCL,
    SSH_FXF_READ, SSH_FXF_TRUNC, SSH_FXF_WRITE, SftpFileHandle,
};

#[derive(Clone)]
pub struct SftpServer {
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    fail2ban_manager: Arc<Fail2BanManager>,
    running: Arc<Mutex<bool>>,
    shutdown_tx: Arc<TokioMutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    last_key_rotation: Arc<TokioMutex<Option<DateTime<Utc>>>>,
    active_sessions: Arc<Mutex<HashMap<String, Arc<AtomicU32>>>>,
}

impl SftpServer {
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

        SftpServer {
            config,
            user_manager,
            quota_manager: Arc::new(quota_manager),
            fail2ban_manager,
            running: Arc::new(Mutex::new(false)),
            shutdown_tx: Arc::new(TokioMutex::new(None)),
            last_key_rotation: Arc::new(TokioMutex::new(None)),
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let (
            bind_ip,
            sftp_port,
            host_key_path,
            warnings,
            key_rotation_days,
            max_auth_attempts,
            auth_timeout,
        ) = {
            let cfg = self.config.lock();
            let warnings = cfg.validate_paths();
            (
                cfg.sftp.bind_ip.clone(),
                cfg.sftp.port,
                cfg.sftp.host_key_path.clone(),
                warnings,
                cfg.sftp.host_key_rotation_days,
                cfg.sftp.max_auth_attempts,
                cfg.sftp.auth_timeout,
            )
        };

        if !warnings.is_empty() {
            for warning in &warnings {
                tracing::error!("Config validation failed: {}", warning);
            }
            return Err(anyhow::anyhow!(
                "Config path validation failed: {}",
                warnings.join("; ")
            ));
        }

        tracing::info!("SFTP server starting on {}:{}", bind_ip, sftp_port);

        if key_rotation_days > 0 {
            self.check_and_rotate_key(&host_key_path, key_rotation_days)
                .await?;
        }

        let host_key = Self::load_or_generate_host_key(&host_key_path).await?;

        let mut methods = MethodSet::empty();
        methods.push(MethodKind::Password);
        methods.push(MethodKind::PublicKey);

        let ssh_config = russh::server::Config {
            keys: vec![host_key],
            methods,
            max_auth_attempts: max_auth_attempts as usize,
            auth_rejection_time: std::time::Duration::from_secs(1),
            auth_rejection_time_initial: Some(std::time::Duration::from_millis(0)),
            inactivity_timeout: Some(std::time::Duration::from_secs(auth_timeout)),
            keepalive_interval: Some(std::time::Duration::from_secs(30)),
            keepalive_max: 3,
            nodelay: true,
            ..Default::default()
        };

        let config = Arc::new(ssh_config);

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        {
            let mut tx = self.shutdown_tx.lock().await;
            *tx = Some(shutdown_tx);
        }

        {
            let mut running = self.running.lock();
            *running = true;
        }

        let user_manager_clone = Arc::clone(&self.user_manager);
        let running_clone = Arc::clone(&self.running);
        let config_clone = Arc::clone(&self.config);
        let quota_manager_clone = Arc::clone(&self.quota_manager);
        let sftp_server_for_handler = self.clone();

        let bind_addr = format!("{}:{}", bind_ip, sftp_port);

        let listener = {
            use socket2::{Domain, Protocol, SockAddr, Socket, Type};
            let domain = if bind_ip == "::" || (bind_ip.starts_with('[') && bind_ip.ends_with(']'))
            {
                Domain::IPV6
            } else {
                Domain::IPV4
            };

            let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;

            if domain == Domain::IPV6 {
                socket.set_only_v6(false)?;
            }

            socket.set_reuse_address(true)?;
            socket.set_nonblocking(true)?;
            let addr: std::net::SocketAddr = bind_addr
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid bind address '{}': {}", bind_addr, e))?;
            socket.bind(&SockAddr::from(addr))?;
            socket.listen(128)?;
            tokio::net::TcpListener::from_std(socket.into())
                .map_err(|e| anyhow::anyhow!("Failed to create tokio listener: {}", e))?
        };

        tracing::info!("SFTP server started on {}", bind_addr);

        Arc::clone(&self.fail2ban_manager).start_cleanup_task();

        let fail2ban_manager_clone = Arc::clone(&self.fail2ban_manager);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((socket, peer_addr)) => {
                                let ssh_config = Arc::clone(&config);
                                let user_manager = Arc::clone(&user_manager_clone);
                                let quota_manager = Arc::clone(&quota_manager_clone);
                                let client_ip = peer_addr.ip().to_string();

                                if fail2ban_manager_clone.is_banned(&client_ip).await {
                                    tracing::warn!(
                                        "SFTP connection rejected from {}: IP is banned by Fail2Ban",
                                        client_ip
                                    );
                                    continue;
                                }

                                let ip_allowed = {
                                    let cfg = config_clone.lock();
                                    cfg.is_ip_allowed(&client_ip)
                                };

                                if !ip_allowed {
                                    tracing::warn!(
                                        "SFTP connection rejected from {}: IP not allowed by blacklist/whitelist",
                                        client_ip
                                    );
                                    continue;
                                }

                                let config_for_check = Arc::clone(&config_clone);
                                let connection_allowed = {
                                    let cfg = config_for_check.lock();
                                    cfg.try_register_connection(&client_ip)
                                };

                                if !connection_allowed {
                                    tracing::warn!(
                                        "SFTP connection rejected from {}: connection limit exceeded",
                                        client_ip
                                    );
                                    continue;
                                }

                                tracing::info!(
                                    client_ip = %client_ip,
                                    action = "CONNECT",
                                    protocol = "SFTP",
                                    "Client connected from {}", client_ip
                                );

                                let client_ip_clone = client_ip.clone();
                                let fail2ban_manager = Arc::clone(&fail2ban_manager_clone);
                                let config_for_handler = Arc::clone(&config_clone);
                                let sftp_server_clone = sftp_server_for_handler.clone();
                                tokio::spawn(async move {
                                    let client_ip_for_cleanup = client_ip_clone.clone();
                                    let config_for_cleanup = Arc::clone(&config_for_check);

                                    let _guard = scopeguard::guard((), move |_| {
                                        let cfg = config_for_cleanup.lock();
                                        cfg.unregister_connection(&client_ip_for_cleanup);
                                    });

                                    let max_sessions = {
                                        let cfg = config_for_handler.lock();
                                        cfg.sftp.max_sessions_per_user
                                    };

                                    let handler = crate::core::sftp_server::handler::SftpHandler {
                                        user_manager,
                                        quota_manager,
                                        fail2ban_manager,
                                        sftp_server: Some(Arc::new(sftp_server_clone)),
                                        auth: crate::core::sftp_server::handler::AuthContext {
                                            authenticated: false,
                                            username: None,
                                            home_dir: None,
                                            auth_attempts: 0,
                                            max_auth_attempts,
                                            auth_start_time: Some(std::time::Instant::now()),
                                            auth_timeout_secs: auth_timeout,
                                        },
                                        sftp_channel: None,
                                        sftp_state: None,
                                        client_ip: client_ip.clone(),
                                        users_path: get_program_data_path().join("users.json"),
                                        max_sessions_per_user: max_sessions,
                                    };

                                    if let Err(e) = russh::server::run_stream(ssh_config, socket, handler).await {
                                        tracing::error!("SSH connection error from {}: {}", peer_addr, e);
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::error!("Failed to accept connection: {}", e);
                            }
                        }
                    }
                }
            }

            let mut running = running_clone.lock();
            *running = false;
        });

        Ok(())
    }

    pub async fn stop(&self) {
        {
            let mut tx = self.shutdown_tx.lock().await;
            if let Some(sender) = tx.take()
                && let Err(e) = sender.send(())
            {
                tracing::warn!("Failed to send SFTP shutdown signal: {:?}", e);
            }
        }
        {
            let mut running = self.running.lock();
            *running = false;
        }
        tracing::info!("SFTP server stopped");
    }

    pub fn is_running(&self) -> bool {
        *self.running.lock()
    }

    pub fn increment_session(&self, username: &str) {
        let mut sessions = self.active_sessions.lock();
        let counter = sessions
            .entry(username.to_string())
            .or_insert_with(|| Arc::new(AtomicU32::new(0)));
        let new_count = counter.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::debug!("User {} session count: {}", username, new_count);
    }

    pub fn decrement_session(&self, username: &str) {
        let sessions = self.active_sessions.lock();
        if let Some(counter) = sessions.get(username) {
            let old_count = counter.fetch_sub(1, Ordering::SeqCst);
            if old_count > 1 {
                tracing::debug!("User {} session decremented to {}", username, old_count - 1);
            } else if old_count == 1 {
                drop(sessions);
                let mut sessions_mut = self.active_sessions.lock();
                if let Some(c) = sessions_mut.get(username)
                    && c.load(Ordering::SeqCst) == 0
                {
                    sessions_mut.remove(username);
                    tracing::debug!("User {} removed from session tracking", username);
                }
            } else {
                tracing::warn!(
                    "User {} session count underflow detected (was {})",
                    username,
                    old_count
                );
            }
        } else {
            tracing::warn!("User {} not found in session tracking", username);
        }
    }

    pub fn get_session_count(&self, username: &str) -> u32 {
        let sessions = self.active_sessions.lock();
        sessions
            .get(username)
            .map(|counter| counter.load(Ordering::SeqCst))
            .unwrap_or(0)
    }

    async fn check_and_rotate_key(&self, key_path: &str, rotation_days: u32) -> Result<()> {
        let path = PathBuf::from(key_path);

        if !path.exists() {
            tracing::info!(
                "SFTP host key does not exist, will generate new key: {}",
                path.display()
            );
            return Ok(());
        }

        let last_rotation = *self.last_key_rotation.lock().await;
        let now = Utc::now();

        let should_rotate = match last_rotation {
            Some(last_time) => {
                let age = now.signed_duration_since(last_time);
                age.num_days() >= rotation_days as i64
            }
            None => {
                if let Ok(metadata) = tokio::fs::metadata(&path).await {
                    if let Ok(modified) = metadata.modified() {
                        let file_age = now.signed_duration_since(DateTime::<Utc>::from(modified));
                        file_age.num_days() >= rotation_days as i64
                    } else {
                        true
                    }
                } else {
                    true
                }
            }
        };

        if should_rotate {
            tracing::info!(
                "SFTP host key has reached rotation period ({} days), generating new key...",
                rotation_days
            );

            let backup_path = format!("{}.backup.{}", key_path, now.format("%Y%m%d_%H%M%S"));
            if let Err(e) = tokio::fs::copy(&path, &backup_path).await {
                tracing::warn!("Failed to backup old key: {}", e);
            } else {
                tracing::info!("Old key backed up to: {}", backup_path);
            }

            self.generate_new_host_key(&path).await?;

            *self.last_key_rotation.lock().await = Some(now);

            tracing::info!("SFTP host key rotation completed");
        } else {
            let age = last_rotation
                .map(|t| now.signed_duration_since(t).num_days())
                .unwrap_or(0);
            tracing::debug!(
                "SFTP host key rotation not needed, current age: {} days, rotation period: {} days",
                age,
                rotation_days
            );
        }

        Ok(())
    }

    async fn generate_new_host_key(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut rng = russh::keys::key::safe_rng();
        let key = PrivateKey::random(&mut rng, keys::Algorithm::Ed25519)?;
        let openssh = key.to_openssh(keys::ssh_key::LineEnding::default())?;
        tokio::fs::write(path, openssh.to_string()).await?;
        tracing::info!("Generated new SFTP host private key: {}", path.display());

        let pub_path = path.with_extension("pub");
        let public_key = key.public_key();
        let pub_openssh = public_key.to_openssh()?;
        tokio::fs::write(&pub_path, pub_openssh.to_string()).await?;
        tracing::info!("Generated new SFTP host public key: {}", pub_path.display());

        Ok(())
    }

    async fn load_or_generate_host_key(path: &str) -> Result<PrivateKey> {
        let path = PathBuf::from(path);

        if path.exists() {
            let key_data = tokio::fs::read_to_string(&path).await?;
            let key = PrivateKey::from_openssh(&key_data)?;
            tracing::info!("Loaded existing SFTP host key: {}", path.display());
            return Ok(key);
        }

        tracing::info!(
            "SFTP host key does not exist, generating new key: {}",
            path.display()
        );

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
            tracing::info!("Created SFTP key directory: {}", parent.display());
        }

        let mut rng = russh::keys::key::safe_rng();
        let key = PrivateKey::random(&mut rng, keys::Algorithm::Ed25519)?;
        let openssh = key.to_openssh(keys::ssh_key::LineEnding::default())?;
        tokio::fs::write(&path, openssh.to_string()).await?;
        tracing::info!("Generated SFTP host private key: {}", path.display());

        let pub_path = path.with_extension("pub");
        let public_key = key.public_key();
        let pub_openssh = public_key.to_openssh()?;
        tokio::fs::write(&pub_path, pub_openssh.to_string()).await?;
        tracing::info!("Generated SFTP host public key: {}", pub_path.display());

        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sftp_constants() {
        assert_eq!(SSH_FXF_READ, 0x00000001);
        assert_eq!(SSH_FXF_WRITE, 0x00000002);
        assert_eq!(SSH_FXF_APPEND, 0x00000004);
        assert_eq!(SSH_FXF_CREAT, 0x00000008);
        assert_eq!(SSH_FXF_TRUNC, 0x00000010);
        assert_eq!(SSH_FXF_EXCL, 0x00000020);
    }

    #[test]
    fn test_sftp_handle_timeout() {
        assert_eq!(HANDLE_TIMEOUT_SECS, 1800);
    }

    #[test]
    fn test_max_handles() {
        assert_eq!(MAX_HANDLES, 256);
    }

    #[test]
    fn test_max_packet_size() {
        assert_eq!(MAX_PACKET_SIZE, 256 * 1024);
    }
}
