//! SFTP server core module
//!
//! Provides main structure, state management and packet building tools for SFTP server

use anyhow::Result;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use russh::MethodKind;
use russh::keys::*;
use russh::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::Mutex as TokioMutex;

use crate::core::config::{Config, get_program_data_path};
use crate::core::fail2ban::{Fail2BanConfig, Fail2BanManager};
use crate::core::path_utils::{PathResolveError, safe_resolve_path_with_cwd};
use crate::core::quota::QuotaManager;
use crate::core::rate_limiter::RateLimiter;
use crate::core::users::UserManager;

mod attr_ops;
mod cmd_dispatch;
mod dir_ops;
mod extended;
mod file_ops;
pub mod handler;
mod link_ops;
mod lock_ops;

pub const SSH_FXF_READ: u32 = 0x00000001;
pub const SSH_FXF_WRITE: u32 = 0x00000002;
pub const SSH_FXF_APPEND: u32 = 0x00000004;
pub const SSH_FXF_CREAT: u32 = 0x00000008;
pub const SSH_FXF_TRUNC: u32 = 0x00000010;
pub const SSH_FXF_EXCL: u32 = 0x00000020;

pub const MAX_PACKET_SIZE: usize = 256 * 1024;
pub const MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024;
pub const MAX_HANDLES: usize = 256;
pub const SFTP_READ_BUFFER_SIZE: usize = 128 * 1024;
pub const SFTP_WRITE_FLUSH_THRESHOLD: usize = 64 * 1024;
pub const HANDLE_TIMEOUT_SECS: u64 = 1800;

pub enum SftpFileHandle {
    File {
        path: PathBuf,
        file: tokio::fs::File,
        locked: bool,
        lock_handle: Option<std::fs::File>,
        existed: bool,
        written_bytes: u64,
        read_bytes: u64,
        pending_flush_bytes: u64,
        last_access: std::time::Instant,
    },
    Dir {
        path: PathBuf,
        entries: Vec<DirEntry>,
        index: usize,
        last_access: std::time::Instant,
    },
}

#[derive(Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: u32,
}

#[derive(Clone)]
pub struct SftpServer {
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    fail2ban_manager: Arc<Fail2BanManager>,
    running: Arc<Mutex<bool>>,
    shutdown_tx: Arc<TokioMutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    last_key_rotation: Arc<TokioMutex<Option<DateTime<Utc>>>>,
    // Use atomic counter to track active sessions per user for better concurrency
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

        // Enable password and public key authentication
        let mut methods = MethodSet::empty();
        methods.push(MethodKind::Password);
        methods.push(MethodKind::PublicKey);
        // TODO: KeyboardInteractive requires russh upgrade to implement
        // methods.push(MethodKind::KeyboardInteractive);

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

        // Determine listening mode based on configured bind address
        let bind_addr = format!("{}:{}", bind_ip, sftp_port);

