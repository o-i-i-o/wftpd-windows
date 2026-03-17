use anyhow::Result;
use russh::*;
use russh::keys::*;
use russh::keys::ssh_key::rand_core::OsRng;
use russh::server::Msg;
use russh::MethodKind;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::collections::HashSet;
use tokio::sync::Mutex;

use crate::core::config::Config;
use crate::core::logger::Logger;
use crate::core::users::UserManager;
use crate::core::file_logger::{FileLogger, FileLogInfo};

fn get_program_data_path() -> PathBuf {
    let program_data = std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string());
    PathBuf::from(program_data).join("wftpg")
}

#[derive(Clone)]
pub struct SftpServer {
    config: Arc<StdMutex<Config>>,
    user_manager: Arc<StdMutex<UserManager>>,
    logger: Arc<StdMutex<Logger>>,
    file_logger: Arc<StdMutex<FileLogger>>,
    running: Arc<StdMutex<bool>>,
    shutdown_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl SftpServer {
    pub fn new(
        config: Arc<StdMutex<Config>>,
        user_manager: Arc<StdMutex<UserManager>>,
        logger: Arc<StdMutex<Logger>>,
        file_logger: Arc<StdMutex<FileLogger>>,
    ) -> Self {
        SftpServer {
            config,
            user_manager,
            logger,
            file_logger,
            running: Arc::new(StdMutex::new(false)),
            shutdown_tx: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        let (bind_ip, sftp_port, host_key_path) = {
            let cfg = match self.config.lock() {
                Ok(guard) => guard,
                Err(e) => return Err(anyhow::anyhow!("获取配置锁失败: {}", e)),
            };
            (
                cfg.sftp.bind_ip.clone(),
                cfg.server.sftp_port,
                cfg.sftp.host_key_path.clone(),
            )
        };

        let host_key = Self::load_or_generate_host_key(&host_key_path).await?;

        let mut methods = MethodSet::empty();
        methods.push(MethodKind::Password);
        methods.push(MethodKind::PublicKey);
        let config = russh::server::Config {
            keys: vec![host_key],
            methods,
            ..Default::default()
        };
        let config = Arc::new(config);

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        {
            let mut tx = self.shutdown_tx.lock().await;
            *tx = Some(shutdown_tx);
        }

        {
            let mut running = match self.running.lock() {
                Ok(guard) => guard,
                Err(e) => return Err(anyhow::anyhow!("获取运行状态锁失败: {}", e)),
            };
            *running = true;
        }

        let user_manager_clone = Arc::clone(&self.user_manager);
        let logger_clone = Arc::clone(&self.logger);
        let file_logger_clone = Arc::clone(&self.file_logger);
        let running_clone = Arc::clone(&self.running);

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

        if let Ok(mut logger) = self.logger.lock() {
            logger.info("SFTP", &format!("SFTP server started on {}", bind_addr));
        }

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((socket, peer_addr)) => {
                                let config = Arc::clone(&config);
                                let user_manager = Arc::clone(&user_manager_clone);
                                let logger = Arc::clone(&logger_clone);
                                let file_logger = Arc::clone(&file_logger_clone);
                                let client_ip = peer_addr.ip().to_string();

                                if let Ok(mut logger) = logger_clone.lock() {
                                    logger.client_action(
                                        "SFTP",
                                        &format!("Client connected from {}", client_ip),
                                        &client_ip,
                                        None,
                                        "CONNECT",
                                    );
                                }

                                tokio::spawn(async move {
                                    let handler = SftpHandler {
                                        user_manager,
                                        logger,
                                        file_logger,
                                        authenticated: false,
                                        username: None,
                                        home_dir: None,
                                        sftp_channel: None,
                                        sftp_state: None,
                                        client_ip: client_ip.clone(),
                                        users_path: get_program_data_path().join("users.json"),
                                    };

                                    if let Err(e) = russh::server::run_stream(config, socket, handler).await {
                                        eprintln!("SSH connection error from {}: {}", peer_addr, e);
                                    }
                                });
                            }
                            Err(e) => {
                                eprintln!("Failed to accept connection: {}", e);
                            }
                        }
                    }
                }
            }

            if let Ok(mut running) = running_clone.lock() {
                *running = false;
            }
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
            let mut running = match self.running.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    log::error!("获取运行状态锁失败: {}", e);
                    return;
                }
            };
            *running = false;
        }
        if let Ok(mut logger) = self.logger.lock() {
            logger.info("SFTP", "SFTP server stopped");
        }
    }

    pub fn is_running(&self) -> bool {
        match self.running.lock() {
            Ok(guard) => *guard,
            Err(_) => false,
        }
    }

    async fn load_or_generate_host_key(path: &str) -> Result<PrivateKey> {
        let path = PathBuf::from(path);

        if path.exists() {
            let key_data = tokio::fs::read_to_string(&path).await?;
            let key = PrivateKey::from_openssh(&key_data)?;
            return Ok(key);
        }

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut rng = OsRng;
        let key = PrivateKey::random(&mut rng, keys::Algorithm::Ed25519)?;

        let openssh = key.to_openssh(keys::ssh_key::LineEnding::default())?;
        tokio::fs::write(&path, openssh.to_string()).await?;

        let pub_path = path.with_extension("pub");
        let public_key = key.public_key();
        let pub_openssh = public_key.to_openssh()?;
        tokio::fs::write(&pub_path, pub_openssh.to_string()).await?;

        Ok(key)
    }
}

