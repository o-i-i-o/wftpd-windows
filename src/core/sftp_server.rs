use anyhow::Result;
use parking_lot::Mutex;
use russh::*;
use russh::keys::*;
use russh::keys::ssh_key::rand_core::OsRng;
use russh::server::Msg;
use russh::MethodKind;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::collections::HashSet;
use tokio::sync::Mutex as TokioMutex;

use crate::core::config::{Config, get_program_data_path};
use crate::core::users::UserManager;
use crate::core::quota::QuotaManager;
use crate::core::rate_limiter::RateLimiter;
use crate::core::path_utils::{safe_resolve_path_with_cwd, to_ftp_path, PathResolveError, path_starts_with_ignore_case};

const SSH_FXF_READ: u32 = 0x00000001;
const SSH_FXF_WRITE: u32 = 0x00000002;
const SSH_FXF_APPEND: u32 = 0x00000004;
const SSH_FXF_CREAT: u32 = 0x00000008;
const SSH_FXF_TRUNC: u32 = 0x00000010;
const SSH_FXF_EXCL: u32 = 0x00000020;

const MAX_PACKET_SIZE: usize = 256 * 1024;
const MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024;
// 优化的缓冲区大小配置
const SFTP_READ_BUFFER_SIZE: usize = 128 * 1024; // 128KB (从 32KB 提升)
const SFTP_WRITE_FLUSH_THRESHOLD: usize = 64 * 1024; // 64KB 刷新阈值