        let listener = {
            use socket2::{Domain, Protocol, SockAddr, Socket, Type};
            // Select IPv4 or IPv6 based on configured address type
            let domain = if bind_ip == "::" || (bind_ip.starts_with('[') && bind_ip.ends_with(']'))
            {
                Domain::IPV6
            } else {
                Domain::IPV4
            };

            let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;

            // Enable IPv6 dual-stack support only when configured as [::]
            if domain == Domain::IPV6 {
                socket.set_only_v6(false)?; // Allow IPv4 mapped to IPv6
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

                                    {
                                        let cfg = config_for_check.lock();
                                        cfg.unregister_connection(&client_ip_clone);
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
            if let Some(sender) = tx.take() {
                let _ = sender.send(());
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

    /// Increment user active session count (using atomic operations, lock-free)
    pub fn increment_session(&self, username: &str) {
        let mut sessions = self.active_sessions.lock();
        let counter = sessions
            .entry(username.to_string())
            .or_insert_with(|| Arc::new(AtomicU32::new(0)));
        let new_count = counter.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::debug!("User {} session count: {}", username, new_count);
    }

    /// Decrement user active session count (using atomic operations, lock-free)
    pub fn decrement_session(&self, username: &str) {
        let sessions = self.active_sessions.lock();
        if let Some(counter) = sessions.get(username) {
            let old_count = counter.fetch_sub(1, Ordering::SeqCst);
            if old_count > 0 {
                let new_count = old_count - 1;
                tracing::debug!("User {} session decremented to {}", username, new_count);

                // If count reaches zero, remove from HashMap
                // Use compare_exchange to ensure cleanup only when count is actually 0, avoiding race conditions
                if new_count == 0 {
                    // Try to exchange counter from 0 to 0, success means no other thread modified it
                    if counter
                        .compare_exchange(0, 0, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                    {
                        drop(sessions); // Release lock before removal
                        let mut sessions_mut = self.active_sessions.lock();
                        // Check again to prevent new sessions from joining during lock release
                        if let Some(c) = sessions_mut.get(username) {
                            let current = c.load(Ordering::SeqCst);
                            if current == 0 {
                                sessions_mut.remove(username);
                                tracing::debug!("User {} removed from session tracking", username);
                            } else {
                                tracing::debug!(
                                    "User {} has new sessions ({}), skip cleanup",
                                    username,
                                    current
                                );
                            }
                        }
                    } else {
                        tracing::debug!(
                            "User {} session count changed during cleanup, skip removal",
                            username
                        );
                    }
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

    /// Get user active session count (atomic read)
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

pub struct SftpState {
    pub home_dir: String,
    pub cwd: String,
    pub username: Option<String>,
    pub user_manager: Arc<Mutex<UserManager>>,
    pub quota_manager: Arc<QuotaManager>,
    pub handles: HashMap<String, SftpFileHandle>,
    pub next_handle_id: u32,
    pub sftp_version: u32,
    pub buffer: Vec<u8>,
    pub locked_files: HashSet<PathBuf>,
    pub client_ip: String,
    pub cached_permissions: Option<crate::core::users::Permissions>,
    pub rate_limiter: Option<RateLimiter>,
    pub cache_expiry: Option<std::time::Instant>,
    pub last_handle_cleanup: std::time::Instant,
}

impl SftpState {
    pub fn new(
        home_dir: String,
        username: Option<String>,
        user_manager: Arc<Mutex<UserManager>>,
        quota_manager: Arc<QuotaManager>,
        client_ip: String,
    ) -> Self {
        let mut state = SftpState {
            home_dir: home_dir.clone(),
            cwd: home_dir,
            username,
            user_manager,
            quota_manager,
            handles: HashMap::new(),
            next_handle_id: 0,
            sftp_version: 3,
            buffer: Vec::new(),
            locked_files: HashSet::new(),
            client_ip,
            cached_permissions: None,
            rate_limiter: None,
            cache_expiry: None,
            last_handle_cleanup: std::time::Instant::now(),
        };
        state.cache_permissions();
        state.init_rate_limiter();
        state
    }

    pub fn check_permission(
        &mut self,
        check_fn: impl Fn(&crate::core::users::Permissions) -> bool,
    ) -> bool {
        if let Some(expiry) = self.cache_expiry
            && expiry < std::time::Instant::now()
        {
            self.cached_permissions = None;
            self.cache_permissions();
        }

        if let Some(perms) = &self.cached_permissions {
            return check_fn(perms);
        }

        if let Some(username) = &self.username {
            let users = self.user_manager.lock();
            if let Some(user) = users.get_user(username) {
                check_fn(&user.permissions)
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn cache_permissions(&mut self) {
        if let Some(username) = &self.username {
            let users = self.user_manager.lock();
            if let Some(user) = users.get_user(username) {
                self.cached_permissions = Some(user.permissions);
                self.cache_expiry =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
            }
        }
    }

    pub fn refresh_permissions(&mut self) {
        self.cached_permissions = None;
        self.cache_permissions();
    }

    pub fn init_rate_limiter(&mut self) {
        if let Some(perms) = &self.cached_permissions
            && let Some(speed_kbps) = perms.speed_limit_kbps
        {
            self.rate_limiter = Some(RateLimiter::new(speed_kbps));
        }
    }

    pub fn resolve_path_checked(
        &self,
        id: u32,
        path: &str,
    ) -> std::result::Result<PathBuf, Vec<u8>> {
        self.resolve_path(path).map_err(|e| {
            tracing::warn!("Path resolve failed for '{}': {}", path, e);
            self.build_status_packet(id, 2, &e.to_string(), "")
        })
    }

    pub async fn check_symlink_in_home(&self, path: &PathBuf) -> std::result::Result<(), Vec<u8>> {
        if !path.is_symlink() {
            return Ok(());
        }
        match tokio::fs::read_link(path).await {
            Ok(link_target) => {
                let resolved = if link_target.is_absolute() {
                    link_target
                } else {
                    let parent = path
                        .parent()
                        .unwrap_or(std::path::Path::new(&self.home_dir));
                    parent.join(&link_target)
                };
                if let Ok(canon) = resolved.canonicalize() {
                    let home = PathBuf::from(&self.home_dir);
                    if !crate::core::path_utils::path_starts_with_ignore_case(&canon, home) {
                        tracing::warn!("Symlink points outside home: {:?} -> {:?}", path, canon);
                        return Err(self.build_status_packet(
                            0,
                            3,
                            "Permission denied: symlink target outside home",
                            "",
                        ));
                    }
                }
                Ok(())
            }
            Err(e) => {
                tracing::warn!("Cannot read symlink {:?}: {}", path, e);
                Err(self.build_status_packet(0, 4, "Failed to read symlink", ""))
            }
        }
    }

    pub fn cleanup(&mut self) {
        for (_, handle) in self.handles.drain() {
            if let SftpFileHandle::File { locked, path, .. } = handle
                && locked
            {
                tracing::info!("Releasing lock on {:?} during cleanup", path);
                self.locked_files.remove(&path);
            }
        }
        self.locked_files.clear();
        tracing::info!("SFTP session cleanup completed");
    }

    pub async fn cleanup_expired_handles(&mut self) {
        let timeout = std::time::Duration::from_secs(HANDLE_TIMEOUT_SECS);
        let expired: Vec<String> = self
            .handles
            .iter()
            .filter(|(_, h)| match h {
                SftpFileHandle::File { last_access, .. } => last_access.elapsed() > timeout,
                SftpFileHandle::Dir { last_access, .. } => last_access.elapsed() > timeout,
            })
            .map(|(k, _)| k.clone())
            .collect();

        for handle_key in expired {
            if let Some(handle) = self.handles.remove(&handle_key) {
                match handle {
                    SftpFileHandle::File {
                        locked,
                        path,
                        mut file,
                        ..
                    } => {
                        use tokio::io::AsyncWriteExt;
                        let _ = file.flush().await;
                        if locked {
                            self.locked_files.remove(&path);
                        }
                        tracing::info!(
                            "SFTP handle expired and closed: {:?} (handle={})",
                            path,
                            handle_key
                        );
                    }
                    SftpFileHandle::Dir { path, .. } => {
                        tracing::info!(
                            "SFTP dir handle expired and closed: {:?} (handle={})",
                            path,
                            handle_key
                        );
                    }
                }
            }
        }
    }

    pub fn resolve_path(&self, path: &str) -> Result<PathBuf, PathResolveError> {
        safe_resolve_path_with_cwd(&self.cwd, &self.home_dir, path, false)
    }

    pub fn generate_handle(&mut self) -> String {
        let handle = format!("h{:08x}", self.next_handle_id);
        self.next_handle_id = self.next_handle_id.wrapping_add(1);
        handle
    }

    pub fn parse_u32(&self, data: &[u8], offset: usize) -> u32 {
        if offset + 4 > data.len() {
            return 0;
        }
        u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])
    }

    pub fn parse_u64(&self, data: &[u8], offset: usize) -> u64 {
        if offset + 8 > data.len() {
            return 0;
        }
        u64::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ])
    }

    pub fn parse_string(&self, data: &[u8], offset: usize) -> Result<String> {
        if offset + 4 > data.len() {
            return Ok(String::new());
        }
        let len = self.parse_u32(data, offset) as usize;
        if offset + 4 + len > data.len() {
            return Ok(String::new());
        }
        Ok(String::from_utf8_lossy(&data[offset + 4..offset + 4 + len]).to_string())
    }

    pub fn parse_string_with_len(&self, data: &[u8], offset: usize) -> Result<(String, usize)> {
        if offset + 4 > data.len() {
            return Ok((String::new(), 0));
        }
        let len = self.parse_u32(data, offset) as usize;
        if offset + 4 + len > data.len() {
            return Ok((String::new(), 0));
        }
        let s = String::from_utf8_lossy(&data[offset + 4..offset + 4 + len]).to_string();
        Ok((s, len))
    }

    pub fn build_packet(&self, payload: &[u8]) -> Vec<u8> {
        let mut packet = Vec::new();
        packet.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        packet.extend_from_slice(payload);
        packet
    }

    pub fn build_status_packet(&self, id: u32, status: u32, msg: &str, lang: &str) -> Vec<u8> {
        let mut payload = vec![101];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&status.to_be_bytes());
        payload.extend_from_slice(&(msg.len() as u32).to_be_bytes());
        payload.extend_from_slice(msg.as_bytes());
        payload.extend_from_slice(&(lang.len() as u32).to_be_bytes());
        payload.extend_from_slice(lang.as_bytes());
        self.build_packet(&payload)
    }

    pub fn build_handle_packet(&self, id: u32, handle: &str) -> Vec<u8> {
        let mut payload = vec![102];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&(handle.len() as u32).to_be_bytes());
        payload.extend_from_slice(handle.as_bytes());
        self.build_packet(&payload)
    }

    pub fn build_data_packet(&self, id: u32, data: &[u8]) -> Vec<u8> {
        let mut payload = vec![103];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&(data.len() as u32).to_be_bytes());
        payload.extend_from_slice(data);
        self.build_packet(&payload)
    }

    pub fn build_attrs(&self, is_dir: bool, size: u64) -> Vec<u8> {
        let mut attrs = Vec::new();
        let flags: u32 = 0x00000001 | 0x00000002 | 0x00000004 | 0x00000008;
        attrs.extend_from_slice(&flags.to_be_bytes());
        attrs.extend_from_slice(&size.to_be_bytes());
        let uid: u32 = 1000;
        let gid: u32 = 1000;
        attrs.extend_from_slice(&uid.to_be_bytes());
        attrs.extend_from_slice(&gid.to_be_bytes());
        let permissions = if is_dir { 0o40755u32 } else { 0o100644u32 };
        attrs.extend_from_slice(&permissions.to_be_bytes());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        attrs.extend_from_slice(&now.to_be_bytes());
        attrs.extend_from_slice(&now.to_be_bytes());
        attrs
    }

    pub fn build_attrs_extended(&self, metadata: &std::fs::Metadata, is_dir: bool) -> Vec<u8> {
        use std::os::windows::fs::MetadataExt;

        let mut attrs = Vec::new();

        let mut flags: u32 = 0x00000001 | 0x00000002 | 0x00000004 | 0x00000008;

        attrs.extend_from_slice(&flags.to_be_bytes());

        attrs.extend_from_slice(&metadata.len().to_be_bytes());

        let uid: u32 = 1000;
        let gid: u32 = 1000;
        attrs.extend_from_slice(&uid.to_be_bytes());
        attrs.extend_from_slice(&gid.to_be_bytes());

        let permissions = if is_dir { 0o40755u32 } else { 0o100644u32 };
        attrs.extend_from_slice(&permissions.to_be_bytes());

        let atime = metadata
            .accessed()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);

        attrs.extend_from_slice(&atime.to_be_bytes());
        attrs.extend_from_slice(&mtime.to_be_bytes());

        #[cfg(windows)]
        {
            if let Some(ctime) = metadata
                .creation_time()
                .checked_sub(116444736000000000)
                .map(|ns100| ns100 / 10_000_000)
            {
                flags |= 0x80000000;

                attrs[0..4].copy_from_slice(&flags.to_be_bytes());

                attrs.extend_from_slice(&1u32.to_be_bytes());

                let ext_name = "createtime";
                attrs.extend_from_slice(&(ext_name.len() as u32).to_be_bytes());
                attrs.extend_from_slice(ext_name.as_bytes());

                let ctime_str = ctime.to_string();
                attrs.extend_from_slice(&(ctime_str.len() as u32).to_be_bytes());
                attrs.extend_from_slice(ctime_str.as_bytes());
            }
        }

        attrs
    }
}