struct SftpHandler {
    user_manager: Arc<StdMutex<UserManager>>,
    logger: Arc<StdMutex<Logger>>,
    file_logger: Arc<StdMutex<FileLogger>>,
    authenticated: bool,
    username: Option<String>,
    home_dir: Option<String>,
    sftp_channel: Option<ChannelId>,
    sftp_state: Option<Arc<Mutex<SftpState>>>,
    client_ip: String,
    users_path: std::path::PathBuf,
}

struct SftpState {
    home_dir: String,
    cwd: String,
    username: Option<String>,
    user_manager: Arc<StdMutex<UserManager>>,
    logger: Arc<StdMutex<Logger>>,
    file_logger: Arc<StdMutex<FileLogger>>,
    handles: HashMap<String, SftpFileHandle>,
    next_handle_id: u32,
    sftp_version: u32,
    buffer: Vec<u8>,
    locked_files: HashSet<PathBuf>,
    client_ip: String,
}

enum SftpFileHandle {
    File {
        path: PathBuf,
        file: tokio::fs::File,
        locked: bool,
        existed: bool,
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
        let mut users = match self.user_manager.lock() {
            Ok(guard) => guard,
            Err(_) => return Ok(server::Auth::reject()),
        };
        
        if users.get_user(user).is_none() {
            let _ = users.reload(&self.users_path);
        }
        
        match users.authenticate(user, password) {
            Ok(true) => {
                self.authenticated = true;
                self.username = Some(user.to_string());
                
                if let Some(u) = users.get_user(user) {
                    self.home_dir = Some(u.home_dir.clone());
                }

                if let Ok(mut logger) = self.logger.lock() {
                    logger.client_action(
                        "SFTP",
                        &format!("User {} logged in", user),
                        &self.client_ip,
                        Some(user),
                        "LOGIN",
                    );
                }

                Ok(server::Auth::Accept)
            }
            Ok(false) => {
                if let Ok(mut logger) = self.logger.lock() {
                    logger.client_action(
                        "SFTP",
                        &format!("Failed login attempt for user {}", user),
                        &self.client_ip,
                        Some(user),
                        "AUTH_FAIL",
                    );
                }
                Ok(server::Auth::Reject { 
                    proceed_with_methods: None,
                    partial_success: false,
                })
            }
            Err(e) => {
                if let Ok(mut logger) = self.logger.lock() {
                    logger.client_action(
                        "SFTP",
                        &format!("Authentication error for user {}: {}", user, e),
                        &self.client_ip,
                        Some(user),
                        "AUTH_ERROR",
                    );
                }
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
            let users = match self.user_manager.lock() {
                Ok(guard) => guard,
                Err(_) => return Ok(server::Auth::reject()),
            };
            if let Some(u) = users.get_user(user) {
                (u.enabled, get_program_data_path().join(format!("keys/{}.pub", user)).to_string_lossy().to_string(), Some(u.home_dir.clone()))
            } else {
                (false, String::new(), None)
            }
        };
        
        if !enabled {
            if let Ok(mut logger) = self.logger.lock() {
                logger.client_action(
                    "SFTP",
                    &format!("Public key auth failed for user {}: user not found or disabled", user),
                    &self.client_ip,
                    Some(user),
                    "AUTH_FAIL",
                );
            }
            return Ok(server::Auth::Reject { 
                proceed_with_methods: None,
                partial_success: false,
            });
        }
        
        if let Ok(stored_key) = tokio::fs::read_to_string(&user_pubkey_path).await
            && let Ok(stored_pubkey) = keys::parse_public_key_base64(stored_key.trim())
                && public_key == &stored_pubkey {
                    self.authenticated = true;
                    self.username = Some(user.to_string());
                    self.home_dir = home_dir;

                    if let Ok(mut logger) = self.logger.lock() {
                        logger.client_action(
                            "SFTP",
                            &format!("User {} logged in via public key", user),
                            &self.client_ip,
                            Some(user),
                            "LOGIN",
                        );
                    }

                    return Ok(server::Auth::Accept);
                }

        if let Ok(mut logger) = self.logger.lock() {
            logger.client_action(
                "SFTP",
                &format!("Public key auth failed for user {}: key mismatch or not found", user),
                &self.client_ip,
                Some(user),
                "AUTH_FAIL",
            );
        }

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

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if name == "sftp" && self.authenticated {
            let _ = session.channel_success(channel);
            
            self.sftp_channel = Some(channel);
            
            let home_dir = self.home_dir.clone().unwrap_or_else(|| "/tmp".to_string());
            let username = self.username.clone();
            
            self.sftp_state = Some(Arc::new(Mutex::new(SftpState {
                home_dir: home_dir.clone(),
                cwd: home_dir.clone(),
                username,
                user_manager: Arc::clone(&self.user_manager),
                logger: Arc::clone(&self.logger),
                file_logger: Arc::clone(&self.file_logger),
                handles: HashMap::new(),
                next_handle_id: 0,
                sftp_version: 3,
                buffer: Vec::new(),
                locked_files: HashSet::new(),
                client_ip: self.client_ip.clone(),
            })));
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
                        let _ = handle.data(channel, CryptoVec::from_slice(&resp)).await;
                    }
                });
            }
        Ok(())
    }
}