#[derive(Clone)]
pub struct SftpServer {
    config: Arc<Mutex<Config>>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    running: Arc<Mutex<bool>>,
    shutdown_tx: Arc<TokioMutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl SftpServer {
    pub fn new(
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> Self {
        let quota_manager = QuotaManager::new(&get_program_data_path());

        SftpServer {
            config,
            user_manager,
            quota_manager: Arc::new(quota_manager),
            running: Arc::new(Mutex::new(false)),
            shutdown_tx: Arc::new(TokioMutex::new(None)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let (bind_ip, sftp_port, host_key_path, warnings) = {
            let cfg = self.config.lock();
            let warnings = cfg.validate_paths();
            (
                cfg.sftp.bind_ip.clone(),
                cfg.sftp.port,
                cfg.sftp.host_key_path.clone(),
                warnings,
            )
        };

        if !warnings.is_empty() {
            for warning in &warnings {
                tracing::error!("配置验证失败: {}", warning);
            }
            return Err(anyhow::anyhow!("配置路径验证失败：{}", warnings.join("; ")));
        }

        tracing::info!("SFTP server starting on {}:{}", bind_ip, sftp_port);

        let host_key = Self::load_or_generate_host_key(&host_key_path).await?;

        let mut methods = MethodSet::empty();
        methods.push(MethodKind::Password);
        methods.push(MethodKind::PublicKey);
        
        // 创建 SSH 服务器配置
        let ssh_config = russh::server::Config {
            keys: vec![host_key],
            methods,
            ..Default::default()
        };
        
        // SSH 转发功能已禁用（SFTP 不需要这些功能）
        
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

        let bind_addr = format!("{}:{}", bind_ip, sftp_port);
        
        let listener = {
            use socket2::{Domain, Protocol, Socket, Type, SockAddr};
            let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
            socket.set_reuse_address(true)?;
            socket.set_nonblocking(true)?;
            let addr: std::net::SocketAddr = bind_addr.parse()
                .map_err(|e| anyhow::anyhow!("Invalid bind address: {}", e))?;
            socket.bind(&SockAddr::from(addr))?;
            socket.listen(128)?;
            tokio::net::TcpListener::from_std(socket.into())
                .map_err(|e| anyhow::anyhow!("Failed to create tokio listener: {}", e))?
        };

        tracing::info!("SFTP server started on {}", bind_addr);

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
                                
                                // 检查连接数限制
                                let config_for_check = Arc::clone(&config_clone);
                                let ip_allowed = {
                                    let cfg = config_for_check.lock();
                                    cfg.check_connection_limits(&client_ip)
                                };

                                if !ip_allowed {
                                    tracing::warn!(
                                        "Connection rejected from {}: connection limit exceeded",
                                        client_ip
                                    );
                                    continue;
                                }
                                
                                // 注册连接
                                {
                                    let cfg = config_for_check.lock();
                                    cfg.register_connection(&client_ip);
                                }

                                tracing::info!(
                                    client_ip = %client_ip,
                                    action = "CONNECT",
                                    protocol = "SFTP",
                                    "Client connected from {}", client_ip
                                );

                                let client_ip_clone = client_ip.clone();
                                tokio::spawn(async move {
                                    let handler = SftpHandler {
                                        user_manager,
                                        quota_manager,
                                        authenticated: false,
                                        username: None,
                                        home_dir: None,
                                        sftp_channel: None,
                                        sftp_state: None,
                                        client_ip: client_ip.clone(),
                                        users_path: get_program_data_path().join("users.json"),
                                    };

                                    if let Err(e) = russh::server::run_stream(ssh_config, socket, handler).await {
                                        tracing::error!("SSH connection error from {}: {}", peer_addr, e);
                                    }
                                    
                                    // 连接结束时注销
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

    async fn load_or_generate_host_key(path: &str) -> Result<PrivateKey> {
        let path = PathBuf::from(path);

        if path.exists() {
            let key_data = tokio::fs::read_to_string(&path).await?;
            let key = PrivateKey::from_openssh(&key_data)?;
            tracing::info!("已加载现有 SFTP 主机密钥: {}", path.display());
            return Ok(key);
        }

        tracing::info!("SFTP 主机密钥不存在，正在生成新密钥: {}", path.display());

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
            tracing::info!("已创建 SFTP 密钥目录: {}", parent.display());
        }

        let mut rng = OsRng;
        let key = PrivateKey::random(&mut rng, keys::Algorithm::Ed25519)?;
        let openssh = key.to_openssh(keys::ssh_key::LineEnding::default())?;
        tokio::fs::write(&path, openssh.to_string()).await?;
        tracing::info!("已生成 SFTP 主机私钥: {}", path.display());

        let pub_path = path.with_extension("pub");
        let public_key = key.public_key();
        let pub_openssh = public_key.to_openssh()?;
        tokio::fs::write(&pub_path, pub_openssh.to_string()).await?;
        tracing::info!("已生成 SFTP 主机公钥: {}", pub_path.display());

        Ok(key)
    }
}

struct SftpHandler {
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    authenticated: bool,
    username: Option<String>,
    home_dir: Option<String>,
    sftp_channel: Option<ChannelId>,
    sftp_state: Option<Arc<TokioMutex<SftpState>>>,
    client_ip: String,
    users_path: std::path::PathBuf,
}

struct SftpState {
    home_dir: String,
    cwd: String,
    username: Option<String>,
    user_manager: Arc<Mutex<UserManager>>,
    quota_manager: Arc<QuotaManager>,
    handles: HashMap<String, SftpFileHandle>,
    next_handle_id: u32,
    sftp_version: u32,
    buffer: Vec<u8>,
    locked_files: HashSet<PathBuf>,
    client_ip: String,
    cached_permissions: Option<crate::core::users::Permissions>,
    rate_limiter: Option<RateLimiter>,
    // 优化的权限缓存：按操作类型缓存
    permission_cache: std::collections::HashMap<String, bool>,
    cache_expiry: Option<std::time::Instant>,
}

enum SftpFileHandle {
    File {
        path: PathBuf,
        file: tokio::fs::File,
        locked: bool,
        existed: bool,
        written_bytes: u64,
        read_bytes: u64,
        // 优化的写入缓冲计数
        pending_flush_bytes: u64,
    },
    Dir {
        path: PathBuf,
        entries: Vec<(String, bool, u64)>,
        index: usize,
    },
}

impl russh::server::Handler for SftpHandler {
    type Error = anyhow::Error;

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        let mut users = self.user_manager.lock();

        if users.get_user(user).is_none() {
            let _ = users.reload(&self.users_path);
        }

        match users.authenticate(user, password) {
            Ok(true) => {
                if let Some(u) = users.get_user(user) {
                    match std::path::PathBuf::from(&u.home_dir).canonicalize() {
                        Ok(home_canon) => {
                            self.home_dir = Some(home_canon.to_string_lossy().to_string());
                        }
                        Err(e) => {
                            tracing::error!("SFTP auth failed: cannot canonicalize home directory '{}' for user '{}': {}", u.home_dir, user, e);
                            tracing::warn!(
                                client_ip = %self.client_ip,
                                username = %user,
                                action = "HOME_NOT_FOUND",
                                "Home directory not found for user {}: {}", user, u.home_dir
                            );
                            return Ok(server::Auth::Reject {
                                proceed_with_methods: None,
                                partial_success: false,
                            });
                        }
                    }
                }

                self.authenticated = true;
                self.username = Some(user.to_string());

                tracing::info!(
                    client_ip = %self.client_ip,
                    username = %user,
                    action = "LOGIN",
                    protocol = "SFTP",
                    "User {} logged in", user
                );

                Ok(server::Auth::Accept)
            }
            Ok(false) => {
                tracing::warn!(
                    client_ip = %self.client_ip,
                    username = %user,
                    action = "AUTH_FAIL",
                    protocol = "SFTP",
                    "Failed login attempt for user {}", user
                );
                Ok(server::Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                })
            }
            Err(e) => {
                tracing::error!(
                    client_ip = %self.client_ip,
                    username = %user,
                    action = "AUTH_ERROR",
                    protocol = "SFTP",
                    "Authentication error for user {}: {}", user, e
                );
                Ok(server::Auth::Reject { 
                    proceed_with_methods: None,
                    partial_success: false,
                })
            }
        }
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        let (enabled, user_pubkey_path, home_dir) = {
            let users = self.user_manager.lock();
            if let Some(u) = users.get_user(user) {
                (u.enabled, get_program_data_path().join(format!("keys/{}.pub", user)).to_string_lossy().to_string(), Some(u.home_dir.clone()))
            } else {
                (false, String::new(), None)
            }
        };
        
        if !enabled {
            tracing::warn!(
                client_ip = %self.client_ip,
                username = %user,
                action = "AUTH_FAIL",
                "Public key auth failed for user {}: user not found or disabled", user
            );
            return Ok(server::Auth::Reject { 
                proceed_with_methods: None,
                partial_success: false,
            });
        }
        
        if let Ok(stored_key) = tokio::fs::read_to_string(&user_pubkey_path).await
            && let Ok(stored_pubkey) = keys::parse_public_key_base64(stored_key.trim())
                && public_key == &stored_pubkey {
                    if let Some(ref hd) = home_dir {
                        match std::path::PathBuf::from(hd).canonicalize() {
                            Ok(home_canon) => {
                                self.home_dir = Some(home_canon.to_string_lossy().to_string());
                            }
                            Err(e) => {
                                tracing::error!("SFTP pubkey auth failed: cannot canonicalize home directory '{}' for user '{}': {}", hd, user, e);
                                tracing::warn!(
                                    client_ip = %self.client_ip,
                                    username = %user,
                                    action = "HOME_NOT_FOUND",
                                    "Home directory not found for user {}: {}", user, hd
                                );
                                return Ok(server::Auth::Reject {
                                    proceed_with_methods: None,
                                    partial_success: false,
                                });
                            }
                        }
                    }

                    self.authenticated = true;
                    self.username = Some(user.to_string());

                    tracing::info!(
                        client_ip = %self.client_ip,
                        username = %user,
                        action = "LOGIN",
                        "User {} logged in via public key", user
                    );

                    return Ok(server::Auth::Accept);
                }

        tracing::warn!(
            client_ip = %self.client_ip,
            username = %user,
            action = "AUTH_FAIL",
            "Public key auth failed for user {}: key mismatch or not found", user
        );

        Ok(server::Auth::Reject { 
            proceed_with_methods: None,
            partial_success: false,
        })
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        _session: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        Ok(self.authenticated)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        session: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        // TCP 转发已禁用（SFTP 不需要此功能）
        tracing::warn!(
            client_ip = %self.client_ip,
            action = "TCP_FORWARD_DISABLED",
            "TCP forwarding is disabled for SFTP"
        );
        let _ = session.channel_failure(channel.id());
        Ok(false)
    }

    async fn channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        session: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        // TCP 转发已禁用（SFTP 不需要此功能）
        tracing::warn!(
            client_ip = %self.client_ip,
            action = "FORWARDED_TCP_DISABLED",
            "Forwarded TCP connection is disabled for SFTP"
        );
        let _ = session.channel_failure(channel.id());
        Ok(false)
    }

    async fn tcpip_forward(
        &mut self,
        _address: &str,
        _port: &mut u32,
        _session: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        // TCP 端口转发已禁用（SFTP 不需要此功能）
        tracing::warn!(
            client_ip = %self.client_ip,
            action = "TCP_PORT_FORWARD_DISABLED",
            "TCP port forwarding is disabled for SFTP"
        );
        Ok(false)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        address: &str,
        port: u32,
        _session: &mut server::Session,
    ) -> Result<bool, Self::Error> {
        // 取消 TCP 端口转发（由于不允许转发，直接返回失败）
        tracing::warn!(
            client_ip = %self.client_ip,
            action = "TCP_FORWARD_CANCEL_DENIED",
            "Cancel TCP port forward denied: {}:{}",
            address, port
        );
        Ok(false)
    }

    async fn x11_request(
        &mut self,
        channel: ChannelId,
        _single_connection: bool,
        _auth_protocol: &str,
        _auth_cookie: &str,
        _screen_number: u32,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        // X11 转发已禁用（SFTP 不需要此功能）
        tracing::warn!(
            client_ip = %self.client_ip,
            action = "X11_FORWARD_DISABLED",
            "X11 forwarding is disabled for SFTP"
        );
        let _ = session.channel_failure(channel);
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if name == "sftp" && self.authenticated {
            if let Some(ref home_dir) = self.home_dir {
                let _ = session.channel_success(channel);
                
                self.sftp_channel = Some(channel);
                
                let username = self.username.clone();
                
                let mut state = SftpState {
                    home_dir: home_dir.clone(),
                    cwd: home_dir.clone(),
                    username,
                    user_manager: Arc::clone(&self.user_manager),
                    quota_manager: Arc::clone(&self.quota_manager),
                    handles: HashMap::new(),
                    next_handle_id: 0,
                    sftp_version: 3,
                    buffer: Vec::new(),
                    locked_files: HashSet::new(),
                    client_ip: self.client_ip.clone(),
                    cached_permissions: None,
                    rate_limiter: None,
                    permission_cache: std::collections::HashMap::new(),
                    cache_expiry: None,
                };
                state.cache_permissions();
                state.init_rate_limiter();
                
                self.sftp_state = Some(Arc::new(TokioMutex::new(state)));
            } else {
                tracing::error!("SFTP subsystem request failed: home directory not set");
                let _ = session.channel_failure(channel);
            }
        } else {
            let _ = session.channel_failure(channel);
        }
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if self.sftp_channel == Some(channel)
            && let Some(state) = &self.sftp_state {
                let state_clone = Arc::clone(state);
                let handle = session.handle();
                let data_vec = data.to_vec();
                
                tokio::spawn(async move {
                    let response = {
                        let mut state = state_clone.lock().await;
                        state.process_sftp_data(&data_vec).await
                    };
                    
                    if let Ok(resp) = response {
                        let _ = handle.data(channel, bytes::Bytes::from(resp)).await;
                    }
                });
            }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if self.sftp_channel == Some(channel)
            && let Some(state) = &self.sftp_state {
                let mut state = state.lock().await;
                state.cleanup();
            }
        Ok(())
    }
}

impl SftpState {
    async fn process_sftp_data(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        if self.buffer.len() + data.len() > MAX_BUFFER_SIZE {
            tracing::warn!(
                "SFTP buffer overflow attempt: buffer={}, incoming={}, max={}",
                self.buffer.len(),
                data.len(),
                MAX_BUFFER_SIZE
            );
            self.buffer.clear();
            return Ok(self.build_status_packet(0, 4, "Buffer overflow", ""));
        }
        
        self.buffer.extend_from_slice(data);

        let mut responses: Vec<u8> = Vec::new();

        while self.buffer.len() >= 4 {
            let packet_len = u32::from_be_bytes([
                self.buffer[0], self.buffer[1], self.buffer[2], self.buffer[3],
            ]) as usize;

            if packet_len > MAX_PACKET_SIZE {
                tracing::warn!("SFTP packet too large: {} bytes (max {})", packet_len, MAX_PACKET_SIZE);
                self.buffer.clear();
                return Ok(self.build_status_packet(0, 4, "Packet too large", ""));
            }

            if self.buffer.len() < 4 + packet_len {
                break;
            }

            let packet: Vec<u8> = self.buffer[4..4 + packet_len].to_vec();
            self.buffer.drain(0..4 + packet_len);

            if !packet.is_empty() {
                let response = self.handle_sftp_packet(&packet).await?;
                responses.extend_from_slice(&response);
            }
        }

        Ok(responses)
    }

    fn check_permission(&self, check_fn: impl Fn(&crate::core::users::Permissions) -> bool) -> bool {
        // 检查缓存是否过期（5 秒有效期）
        if let Some(expiry) = self.cache_expiry
            && std::time::Instant::now() < expiry {
            // 缓存有效，返回缓存结果
            if let Some(&result) = self.permission_cache.get("check") {
                return result;
            }
        }
        
        // 缓存失效或不存在，执行实际检查
        if let Some(perms) = &self.cached_permissions {
            check_fn(perms)
        } else if let Some(username) = &self.username {
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

    fn cache_permissions(&mut self) {
        if let Some(username) = &self.username {
            let users = self.user_manager.lock();
            if let Some(user) = users.get_user(username) {
                self.cached_permissions = Some(user.permissions);
                // 设置缓存有效期（5 秒）
                self.cache_expiry = Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
                // 预计算常用权限检查结果
                self.permission_cache.insert("check".to_string(), true);
            }
        }
    }
    
    fn refresh_permissions(&mut self) {
        self.cached_permissions = None;
        self.cache_permissions();
    }
    
    fn init_rate_limiter(&mut self) {
        if let Some(perms) = &self.cached_permissions
            && let Some(speed_kbps) = perms.speed_limit_kbps {
                self.rate_limiter = Some(RateLimiter::new(speed_kbps));
        }
    }

    fn cleanup(&mut self) {
        for (_, handle) in self.handles.drain() {
            if let SftpFileHandle::File { locked, path, .. } = handle
                && locked {
                    tracing::info!("Releasing lock on {:?} during cleanup", path);
                    self.locked_files.remove(&path);
                }
        }
        self.locked_files.clear();
        tracing::info!("SFTP session cleanup completed");
    }

    async fn handle_sftp_packet(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        if data.is_empty() {
            return Ok(self.build_status_packet(0, 5, "Bad packet", ""));
        }

        let msg_type = data[0];

        match msg_type {
            1 => self.handle_init(data).await,
            3 => self.handle_open(data).await,
            4 => self.handle_close(data).await,
            5 => self.handle_read(data).await,
            6 => self.handle_write(data).await,
            7 => self.handle_lstat(data).await,
            8 => self.handle_fstat(data).await,
            9 => self.handle_setstat(data).await,
            10 => self.handle_fsetstat(data).await,
            11 => self.handle_opendir(data).await,
            12 => self.handle_readdir(data).await,
            13 => self.handle_remove(data).await,
            14 => self.handle_mkdir(data).await,
            15 => self.handle_rmdir(data).await,
            16 => self.handle_realpath(data).await,
            17 => self.handle_stat(data).await,
            18 => self.handle_rename(data).await,
            19 => self.handle_readlink(data).await,
            20 => self.handle_symlink(data).await,
            40 => self.handle_lock(data).await,
            41 => self.handle_unlock(data).await,
            200 => self.handle_extended(data).await,
            _ => Ok(self.build_status_packet(0, 8, "Unsupported operation", "")),
        }
    }

    async fn handle_init(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let version = if data.len() >= 5 {
            u32::from_be_bytes([data[1], data[2], data[3], data[4]])
        } else {
            3
        };

        self.sftp_version = version.min(6);
        
        self.refresh_permissions();

        let mut payload = vec![2];
        payload.extend_from_slice(&self.sftp_version.to_be_bytes());
        Ok(self.build_packet(&payload))
    }

    async fn handle_opendir(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_list) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("OPENDIR failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        if !full_path.exists() {
            return Ok(self.build_status_packet(id, 2, "No such directory", ""));
        }

        if !full_path.is_dir() {
            return Ok(self.build_status_packet(id, 4, "Not a directory", ""));
        }

        let handle = self.generate_handle();
        self.handles.insert(handle.clone(), SftpFileHandle::Dir {
            path: full_path,
            entries: Vec::new(),
            index: 0,
        });

        Ok(self.build_handle_packet(id, &handle))
    }

    async fn handle_close(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let handle = self.parse_string(data, 5)?;

        if let Some(SftpFileHandle::File { path, locked, existed, written_bytes, read_bytes, pending_flush_bytes: _, mut file }) = self.handles.remove(&handle) {
            // 关闭前确保刷新所有未写入的数据
            use tokio::io::AsyncWriteExt;
            let _ = file.flush().await; // 忽略 flush 错误，因为可能已经关闭
            
            if locked {
                self.locked_files.remove(&path);
            }
            
            if written_bytes > 0 {
                let file_size = tokio::fs::metadata(&path).await.map(|m| m.len()).unwrap_or(written_bytes);
                
                if existed {
                    crate::file_op_log!(
                        update,
                        self.username.as_deref().unwrap_or("anonymous"),
                        &self.client_ip,
                        &path.to_string_lossy(),
                        file_size,
                        "SFTP"
                    );
                } else {
                    crate::file_op_log!(
                        upload,
                        self.username.as_deref().unwrap_or("anonymous"),
                        &self.client_ip,
                        &path.to_string_lossy(),
                        written_bytes,
                        "SFTP"
                    );
                }

            }

            if read_bytes > 0 {
                crate::file_op_log!(
                    download,
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    &path.to_string_lossy(),
                    read_bytes,
                    "SFTP"
                );
            }
        } else {
            self.handles.remove(&handle);
        }
        Ok(self.build_status_packet(id, 0, "OK", ""))
    }

    async fn handle_readdir(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let handle_str = self.parse_string(data, 5)?;

        let entries_result = {
            let handle = self.handles.get_mut(&handle_str);
            match handle {
                Some(SftpFileHandle::Dir { path, entries, index }) => {
                    if entries.is_empty() && *index == 0 {
                        let mut read_entries = Vec::new();
                        match tokio::fs::read_dir(path).await {
                            Ok(mut dir) => {
                                while let Ok(Some(entry)) = dir.next_entry().await {
                                    let name = entry.file_name().to_string_lossy().to_string();
                                    let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                                    let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
                                    read_entries.push((name, is_dir, size));
                                }
                            }
                            Err(e) => {
                                return Ok(self.build_status_packet(id, 4, &format!("Failed to read directory: {}", e), ""));
                            }
                        }
                        *entries = read_entries;
                    }

                    if *index >= entries.len() {
                        return Ok(self.build_status_packet(id, 1, "End of directory", ""));
                    }

                    let count = (entries.len() - *index).min(100);
                    let result_entries: Vec<(String, bool, u64)> = entries[*index..*index + count].to_vec();
                    *index += count;
                    Some(result_entries)
                }
                _ => None,
            }
        };

        match entries_result {
            Some(dir_entries) => {
                let mut payload = vec![104];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&(dir_entries.len() as u32).to_be_bytes());
                
                for (name, is_dir, size) in dir_entries {
                    payload.extend_from_slice(&(name.len() as u32).to_be_bytes());
                    payload.extend_from_slice(name.as_bytes());
                    
                    let long_name = format!("{} 1 user user {:>10} Jan 01 00:00 {}", 
                        if is_dir { "drwxr-xr-x" } else { "-rw-r--r--" },
                        size, name
                    );
                    payload.extend_from_slice(&(long_name.len() as u32).to_be_bytes());
                    payload.extend_from_slice(long_name.as_bytes());
                    
                    payload.extend_from_slice(&self.build_attrs(is_dir, size));
                }

                Ok(self.build_packet(&payload))
            }
            None => Ok(self.build_status_packet(id, 4, "Invalid handle", "")),
        }
    }

    async fn handle_read(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let (handle_str, handle_len) = self.parse_string_with_len(data, 5)?;
        let offset_pos = 5 + 4 + handle_len;
        let offset = self.parse_u64(data, offset_pos);
        let len = self.parse_u32(data, offset_pos + 8) as usize;

        if !self.check_permission(|p| p.can_read) {
            tracing::warn!("SFTP READ denied: no read permission for user {:?}", self.username);
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File { path, file, read_bytes, .. }) => {
                use tokio::io::{AsyncSeekExt, AsyncReadExt};
                
                if let Err(e) = file.seek(std::io::SeekFrom::Start(offset)).await {
                    tracing::error!("SFTP READ seek error for {:?}: {}", path, e);
                    return Ok(self.build_status_packet(id, 4, &format!("Seek error: {}", e), ""));
                }
                
                let read_len = len.min(SFTP_READ_BUFFER_SIZE);
                let mut buffer = vec![0u8; read_len];
                
                match file.read(&mut buffer).await {
                    Ok(0) => {
                        Ok(self.build_status_packet(id, 1, "End of file", ""))
                    }
                    Ok(n) => {
                        buffer.truncate(n);
                        *read_bytes += n as u64;

                        tracing::debug!(
                            client_ip = %self.client_ip,
                            username = ?self.username.as_deref(),
                            action = "READ",
                            "Read {} bytes from {:?} at offset {}", n, path, offset
                        );

                        Ok(self.build_data_packet(id, &buffer))
                    }
                    Err(e) => {
                        tracing::error!("SFTP READ error for {:?}: {}", path, e);
                        Ok(self.build_status_packet(id, 4, &format!("Read error: {}", e), ""))
                    }
                }
            }
            _ => {
                tracing::warn!("SFTP READ: invalid handle '{}'", handle_str);
                Ok(self.build_status_packet(id, 4, "Invalid handle", ""))
            }
        }
    }

    async fn handle_write(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let (handle_str, handle_len) = self.parse_string_with_len(data, 5)?;
        let offset_pos = 5 + 4 + handle_len;
        let offset = self.parse_u64(data, offset_pos);
        let data_len = self.parse_u32(data, offset_pos + 8) as usize;
        
        if offset_pos + 12 + data_len > data.len() {
            tracing::error!("SFTP WRITE: invalid data length - offset_pos={}, data_len={}, packet_len={}", offset_pos, data_len, data.len());
            return Ok(self.build_status_packet(id, 4, "Invalid data length", ""));
        }
        let write_data = &data[offset_pos + 12..offset_pos + 12 + data_len];

        if !self.check_permission(|p| p.can_write) {
            tracing::warn!("SFTP WRITE denied: no write permission for user {:?}", self.username);
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let quota_mb = self.cached_permissions.as_ref().and_then(|p| p.quota_mb);
        
        if let Some(quota) = quota_mb {
            let current_usage = self.quota_manager.get_usage(self.username.as_deref().unwrap_or("anonymous")).await;
            let quota_bytes = quota * 1024 * 1024;
            if current_usage >= quota_bytes {
                tracing::warn!("SFTP WRITE denied: quota exceeded for user {:?}", self.username);
                return Ok(self.build_status_packet(id, 4, "Quota exceeded", ""));
            }
        }

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File { path, file, written_bytes, pending_flush_bytes, .. }) => {
                use tokio::io::{AsyncSeekExt, AsyncWriteExt};
                
                if let Err(e) = file.seek(std::io::SeekFrom::Start(offset)).await {
                    tracing::error!("SFTP WRITE seek error for {:?}: {}", path, e);
                    return Ok(self.build_status_packet(id, 4, &format!("Seek error: {}", e), ""));
                }
                
                if let Err(e) = file.write_all(write_data).await {
                    tracing::error!("SFTP WRITE error for {:?}: {}", path, e);
                    return Ok(self.build_status_packet(id, 4, &format!("Write error: {}", e), ""));
                }
                
                *written_bytes += data_len as u64;
                *pending_flush_bytes += data_len as u64;
                
                // 优化的批量刷新：只有当累积到阈值时才 flush
                if *pending_flush_bytes >= SFTP_WRITE_FLUSH_THRESHOLD as u64 {
                    if let Err(e) = file.flush().await {
                        tracing::error!("SFTP WRITE flush error for {:?}: {}", path, e);
                        return Ok(self.build_status_packet(id, 4, &format!("Flush error: {}", e), ""));
                    }
                    *pending_flush_bytes = 0;
                }
                
                tracing::debug!("SFTP WRITE: {} bytes to {:?} at offset {} (pending_flush: {})", 
                    data_len, path, offset, pending_flush_bytes);

                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            _ => {
                tracing::warn!("SFTP WRITE: invalid handle '{}'", handle_str);
                Ok(self.build_status_packet(id, 4, "Invalid handle", ""))
            }
        }
    }

    async fn handle_remove(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_delete) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("REMOVE failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        if tokio::fs::remove_file(&full_path).await.is_ok() {
            crate::file_op_log!(
                delete,
                self.username.as_deref().unwrap_or("anonymous"),
                &self.client_ip,
                &full_path.to_string_lossy(),
                "SFTP"
            );
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            Ok(self.build_status_packet(id, 4, "Failed to remove file", ""))
        }
    }

    async fn handle_mkdir(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_mkdir) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("MKDIR failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        if tokio::fs::create_dir_all(&full_path).await.is_ok() {
            crate::file_op_log!(
                mkdir,
                self.username.as_deref().unwrap_or("anonymous"),
                &self.client_ip,
                &full_path.to_string_lossy(),
                "SFTP"
            );
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            Ok(self.build_status_packet(id, 4, "Failed to create directory", ""))
        }
    }