impl SftpState {
    async fn process_sftp_data(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        self.buffer.extend_from_slice(data);

        let mut responses: Vec<u8> = Vec::new();

        while self.buffer.len() >= 4 {
            let packet_len = u32::from_be_bytes([
                self.buffer[0], self.buffer[1], self.buffer[2], self.buffer[3],
            ]) as usize;

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
        if let Ok(users) = self.user_manager.lock()
            && let Some(username) = &self.username
                && let Some(user) = users.get_user(username) {
                    return check_fn(&user.permissions);
                }
        false
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

        let mut payload = vec![2];
        payload.extend_from_slice(&self.sftp_version.to_be_bytes());
        Ok(self.build_packet(&payload))
    }

    async fn handle_opendir(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        let full_path = self.resolve_path(&path);

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

        if let Some(SftpFileHandle::File { path, locked, .. }) = self.handles.remove(&handle) {
            if locked {
                self.locked_files.remove(&path);
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
        let handle_str = self.parse_string(data, 5)?;
        let offset = self.parse_u64(data, 5 + 4 + handle_str.len());
        let len = self.parse_u32(data, 5 + 4 + handle_str.len() + 8) as usize;

        if !self.check_permission(|p| p.can_read) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File { path, file, .. }) => {
                use tokio::io::{AsyncSeekExt, AsyncReadExt};
                
                // Seek to the requested offset
                if let Err(e) = file.seek(std::io::SeekFrom::Start(offset)).await {
                    return Ok(self.build_status_packet(id, 4, &format!("Seek error: {}", e), ""));
                }
                
                // Read data (limit to 32KB as per SFTP spec recommendation)
                let read_len = len.min(32768);
                let mut buffer = vec![0u8; read_len];
                
                match file.read(&mut buffer).await {
                    Ok(0) => {
                        // EOF reached - return status 1 (EOF)
                        Ok(self.build_status_packet(id, 1, "End of file", ""))
                    }
                    Ok(n) => {
                        buffer.truncate(n);
                        
                        if let Ok(mut logger) = self.logger.lock() {
                            logger.client_action(
                                "SFTP",
                                &format!("Read {} bytes from {:?} at offset {}", n, path, offset),
                                &self.client_ip,
                                self.username.as_deref(),
                                "READ",
                            );
                        }

                        if let Ok(mut file_logger) = self.file_logger.lock() {
                            file_logger.log_download(
                                self.username.as_deref().unwrap_or("anonymous"),
                                &self.client_ip,
                                &path.to_string_lossy(),
                                n as u64,
                                "SFTP",
                            );
                        }

                        Ok(self.build_data_packet(id, &buffer))
                    }
                    Err(e) => {
                        Ok(self.build_status_packet(id, 4, &format!("Read error: {}", e), ""))
                    }
                }
            }
            _ => Ok(self.build_status_packet(id, 4, "Invalid handle", "")),
        }
    }

    async fn handle_write(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let handle_str = self.parse_string(data, 5)?;
        let offset_pos = 5 + 4 + handle_str.len();
        let offset = self.parse_u64(data, offset_pos);
        let data_len = self.parse_u32(data, offset_pos + 8) as usize;
        
        // Validate data length
        if offset_pos + 12 + data_len > data.len() {
            return Ok(self.build_status_packet(id, 4, "Invalid data length", ""));
        }
        let write_data = &data[offset_pos + 12..offset_pos + 12 + data_len];

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let handle = self.handles.get_mut(&handle_str);
        match handle {
            Some(SftpFileHandle::File { path, file, existed, .. }) => {
                use tokio::io::{AsyncSeekExt, AsyncWriteExt};
                
                // Seek to the requested offset
                if let Err(e) = file.seek(std::io::SeekFrom::Start(offset)).await {
                    return Ok(self.build_status_packet(id, 4, &format!("Seek error: {}", e), ""));
                }
                
                // Write data
                if let Err(e) = file.write_all(write_data).await {
                    return Ok(self.build_status_packet(id, 4, &format!("Write error: {}", e), ""));
                }
                
                // Flush to ensure data is persisted
                if let Err(e) = file.flush().await {
                    return Ok(self.build_status_packet(id, 4, &format!("Flush error: {}", e), ""));
                }

                if let Ok(mut logger) = self.logger.lock() {
                    logger.client_action(
                        "SFTP",
                        &format!("Wrote {} bytes to {:?} at offset {}", data_len, path, offset),
                        &self.client_ip,
                        self.username.as_deref(),
                        "WRITE",
                    );
                }

                if let Ok(mut file_logger) = self.file_logger.lock() {
                    if *existed {
                        file_logger.log_update(
                            self.username.as_deref().unwrap_or("anonymous"),
                            &self.client_ip,
                            &path.to_string_lossy(),
                            data_len as u64,
                            "SFTP",
                        );
                    } else {
                        file_logger.log_upload(
                            self.username.as_deref().unwrap_or("anonymous"),
                            &self.client_ip,
                            &path.to_string_lossy(),
                            data_len as u64,
                            "SFTP",
                        );
                    }
                }

                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            _ => Ok(self.build_status_packet(id, 4, "Invalid handle", "")),
        }
    }

    async fn handle_remove(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_delete) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = self.resolve_path(&path);

        if tokio::fs::remove_file(&full_path).await.is_ok() {
            if let Ok(mut file_logger) = self.file_logger.lock() {
                file_logger.log_delete(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    &full_path.to_string_lossy(),
                    "SFTP",
                );
            }
            if let Ok(mut logger) = self.logger.lock() {
                logger.client_action(
                    "SFTP",
                    &format!("Removed file: {}", path),
                    &self.client_ip,
                    self.username.as_deref(),
                    "DELETE",
                );
            }
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

        let full_path = self.resolve_path(&path);

        if tokio::fs::create_dir_all(&full_path).await.is_ok() {
            if let Ok(mut file_logger) = self.file_logger.lock() {
                file_logger.log_mkdir(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    &full_path.to_string_lossy(),
                    "SFTP",
                );
            }
            if let Ok(mut logger) = self.logger.lock() {
                logger.client_action(
                    "SFTP",
                    &format!("Created directory: {}", path),
                    &self.client_ip,
                    self.username.as_deref(),
                    "MKDIR",
                );
            }
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

        let full_path = self.resolve_path(&path);

        if tokio::fs::remove_dir_all(&full_path).await.is_ok() {
            if let Ok(mut file_logger) = self.file_logger.lock() {
                file_logger.log_rmdir(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    &full_path.to_string_lossy(),
                    "SFTP",
                );
            }
            if let Ok(mut logger) = self.logger.lock() {
                logger.client_action(
                    "SFTP",
                    &format!("Removed directory: {}", path),
                    &self.client_ip,
                    self.username.as_deref(),
                    "RMDIR",
                );
            }
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            Ok(self.build_status_packet(id, 4, "Failed to remove directory", ""))
        }
    }

    async fn handle_rename(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let old_path = self.parse_string(data, 5)?;
        let new_path_pos = 5 + 4 + old_path.len();
        let new_path = self.parse_string(data, new_path_pos)?;

        if !self.check_permission(|p| p.can_rename) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let old_full = self.resolve_path(&old_path);
        let new_full = self.resolve_path(&new_path);

        if tokio::fs::rename(&old_full, &new_full).await.is_ok() {
            if let Ok(mut file_logger) = self.file_logger.lock() {
                file_logger.log_rename(
                    self.username.as_deref().unwrap_or("anonymous"),
                    &self.client_ip,
                    &old_full.to_string_lossy(),
                    &new_full.to_string_lossy(),
                    "SFTP",
                );
            }
            if let Ok(mut logger) = self.logger.lock() {
                logger.client_action(
                    "SFTP",
                    &format!("Renamed: {} -> {}", old_path, new_path),
                    &self.client_ip,
                    self.username.as_deref(),
                    "RENAME",
                );
            }
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            Ok(self.build_status_packet(id, 4, "Failed to rename", ""))
        }
    }

    async fn handle_stat(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        let full_path = self.resolve_path(&path);

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
        let path = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = self.resolve_path(&path);

        // SETSTAT can set various attributes - we support basic ones
        // For now, just acknowledge success (actual implementation would parse attrs)
        if full_path.exists() {
            Ok(self.build_status_packet(id, 0, "OK", ""))
        } else {
            Ok(self.build_status_packet(id, 2, "No such file", ""))
        }
    }

    async fn handle_fsetstat(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let handle_str = self.parse_string(data, 5)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let handle = self.handles.get(&handle_str);
        match handle {
            Some(SftpFileHandle::File { .. }) => {
                // FSETSTAT can set various attributes - we support basic ones
                // For now, just acknowledge success
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            _ => Ok(self.build_status_packet(id, 4, "Invalid handle", "")),
        }
    }

    async fn handle_realpath(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        let full_path = if path.is_empty() || path == "." {
            PathBuf::from(&self.cwd)
        } else {
            self.resolve_path(&path)
        };

        let resolved = if full_path.exists() {
            full_path.canonicalize().unwrap_or(full_path)
        } else {
            full_path
        };

        let path_str = resolved.to_string_lossy().to_string();

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

    fn resolve_path(&self, path: &str) -> PathBuf {
        let home = PathBuf::from(&self.home_dir);
        let home_canon = if home.exists() {
            match home.canonicalize() {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("Failed to canonicalize home directory: {}", e);
                    return home;
                }
            }
        } else {
            log::warn!("Home directory does not exist: {:?}", home);
            return home;
        };
        
        let clean_path = path.trim();
        if clean_path.is_empty() || clean_path == "." || clean_path == "./" {
            return PathBuf::from(&self.cwd);
        }

        let resolved = if clean_path.starts_with('/') {
            home_canon.join(clean_path.trim_start_matches('/'))
        } else {
            PathBuf::from(&self.cwd).join(clean_path)
        };
        
        fn is_path_safe(resolved: &Path, home: &Path) -> bool {
            match resolved.canonicalize() {
                Ok(canon) => canon.starts_with(home),
                Err(_) => false,
            }
        }

        if resolved.exists() {
            if is_path_safe(&resolved, &home_canon) {
                match resolved.canonicalize() {
                    Ok(canon) => canon,
                    Err(_) => PathBuf::from(&self.cwd),
                }
            } else {
                log::warn!("Path escape attempt detected: {:?}", resolved);
                PathBuf::from(&self.cwd)
            }
        } else {
            let mut safe_path = PathBuf::from(&self.cwd);
            for component in resolved.components() {
                match component {
                    std::path::Component::Normal(name) => {
                        safe_path.push(name);
                    }
                    std::path::Component::ParentDir => {
                        safe_path.pop();
                    }
                    std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                        safe_path = PathBuf::from(&self.cwd);
                    }
                    std::path::Component::CurDir => {}
                }
            }
            
            if is_path_safe(&safe_path, &home_canon) {
                safe_path
            } else {
                log::warn!("Path escape attempt detected in non-existent path: {:?}", safe_path);
                PathBuf::from(&self.cwd)
            }
        }
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
        // SSH_FILEXFER_ATTR_SIZE (0x00000001) | SSH_FILEXFER_ATTR_UIDGID (0x00000002) 
        // | SSH_FILEXFER_ATTR_PERMISSIONS (0x00000004) | SSH_FILEXFER_ATTR_ACMODTIME (0x00000008)
        let flags: u32 = 0x00000001 | 0x00000002 | 0x00000004 | 0x00000008;
        attrs.extend_from_slice(&flags.to_be_bytes());
        attrs.extend_from_slice(&size.to_be_bytes());
        let uid: u32 = 1000;
        let gid: u32 = 1000;
        attrs.extend_from_slice(&uid.to_be_bytes());
        attrs.extend_from_slice(&gid.to_be_bytes());
        let permissions = if is_dir { 0o755u32 } else { 0o644u32 };
        attrs.extend_from_slice(&permissions.to_be_bytes());
        // atime and mtime (u32 in SFTP v3)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        attrs.extend_from_slice(&now.to_be_bytes()); // atime
        attrs.extend_from_slice(&now.to_be_bytes()); // mtime
        attrs
    }

    async fn handle_open(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;
        let pflags_pos = 5 + 4 + path.len();
        let pflags = self.parse_u32(data, pflags_pos);

        let need_read = pflags & 0x00000001 != 0;
        let need_write = pflags & 0x00000002 != 0;
        let need_append = pflags & 0x00000008 != 0;

        if !self.check_permission(|p| {
            (!need_read || p.can_read) &&
            (!need_write || p.can_write) &&
            (!need_append || p.can_append)
        }) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_path = self.resolve_path(&path);
        let file_existed = full_path.exists();

        let file_result = if pflags & 0x00000002 != 0 {
            if pflags & 0x00000010 != 0 {
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&full_path).await
            } else if pflags & 0x00000008 != 0 {
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .append(true)
                    .open(&full_path).await
            } else {
                tokio::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
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
                });
                Ok(self.build_handle_packet(id, &handle))
            }
            Err(_) => Ok(self.build_status_packet(id, 4, "Failed to open file", "")),
        }
    }