    async fn handle_rmdir(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_rmdir) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("RMDIR failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        // ✅ Windows 上需要先检查是否是符号链接
        // 符号链接目录需要用 remove_dir 而不是 remove_dir_all
        let is_symlink = full_path.is_symlink();
        
        let result = if is_symlink {
            // 符号链接目录，使用 std::fs::remove_dir
            std::fs::remove_dir(&full_path)
        } else {
            // 普通目录，使用 tokio::fs::remove_dir_all
            tokio::fs::remove_dir_all(&full_path).await
        };

        if result.is_ok() {
            crate::file_op_log!(
                rmdir,
                self.username.as_deref().unwrap_or("anonymous"),
                &self.client_ip,
                &full_path.to_string_lossy(),
                "SFTP"
            );
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            Ok(self.build_status_packet(id, 4, "Failed to remove directory", ""))
        }
    }

    async fn handle_rename(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let (old_path, old_len) = self.parse_string_with_len(data, 5)?;
        let new_path_pos = 5 + 4 + old_len;
        let new_path = self.parse_string(data, new_path_pos)?;

        if !self.check_permission(|p| p.can_rename) {
            tracing::warn!("SFTP RENAME denied: no permission for user {:?}", self.username);
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let old_full = match self.resolve_path(&old_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("RENAME failed for old path '{}': {}", old_path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };
        let new_full = match self.resolve_path(&new_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("RENAME failed for new path '{}': {}", new_path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        tracing::debug!("SFTP RENAME: raw_old='{}', resolved_old='{}', raw_new='{}', resolved_new='{}'", 
            old_path, old_full.display(), new_path, new_full.display());

        if !old_full.exists() {
            tracing::warn!("SFTP RENAME failed: source does not exist - {}", old_full.display());
            return Ok(self.build_status_packet(id, 2, "No such file", ""));
        }

        if !path_starts_with_ignore_case(&old_full, &self.home_dir) || !path_starts_with_ignore_case(&new_full, &self.home_dir) {
            tracing::warn!("SFTP RENAME denied: path outside home - old={}, new={}", old_full.display(), new_full.display());
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        if old_full.is_symlink() {
            match tokio::fs::read_link(&old_full).await {
                Ok(link_target) => {
                    let resolved_target = if link_target.is_absolute() {
                        link_target
                    } else {
                        let parent = old_full.parent().unwrap_or(Path::new(&self.home_dir));
                        parent.join(&link_target)
                    };
                    
                    let canon_target = match resolved_target.canonicalize() {
                        Ok(c) => c,
                        Err(_) => {
                            tracing::warn!("SFTP RENAME denied: cannot resolve symlink target - {}", old_full.display());
                            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
                        }
                    };
                    
                    if !path_starts_with_ignore_case(&canon_target, &self.home_dir) {
                        tracing::warn!("SFTP RENAME denied: symlink points outside home - {} -> {}", old_full.display(), canon_target.display());
                        return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
                    }
                }
                Err(e) => {
                    tracing::warn!("SFTP RENAME failed: cannot read symlink - {}: {}", old_full.display(), e);
                    return Ok(self.build_status_packet(id, 4, "Failed to read symlink", ""));
                }
            }
        }

        if new_full.exists() && new_full.is_symlink() {
            match tokio::fs::read_link(&new_full).await {
                Ok(link_target) => {
                    let resolved_target = if link_target.is_absolute() {
                        link_target
                    } else {
                        let parent = new_full.parent().unwrap_or(Path::new(&self.home_dir));
                        parent.join(&link_target)
                    };
                    
                    let canon_target = match resolved_target.canonicalize() {
                        Ok(c) => c,
                        Err(_) => {
                            tracing::warn!("SFTP RENAME denied: cannot resolve destination symlink target - {}", new_full.display());
                            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
                        }
                    };
                    
                    if !path_starts_with_ignore_case(&canon_target, &self.home_dir) {
                        tracing::warn!("SFTP RENAME denied: destination symlink points outside home - {} -> {}", new_full.display(), canon_target.display());
                        return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
                    }
                }
                Err(e) => {
                    tracing::warn!("SFTP RENAME failed: cannot read destination symlink - {}: {}", new_full.display(), e);
                    return Ok(self.build_status_packet(id, 4, "Failed to read symlink", ""));
                }
            }
        }

        match tokio::fs::rename(&old_full, &new_full).await {
            Ok(()) => {
                // 判断是重命名还是移动：检查父目录是否相同
                let old_parent = old_full.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                let new_parent = new_full.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                if old_parent == new_parent {
                    crate::file_op_log!(
                        rename,
                        self.username.as_deref().unwrap_or("anonymous"),
                        &self.client_ip,
                        &old_full.to_string_lossy(),
                        &new_full.to_string_lossy(),
                        "SFTP"
                    );
                } else {
                    crate::file_op_log!(
                        move,
                        self.username.as_deref().unwrap_or("anonymous"),
                        &self.client_ip,
                        &old_full.to_string_lossy(),
                        &new_full.to_string_lossy(),
                        "SFTP"
                    );
                }
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                tracing::error!("SFTP Rename failed: {} -> {}: {} (os error {:?})", old_full.display(), new_full.display(), e, e.raw_os_error());
                Ok(self.build_status_packet(id, 4, "Failed to rename", ""))
            }
        }
    }

    async fn handle_stat(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_read || p.can_list) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("STAT failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        match tokio::fs::metadata(&full_path).await {
            Ok(metadata) => {
                let mut payload = vec![105];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&self.build_attrs(metadata.is_dir(), metadata.len()));
                Ok(self.build_packet(&payload))
            }
            Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
        }
    }

    async fn handle_lstat(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        if !self.check_permission(|p| p.can_read || p.can_list) {
            let id = self.parse_u32(data, 1);
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }
        self.handle_stat(data).await
    }

    async fn handle_fstat(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let handle_str = self.parse_string(data, 5)?;

        let handle = self.handles.get(&handle_str);
        match handle {
            Some(SftpFileHandle::File { path, .. }) => {
                match tokio::fs::metadata(path).await {
                    Ok(metadata) => {
                        let mut payload = vec![105];
                        payload.extend_from_slice(&id.to_be_bytes());
                        payload.extend_from_slice(&self.build_attrs(metadata.is_dir(), metadata.len()));
                        Ok(self.build_packet(&payload))
                    }
                    Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
                }
            }
            _ => Ok(self.build_status_packet(id, 4, "Invalid handle", "")),
        }
    }

    async fn handle_setstat(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let (path, path_len) = self.parse_string_with_len(data, 5)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("SETSTAT failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        if !full_path.exists() {
            return Ok(self.build_status_packet(id, 2, "No such file", ""));
        }

        let attr_offset = 5 + 4 + path_len;
        if attr_offset >= data.len() {
            return Ok(self.build_status_packet(id, 0, "OK", ""));
        }

        match self.apply_file_attributes(&full_path, &data[attr_offset..]).await {
            Ok(()) => {
                tracing::debug!("SETSTAT applied to {:?}", full_path);
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                tracing::warn!("SETSTAT failed for {:?}: {}", full_path, e);
                Ok(self.build_status_packet(id, 4, &format!("Failed to set attributes: {}", e), ""))
            }
        }
    }

    async fn handle_fsetstat(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let (handle_str, handle_len) = self.parse_string_with_len(data, 5)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let path = match self.handles.get(&handle_str) {
            Some(SftpFileHandle::File { path, .. }) => path.clone(),
            Some(SftpFileHandle::Dir { path, .. }) => path.clone(),
            _ => return Ok(self.build_status_packet(id, 4, "Invalid handle", "")),
        };

        let attr_offset = 5 + 4 + handle_len;
        if attr_offset >= data.len() {
            return Ok(self.build_status_packet(id, 0, "OK", ""));
        }

        match self.apply_file_attributes(&path, &data[attr_offset..]).await {
            Ok(()) => {
                tracing::debug!("FSETSTAT applied to {:?}", path);
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                tracing::warn!("FSETSTAT failed for {:?}: {}", path, e);
                Ok(self.build_status_packet(id, 4, &format!("Failed to set attributes: {}", e), ""))
            }
        }
    }

    async fn apply_file_attributes(&self, path: &PathBuf, data: &[u8]) -> Result<()> {
        if data.len() < 4 {
            return Ok(());
        }

        let flags = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let mut offset = 4;

        if flags & 0x00000001 != 0 && offset + 8 <= data.len() {
            let size = u64::from_be_bytes([
                data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
            ]);
            offset += 8;

            let file = tokio::fs::OpenOptions::new().write(true).open(path).await?;
            file.set_len(size).await?;
            tracing::debug!("SETSTAT: set size to {} for {:?}", size, path);
        }

        if flags & 0x00000002 != 0 && offset + 4 <= data.len() {
            let _uid = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
            offset += 4;
            tracing::debug!("SETSTAT: uid change requested for {:?} (ignored on Windows)", path);
        }

        if flags & 0x00000004 != 0 && offset + 4 <= data.len() {
            let _gid = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
            offset += 4;
            tracing::debug!("SETSTAT: gid change requested for {:?} (ignored on Windows)", path);
        }

        if flags & 0x00000008 != 0 && offset + 4 <= data.len() {
            let permissions = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
            offset += 4;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = permissions & 0o777;
                let metadata = tokio::fs::metadata(path).await?;
                let mut perm = metadata.permissions();
                perm.set_mode(mode);
                tokio::fs::set_permissions(path, perm).await?;
                tracing::debug!("SETSTAT: set permissions to {:o} for {:?}", mode, path);
            }
            #[cfg(windows)]
            {
                tracing::debug!("SETSTAT: permissions change to {:o} for {:?} (ignored on Windows)", permissions, path);
            }
        }

        if flags & 0x00000010 != 0 && offset + 4 <= data.len() {
            let atime_sec = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as i64;
            offset += 4;

            if offset + 4 <= data.len() {
                let _atime_nsec = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
                offset += 4;
            }

            if flags & 0x00000020 != 0 && offset + 4 <= data.len() {
                let mtime_sec = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as i64;
                offset += 4;

                if offset + 4 <= data.len() {
                    let _mtime_nsec = u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
                }

                #[cfg(windows)]
                {
                    use std::time::{SystemTime, Duration};
                    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(mtime_sec as u64);
                    let atime = SystemTime::UNIX_EPOCH + Duration::from_secs(atime_sec as u64);
                    let file = tokio::fs::File::open(path).await?;
                    let std_file = file.into_std().await;
                    let _metadata = std_file.metadata()?;
                    let times = std::fs::FileTimes::new()
                        .set_modified(mtime)
                        .set_accessed(atime);
                    std_file.set_times(times)?;
                    tracing::debug!("SETSTAT: set mtime={:?}, atime={:?} for {:?}", mtime, atime, path);
                }
                #[cfg(not(windows))]
                {
                    use std::time::{SystemTime, Duration};
                    use std::os::unix::fs::FileTimesExt;
                    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(mtime_sec as u64);
                    let atime = SystemTime::UNIX_EPOCH + Duration::from_secs(atime_sec as u64);
                    let file = tokio::fs::File::open(path).await?;
                    let std_file = file.into_std().await;
                    let times = std::fs::FileTimes::new()
                        .set_modified(mtime)
                        .set_accessed(atime);
                    std_file.set_times(times)?;
                    tracing::debug!("SETSTAT: set mtime={:?}, atime={:?} for {:?}", mtime, atime, path);
                }
            }
        }

        Ok(())
    }

    async fn handle_realpath(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        let full_path = if path.is_empty() || path == "." {
            Ok(PathBuf::from(&self.cwd))
        } else {
            self.resolve_path(&path)
        };

        let full_path = match full_path {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("REALPATH failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        let resolved = if full_path.exists() {
            full_path.canonicalize().unwrap_or(full_path)
        } else {
            full_path
        };

        let path_str = match to_ftp_path(&resolved, std::path::Path::new(&self.home_dir)) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("REALPATH failed: {}", e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        let mut payload = vec![104];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&1u32.to_be_bytes());
        payload.extend_from_slice(&(path_str.len() as u32).to_be_bytes());
        payload.extend_from_slice(path_str.as_bytes());
        let longname = format!("drwxr-xr-x  1 user user  0 Jan 01 00:00 {}", path_str);
        payload.extend_from_slice(&(longname.len() as u32).to_be_bytes());
        payload.extend_from_slice(longname.as_bytes());
        payload.extend_from_slice(&self.build_attrs(true, 0));

        Ok(self.build_packet(&payload))
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf, PathResolveError> {
        safe_resolve_path_with_cwd(&self.cwd, &self.home_dir, path)
    }

    fn generate_handle(&mut self) -> String {
        let handle = format!("h{:08x}", self.next_handle_id);
        self.next_handle_id = self.next_handle_id.wrapping_add(1);
        handle
    }

    fn parse_u32(&self, data: &[u8], offset: usize) -> u32 {
        if offset + 4 > data.len() {
            return 0;
        }
        u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
    }

    fn parse_u64(&self, data: &[u8], offset: usize) -> u64 {
        if offset + 8 > data.len() {
            return 0;
        }
        u64::from_be_bytes([
            data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
            data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7],
        ])
    }

    fn parse_string(&self, data: &[u8], offset: usize) -> Result<String> {
        if offset + 4 > data.len() {
            return Ok(String::new());
        }
        let len = self.parse_u32(data, offset) as usize;
        if offset + 4 + len > data.len() {
            return Ok(String::new());
        }
        Ok(String::from_utf8_lossy(&data[offset + 4..offset + 4 + len]).to_string())
    }

    fn parse_string_with_len(&self, data: &[u8], offset: usize) -> Result<(String, usize)> {
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

    fn build_packet(&self, payload: &[u8]) -> Vec<u8> {
        let mut packet = Vec::new();
        packet.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        packet.extend_from_slice(payload);
        packet
    }

    fn build_status_packet(&self, id: u32, status: u32, msg: &str, lang: &str) -> Vec<u8> {
        let mut payload = vec![101];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&status.to_be_bytes());
        payload.extend_from_slice(&(msg.len() as u32).to_be_bytes());
        payload.extend_from_slice(msg.as_bytes());
        payload.extend_from_slice(&(lang.len() as u32).to_be_bytes());
        payload.extend_from_slice(lang.as_bytes());
        self.build_packet(&payload)
    }

    fn build_handle_packet(&self, id: u32, handle: &str) -> Vec<u8> {
        let mut payload = vec![102];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&(handle.len() as u32).to_be_bytes());
        payload.extend_from_slice(handle.as_bytes());
        self.build_packet(&payload)
    }

    fn build_data_packet(&self, id: u32, data: &[u8]) -> Vec<u8> {
        let mut payload = vec![103];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&(data.len() as u32).to_be_bytes());
        payload.extend_from_slice(data);
        self.build_packet(&payload)
    }

    fn build_attrs(&self, is_dir: bool, size: u64) -> Vec<u8> {
        let mut attrs = Vec::new();
        // SSH_FILEXFER_ATTR_SIZE | SSH_FILEXFER_ATTR_UIDGID | SSH_FILEXFER_ATTR_PERMISSIONS | SSH_FILEXFER_ATTR_ACMODTIME
        let flags: u32 = 0x00000001 | 0x00000002 | 0x00000004 | 0x00000008;
        attrs.extend_from_slice(&flags.to_be_bytes());
        attrs.extend_from_slice(&size.to_be_bytes());
        let uid: u32 = 1000;
        let gid: u32 = 1000;
        attrs.extend_from_slice(&uid.to_be_bytes());
        attrs.extend_from_slice(&gid.to_be_bytes());
        // SFTP permissions 格式：高 16 位包含文件类型 (S_IFDIR = 0o40000, S_IFREG = 0o10000)
        let permissions = if is_dir {
            0o40755u32  // S_IFDIR | 0755
        } else {
            0o100644u32  // S_IFREG | 0644
        };
        attrs.extend_from_slice(&permissions.to_be_bytes());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        attrs.extend_from_slice(&now.to_be_bytes());
        attrs.extend_from_slice(&now.to_be_bytes());
        attrs
    }

    async fn handle_open(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let (path, path_len) = self.parse_string_with_len(data, 5)?;
        let pflags_pos = 5 + 4 + path_len;
        let pflags = self.parse_u32(data, pflags_pos);

        let need_read = pflags & SSH_FXF_READ != 0;
        let need_write = pflags & SSH_FXF_WRITE != 0;
        let need_append = pflags & SSH_FXF_APPEND != 0;
        let need_creat = pflags & SSH_FXF_CREAT != 0;
        let need_trunc = pflags & SSH_FXF_TRUNC != 0;
        let need_excl = pflags & SSH_FXF_EXCL != 0;

        if !self.check_permission(|p| {
            (!need_read || p.can_read) &&
            (!need_write || p.can_write) &&
            (!need_append || p.can_append)
        }) {
            tracing::warn!("SFTP OPEN denied: no permission for user {:?} (read={}, write={}, append={})", 
                self.username, need_read, need_write, need_append);
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("OPEN failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };
        let file_existed = full_path.exists();

        tracing::debug!("SFTP OPEN: raw='{}', resolved='{}', existed={}, flags=0x{:08X} (read={}, write={}, append={}, creat={}, trunc={}, excl={})", 
            path, full_path.display(), file_existed, pflags, need_read, need_write, need_append, need_creat, need_trunc, need_excl);

        let file_result = if need_write {
            if need_excl && need_creat && file_existed {
                return Ok(self.build_status_packet(id, 4, "File already exists", ""));
            }
            
            if need_append {
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(need_creat)
                    .append(true)
                    .open(&full_path).await
            } else if need_trunc {
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(need_creat)
                    .truncate(true)
                    .open(&full_path).await
            } else if need_creat {
                tokio::fs::OpenOptions::new()
                    .read(need_read)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(&full_path).await
            } else {
                tokio::fs::OpenOptions::new()
                    .read(need_read)
                    .write(true)
                    .open(&full_path).await
            }
        } else {
            tokio::fs::File::open(&full_path).await
        };

        match file_result {
            Ok(file) => {
                let handle = self.generate_handle();
                self.handles.insert(handle.clone(), SftpFileHandle::File {
                    path: full_path,
                    file,
                    locked: false,
                    existed: file_existed,
                    written_bytes: 0,
                    read_bytes: 0,
                    pending_flush_bytes: 0,
                });
                tracing::debug!("SFTP OPEN: handle '{}' created for {}", handle, path);
                Ok(self.build_handle_packet(id, &handle))
            }
            Err(e) => {
                tracing::error!("SFTP OPEN failed for {}: {}", full_path.display(), e);
                Ok(self.build_status_packet(id, 4, "Failed to open file", ""))
            }
        }
    }

    async fn handle_readlink(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("READLINK failed for '{}': {}", path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        match tokio::fs::read_link(&full_path).await {
            Ok(target) => {
                let target_str = target.to_string_lossy().to_string();
                let mut payload = vec![104];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&1u32.to_be_bytes());
                payload.extend_from_slice(&(target_str.len() as u32).to_be_bytes());
                payload.extend_from_slice(target_str.as_bytes());
                payload.extend_from_slice(&(target_str.len() as u32).to_be_bytes());
                payload.extend_from_slice(target_str.as_bytes());
                payload.extend_from_slice(&self.build_attrs(false, 0));
                Ok(self.build_packet(&payload))
            }
            Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
        }
    }

    async fn handle_symlink(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let (target, target_len) = self.parse_string_with_len(data, 5)?;
        let link_pos = 5 + 4 + target_len;
        let link_path = self.parse_string(data, link_pos)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_link = match self.resolve_path(&link_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("SYMLINK failed for link path '{}': {}", link_path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };
        let full_target = match self.resolve_path(&target) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("SYMLINK failed for target path '{}': {}", target, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        let home_path = std::path::Path::new(&self.home_dir);
        if !full_link.starts_with(home_path) {
            return Ok(self.build_status_packet(id, 3, "Permission denied: link path outside home", ""));
        }
        if !full_target.starts_with(home_path) {
            return Ok(self.build_status_packet(id, 3, "Permission denied: target path outside home", ""));
        }

        let symlink_result = std::os::windows::fs::symlink_file(&full_target, &full_link);
        
        if symlink_result.is_ok() {
            crate::file_op_log!(
                self.username.as_deref().unwrap_or("anonymous"),
                &self.client_ip,
                "SYMLINK",
                &format!("{} -> {}", full_link.to_string_lossy(), full_target.to_string_lossy()),
                0,
                "SFTP",
                true,
                "符号链接创建成功"
            );
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            Ok(self.build_status_packet(id, 4, "Failed to create symlink", ""))
        }
    }

    async fn handle_lock(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let handle_str = self.parse_string(data, 5)?;

        if self.sftp_version < 5 {
            return Ok(self.build_status_packet(id, 8, "Lock requires SFTP v5+", ""));
        }

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File { path, file, locked, .. }) => {
                if *locked {
                    return Ok(self.build_status_packet(id, 0, "Already locked", ""));
                }

                let std_file = file.try_clone().await?.into_std().await;
                match fs2::FileExt::lock_exclusive(&std_file) {
                    Ok(()) => {
                        *locked = true;
                        self.locked_files.insert(path.clone());
                        tracing::info!(
                            client_ip = %self.client_ip,
                            username = ?self.username.as_deref(),
                            action = "LOCK",
                            "Locked file: {:?}", path
                        );
                        Ok(self.build_status_packet(id, 0, "OK", ""))
                    }
                    Err(_) => Ok(self.build_status_packet(id, 4, "Failed to lock file", "")),
                }
            }
            _ => Ok(self.build_status_packet(id, 4, "Invalid handle", "")),
        }
    }

    async fn handle_unlock(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let handle_str = self.parse_string(data, 5)?;

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File { path, file, locked, .. }) => {
                if !*locked {
                    return Ok(self.build_status_packet(id, 0, "Not locked", ""));
                }

                let std_file = file.try_clone().await?.into_std().await;
                match fs2::FileExt::unlock(&std_file) {
                    Ok(()) => {
                        *locked = false;
                        self.locked_files.remove(path);
                        tracing::info!(
                            client_ip = %self.client_ip,
                            username = ?self.username.as_deref(),
                            action = "UNLOCK",
                            "Unlocked file: {:?}", path
                        );
                        Ok(self.build_status_packet(id, 0, "OK", ""))
                    }
                    Err(_) => Ok(self.build_status_packet(id, 4, "Failed to unlock file", "")),
                }
            }
            _ => Ok(self.build_status_packet(id, 4, "Invalid handle", "")),
        }
    }

    async fn handle_extended(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let ext_name = self.parse_string(data, 5)?;

        match ext_name.as_str() {
            "limits@openssh.com" => self.handle_limits(id).await,
            "statvfs@openssh.com" => self.handle_statvfs(id, data).await,
            "md5sum@openssh.com" | "md5-hash@openssh.com" => self.handle_md5sum(id, data).await,
            "sha256sum@openssh.com" | "sha256-hash@openssh.com" => self.handle_sha256sum(id, data).await,
            "copy-file" => self.handle_copy_file(id, data).await,
            "hardlink@openssh.com" => self.handle_hardlink(id, data).await,
            _ => {
                Ok(self.build_status_packet(id, 8, &format!("Unsupported extension: {}", ext_name), ""))
            }
        }
    }

    async fn handle_limits(&self, id: u32) -> Result<Vec<u8>> {
        let max_packet_size: u64 = 32768;
        let max_read_size: u64 = 32768;
        let max_write_size: u64 = 32768;
        let max_open_handles: u64 = 1000;
        let max_locks: u64 = 100;

        let mut payload = vec![201];
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&max_packet_size.to_be_bytes());
        payload.extend_from_slice(&max_read_size.to_be_bytes());
        payload.extend_from_slice(&max_write_size.to_be_bytes());
        payload.extend_from_slice(&max_open_handles.to_be_bytes());
        payload.extend_from_slice(&max_locks.to_be_bytes());
        Ok(self.build_packet(&payload))
    }