    async fn handle_readlink(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let id = self.parse_u32(data, 1);
        let path = self.parse_string(data, 5)?;

        let full_path = self.resolve_path(&path);

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
        let target = self.parse_string(data, 5)?;
        let link_pos = 5 + 4 + target.len();
        let link_path = self.parse_string(data, link_pos)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let full_link = self.resolve_path(&link_path);
        let full_target = self.resolve_path(&target);

        let symlink_result = std::os::windows::fs::symlink_file(&full_target, &full_link);
        
        if symlink_result.is_ok() {
            if let Ok(mut file_logger) = self.file_logger.lock() {
                file_logger.log(FileLogInfo {
                    username: self.username.as_deref().unwrap_or("anonymous"),
                    client_ip: &self.client_ip,
                    operation: "SYMLINK",
                    file_path: &format!("{} -> {}", full_link.to_string_lossy(), full_target.to_string_lossy()),
                    file_size: 0,
                    protocol: "SFTP",
                    success: true,
                    message: "符号链接创建成功",
                });
            }
            if let Ok(mut logger) = self.logger.lock() {
                logger.client_action(
                    "SFTP",
                    &format!("Created symlink: {} -> {}", link_path, target),
                    &self.client_ip,
                    self.username.as_deref(),
                    "SYMLINK",
                );
            }
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
                        if let Ok(mut logger) = self.logger.lock() {
                            logger.client_action(
                                "SFTP",
                                &format!("Locked file: {:?}", path),
                                &self.client_ip,
                                self.username.as_deref(),
                                "LOCK",
                            );
                        }
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
                        if let Ok(mut logger) = self.logger.lock() {
                            logger.client_action(
                                "SFTP",
                                &format!("Unlocked file: {:?}", path),
                                &self.client_ip,
                                self.username.as_deref(),
                                "UNLOCK",
                            );
                        }
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
        // ext_name starts at offset 5; skip its length prefix (4 bytes) + content
        let ext_name = self.parse_string(data, 5)?;
        let path_offset = 5 + 4 + ext_name.len();
        let path = self.parse_string(data, path_offset)?;
        let _full_path = self.resolve_path(&path);

        Ok(self.build_status_packet(id, 8, "statvfs not supported on this platform", ""))
    }

    async fn handle_md5sum(&self, id: u32, data: &[u8]) -> Result<Vec<u8>> {
        let ext_name = self.parse_string(data, 5)?;
        let path_pos = 5 + 4 + ext_name.len();
        let path = self.parse_string(data, path_pos)?;
        let full_path = self.resolve_path(&path);

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
        let path = self.parse_string(data, 5 + 4)?;
        let full_path = self.resolve_path(&path);

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
        let ext_name = self.parse_string(data, 5)?;
        let src_pos = 5 + 4 + ext_name.len();
        let src_path = self.parse_string(data, src_pos)?;
        let dst_pos = src_pos + 4 + src_path.len();
        let dst_path = self.parse_string(data, dst_pos)?;

        if !self.check_permission(|p| p.can_read && p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        let src_full = self.resolve_path(&src_path);
        let dst_full = self.resolve_path(&dst_path);

        match tokio::fs::copy(&src_full, &dst_full).await {
            Ok(size) => {
                if let Ok(mut file_logger) = self.file_logger.lock() {
                    file_logger.log(FileLogInfo {
                        username: self.username.as_deref().unwrap_or("anonymous"),
                        client_ip: &self.client_ip,
                        operation: "COPY",
                        file_path: &format!("{} -> {}", src_full.to_string_lossy(), dst_full.to_string_lossy()),
                        file_size: size,
                        protocol: "SFTP",
                        success: true,
                        message: "文件复制成功",
                    });
                }
                if let Ok(mut logger) = self.logger.lock() {
                    logger.client_action(
                        "SFTP",
                        &format!("Copied: {} -> {}", src_path, dst_path),
                        &self.client_ip,
                        self.username.as_deref(),
                        "COPY",
                    );
                }
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(_) => Ok(self.build_status_packet(id, 4, "Failed to copy file", "")),
        }
    }

    async fn handle_hardlink(&mut self, id: u32, data: &[u8]) -> Result<Vec<u8>> {
        let src_path = self.parse_string(data, 5 + 4)?;
        let dst_pos = 5 + 4 + 4 + src_path.len();
        let dst_path = self.parse_string(data, dst_pos)?;

        if !self.check_permission(|p| p.can_write) {
            return Ok(self.build_status_packet(id, 3, "Permission denied", ""));
        }

        
        let src_full = self.resolve_path(&src_path);
        let dst_full = self.resolve_path(&dst_path);

        match std::fs::hard_link(&src_full, &dst_full) {
            Ok(_) => {
                if let Ok(mut file_logger) = self.file_logger.lock() {
                    file_logger.log(FileLogInfo {
                        username: self.username.as_deref().unwrap_or("anonymous"),
                        client_ip: &self.client_ip,
                        operation: "HARDLINK",
                        file_path: &format!("{} -> {}", src_full.to_string_lossy(), dst_full.to_string_lossy()),
                        file_size: 0,
                        protocol: "SFTP",
                        success: true,
                        message: "硬链接创建成功",
                    });
                }
                if let Ok(mut logger) = self.logger.lock() {
                    logger.client_action(
                        "SFTP",
                        &format!("Created hardlink: {} -> {}", src_path, dst_path),
                        &self.client_ip,
                        self.username.as_deref(),
                        "HARDLINK",
                    );
                }
                Ok(self.build_status_packet(id, 0, "OK", ""))
            }
            Err(e) => {
                if let Ok(mut logger) = self.logger.lock() {
                    logger.client_action(
                        "SFTP",
                        &format!("Failed to create hardlink: {} -> {}: {}", src_path, dst_path, e),
                        &self.client_ip,
                self.username.as_deref(),
                "HARDLINK_FAIL",
                    );
                }
                Ok(self.build_status_packet(id, 4, "Failed to create hardlink", ""))
            }
        }
    }
}