    async fn handle_statvfs(&self, id: u32, data: &[u8]) -> Result<Vec<u8>> {
        let (_ext_name, ext_len) = self.parse_string_with_len(data, 5)?;
        let path_offset = 5 + 4 + ext_len;
        let path = self.parse_string(data, path_offset)?;
        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(_) => {
                return Ok(self.build_status_packet(id, 2, "Invalid path", ""));
            }
        };

        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

            let wide_path: Vec<u16> = full_path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let mut free_bytes_available: u64 = 0;
            let mut total_bytes: u64 = 0;
            let mut total_free_bytes: u64 = 0;

            unsafe {
                if GetDiskFreeSpaceExW(
                    windows::core::PCWSTR(wide_path.as_ptr()),
                    Some(&mut free_bytes_available),
                    Some(&mut total_bytes),
                    Some(&mut total_free_bytes),
                ).is_err()
                {
                    return Ok(self.build_status_packet(id, 4, "Failed to get disk space info", ""));
                }
            }

            let block_size: u64 = 4096;
            let total_blocks = total_bytes / block_size;
            let free_blocks = total_free_bytes / block_size;
            let available_blocks = free_bytes_available / block_size;
            let total_inodes = total_blocks / 16;
            let free_inodes = free_blocks / 16;
            let avail_inodes = available_blocks / 16;
            let fsid: u64 = 0;
            let namemax: u64 = 255;

            let mut payload = vec![201];
            payload.extend_from_slice(&id.to_be_bytes());
            payload.extend_from_slice(&total_blocks.to_be_bytes());
            payload.extend_from_slice(&free_blocks.to_be_bytes());
            payload.extend_from_slice(&available_blocks.to_be_bytes());
            payload.extend_from_slice(&total_inodes.to_be_bytes());
            payload.extend_from_slice(&free_inodes.to_be_bytes());
            payload.extend_from_slice(&avail_inodes.to_be_bytes());
            payload.extend_from_slice(&block_size.to_be_bytes());
            payload.extend_from_slice(&fsid.to_be_bytes());
            payload.extend_from_slice(&namemax.to_be_bytes());

            tracing::debug!(
                "statvfs: path={:?}, total={}MB, free={}MB, available={}MB",
                full_path,
                total_bytes / 1024 / 1024,
                total_free_bytes / 1024 / 1024,
                free_bytes_available / 1024 / 1024
            );

            Ok(self.build_packet(&payload))
        }

        #[cfg(not(windows))]
        {
            use std::ffi::CString;
            use libc::statvfs;

            let path_cstr = match CString::new(full_path.to_string_lossy().as_bytes()) {
                Ok(s) => s,
                Err(_) => return Ok(self.build_status_packet(id, 4, "Invalid path encoding", "")),
            };

            let mut vfs: statvfs = unsafe { mem::zeroed() };

            unsafe {
                if statvfs(path_cstr.as_ptr(), &mut vfs) != 0 {
                    return Ok(self.build_status_packet(id, 4, "Failed to get filesystem info", ""));
                }
            }

            let total_blocks = vfs.f_blocks;
            let free_blocks = vfs.f_bfree;
            let available_blocks = vfs.f_bavail;
            let total_inodes = vfs.f_files;
            let free_inodes = vfs.f_ffree;
            let avail_inodes = vfs.f_favail;
            let block_size = vfs.f_bsize as u64;
            let fsid = vfs.f_fsid as u64;
            let namemax = vfs.f_namemax as u64;

            let mut payload = vec![201];
            payload.extend_from_slice(&id.to_be_bytes());
            payload.extend_from_slice(&total_blocks.to_be_bytes());
            payload.extend_from_slice(&free_blocks.to_be_bytes());
            payload.extend_from_slice(&available_blocks.to_be_bytes());
            payload.extend_from_slice(&total_inodes.to_be_bytes());
            payload.extend_from_slice(&free_inodes.to_be_bytes());
            payload.extend_from_slice(&avail_inodes.to_be_bytes());
            payload.extend_from_slice(&block_size.to_be_bytes());
            payload.extend_from_slice(&fsid.to_be_bytes());
            payload.extend_from_slice(&namemax.to_be_bytes());

            tracing::debug!(
                "statvfs: path={:?}, total={}MB, free={}MB, available={}MB",
                full_path,
                (total_blocks * block_size) / 1024 / 1024,
                (free_blocks * block_size) / 1024 / 1024,
                (available_blocks * block_size) / 1024 / 1024
            );

            Ok(self.build_packet(&payload))
        }
    }

    async fn handle_md5sum(&self, id: u32, data: &[u8]) -> Result<Vec<u8>> {
        let (_ext_name, ext_len) = self.parse_string_with_len(data, 5 + 4)?;
        let path_pos = 5 + 4 + ext_len;
        let path = self.parse_string(data, path_pos)?;
        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        if !self.check_permission(|p| p.can_read) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        match tokio::fs::File::open(&full_path).await {
            Ok(mut file) => {
                use md5::{Md5, Digest};
                use tokio::io::AsyncReadExt;
                let mut hasher = Md5::new();
                let mut buffer = [0u8; 8192];
                loop {
                    match file.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(n) => hasher.update(&buffer[..n]),
                        Err(_) => return Ok(self.build_status_packet(id, 4, "Read error", "")),
                    }
                }
                let hash = hasher.finalize();
                let hash_hex = hex::encode(hash);

                let mut payload = vec![201];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&(hash_hex.len() as u32).to_be_bytes());
                payload.extend_from_slice(hash_hex.as_bytes());
                Ok(self.build_packet(&payload))
            }
            Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
        }
    }

    async fn handle_sha256sum(&self, id: u32, data: &[u8]) -> Result<Vec<u8>> {
        let (_ext_name, ext_len) = self.parse_string_with_len(data, 5 + 4)?;
        let path_pos = 5 + 4 + ext_len;
        let path = self.parse_string(data, path_pos)?;
        let full_path = match self.resolve_path(&path) {
            Ok(p) => p,
            Err(e) => {
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        if !self.check_permission(|p| p.can_read) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        match tokio::fs::File::open(&full_path).await {
            Ok(mut file) => {
                use sha2::{Sha256, Digest};
                use tokio::io::AsyncReadExt;
                let mut hasher = Sha256::new();
                let mut buffer = [0u8; 8192];
                loop {
                    match file.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(n) => hasher.update(&buffer[..n]),
                        Err(_) => return Ok(self.build_status_packet(id, 4, "Read error", "")),
                    }
                }
                let hash = hasher.finalize();
                let hash_hex = hex::encode(hash);

                let mut payload = vec![201];
                payload.extend_from_slice(&id.to_be_bytes());
                payload.extend_from_slice(&(hash_hex.len() as u32).to_be_bytes());
                payload.extend_from_slice(hash_hex.as_bytes());
                Ok(self.build_packet(&payload))
            }
            Err(_) => Ok(self.build_status_packet(id, 2, "No such file", "")),
        }
    }

    async fn handle_copy_file(&mut self, id: u32, data: &[u8]) -> Result<Vec<u8>> {
        let (_ext_name, ext_len) = self.parse_string_with_len(data, 5)?;
        let src_pos = 5 + 4 + ext_len;
        let (src_path, src_len) = self.parse_string_with_len(data, src_pos)?;
        let dst_pos = src_pos + 4 + src_len;
        let dst_path = self.parse_string(data, dst_pos)?;

        if !self.check_permission(|p| p.can_read && p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let src_full = match self.resolve_path(&src_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("COPY failed for src path '{}': {}", src_path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };
        let dst_full = match self.resolve_path(&dst_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("COPY failed for dst path '{}': {}", dst_path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        match tokio::fs::copy(&src_full, &dst_full).await {
            Ok(size) => {
                crate::file_op_log!(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    "COPY",
                    &format!("{} -> {}", src_full.to_string_lossy(), dst_full.to_string_lossy()),
                    size,
                    "SFTP",
                    true,
                    "文件复制成功"
                );
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(_) => Ok(self.build_status_packet(id, 4, "Failed to copy file", "")),
        }
    }

    async fn handle_hardlink(&mut self, id: u32, data: &[u8]) -> Result<Vec<u8>> {
        let (src_path, src_len) = self.parse_string_with_len(data, 5 + 4)?;
        let dst_pos = 5 + 4 + 4 + src_len;
        let dst_path = self.parse_string(data, dst_pos)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        
        let src_full = match self.resolve_path(&src_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("HARDLINK failed for src path '{}': {}", src_path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };
        let dst_full = match self.resolve_path(&dst_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("HARDLINK failed for dst path '{}': {}", dst_path, e);
                return Ok(self.build_status_packet(id, 2, &e.to_string(), ""));
            }
        };

        match std::fs::hard_link(&src_full, &dst_full) {
            Ok(_) => {
                crate::file_op_log!(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    "HARDLINK",
                    &format!("{} -> {}", src_full.to_string_lossy(), dst_full.to_string_lossy()),
                    0,
                    "SFTP",
                    true,
                    "硬链接创建成功"
                );
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                tracing::error!(
                    client_ip = %self.client_ip,
                    username = ?self.username.as_deref(),
                    action = "HARDLINK_FAIL",
                    "Failed to create hardlink: {} -> {}: {}", src_path, dst_path, e
                );
                Ok(self.build_status_packet(id, 4, "Failed to create hardlink", ""))
            }
        }
    }
}
