//! SFTP SSH 处理器
//!
//! 实现 russh::server::Handler trait，处理 SSH 连接和 SFTP 会话

use parking_lot::Mutex;
use russh::keys::PublicKey;
use russh::server::Msg;
use russh::{server, Channel, ChannelId};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use crate::core::fail2ban::Fail2BanManager;
use crate::core::config::get_program_data_path;
use crate::core::quota::QuotaManager;
use crate::core::sftp_server::{SftpState, MAX_BUFFER_SIZE};
use crate::core::users::UserManager;

pub struct SftpHandler {
    pub user_manager: Arc<Mutex<UserManager>>,
    pub quota_manager: Arc<QuotaManager>,
    pub fail2ban_manager: Arc<Fail2BanManager>,
    pub authenticated: bool,
    pub username: Option<String>,
    pub home_dir: Option<String>,
    pub sftp_channel: Option<ChannelId>,
    pub sftp_state: Option<Arc<TokioMutex<SftpState>>>,
    pub client_ip: String,
    pub users_path: std::path::PathBuf,
}

impl SftpHandler {
    async fn process_sftp_data(
        state: Arc<TokioMutex<SftpState>>,
        data: Vec<u8>,
    ) -> Result<Vec<u8>, anyhow::Error> {
        let mut state = state.lock().await;

        if state.buffer.len() + data.len() > MAX_BUFFER_SIZE {
            tracing::warn!(
                "SFTP buffer overflow attempt: buffer={}, incoming={}, max={}",
                state.buffer.len(),
                data.len(),
                MAX_BUFFER_SIZE
            );
            state.buffer.clear();
            return Ok(state.build_status_packet(0, 4, "Buffer overflow", ""));
        }

        state.buffer.extend_from_slice(&data);

        let mut responses: Vec<u8> = Vec::new();

        while state.buffer.len() >= 4 {
            let packet_len = u32::from_be_bytes([
                state.buffer[0],
                state.buffer[1],
                state.buffer[2],
                state.buffer[3],
            ]) as usize;

            if packet_len > crate::core::sftp_server::MAX_PACKET_SIZE {
                tracing::warn!(
                    "SFTP packet too large: {} bytes (max {})",
                    packet_len,
                    crate::core::sftp_server::MAX_PACKET_SIZE
                );
                state.buffer.clear();
                return Ok(state.build_status_packet(0, 4, "Packet too large", ""));
            }

            if state.buffer.len() < 4 + packet_len {
                break;
            }

            let packet: Vec<u8> = state.buffer[4..4 + packet_len].to_vec();
            state.buffer.drain(0..4 + packet_len);

            if !packet.is_empty() {
                let response = state.handle_sftp_packet(&packet).await?;
                responses.extend_from_slice(&response);
            }
        }

        Ok(responses)
    }
}

impl russh::server::Handler for SftpHandler {
    type Error = anyhow::Error;

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        let (auth_result, home_dir_opt) = {
            let mut users = self.user_manager.lock();

            if users.get_user(user).is_none() {
                let _ = users.reload(&self.users_path);
            }

            let result = users.authenticate(user, password);
            let home = users.get_user(user).map(|u| u.home_dir.clone());
            (result, home)
        };

        match auth_result {
            Ok(true) => {
                if let Some(home_dir) = home_dir_opt {
                    match std::path::PathBuf::from(&home_dir).canonicalize() {
                        Ok(home_canon) => {
                            self.home_dir = Some(home_canon.to_string_lossy().to_string());
                        }
                        Err(e) => {
                            tracing::error!(
                                "SFTP auth failed: cannot canonicalize home directory '{}' for user '{}': {}",
                                home_dir,
                                user,
                                e
                            );
                            tracing::warn!(
                                client_ip = %self.client_ip,
                                username = %user,
                                action = "HOME_NOT_FOUND",
                                "Home directory not found for user {}: {}", user, home_dir
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

                self.fail2ban_manager.reset_failures(&self.client_ip).await;

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
                self.fail2ban_manager.add_failure(&self.client_ip).await;

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
                self.fail2ban_manager.add_failure(&self.client_ip).await;

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
                (
                    u.enabled,
                    get_program_data_path()
                        .join(format!("keys/{}.pub", user))
                        .to_string_lossy()
                        .to_string(),
                    Some(u.home_dir.clone()),
                )
            } else {
                (false, String::new(), None)
            }
        };

        if !enabled {
            self.fail2ban_manager.add_failure(&self.client_ip).await;

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
            && let Ok(stored_pubkey) = russh::keys::parse_public_key_base64(stored_key.trim())
            && public_key == &stored_pubkey
        {
            if let Some(ref hd) = home_dir {
                match std::path::PathBuf::from(hd).canonicalize() {
                    Ok(home_canon) => {
                        self.home_dir = Some(home_canon.to_string_lossy().to_string());
                    }
                    Err(e) => {
                        tracing::error!(
                            "SFTP pubkey auth failed: cannot canonicalize home directory '{}' for user '{}': {}",
                            hd,
                            user,
                            e
                        );
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

            self.fail2ban_manager.reset_failures(&self.client_ip).await;

            tracing::info!(
                client_ip = %self.client_ip,
                username = %user,
                action = "LOGIN",
                "User {} logged in via public key", user
            );

            return Ok(server::Auth::Accept);
        }

        self.fail2ban_manager.add_failure(&self.client_ip).await;

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

                let state = SftpState::new(
                    home_dir.clone(),
                    username,
                    Arc::clone(&self.user_manager),
                    Arc::clone(&self.quota_manager),
                    self.client_ip.clone(),
                );

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
            && let Some(state) = &self.sftp_state
        {
            let state_clone = Arc::clone(state);
            let handle = session.handle();
            let data_vec = data.to_vec();

            tokio::spawn(async move {
                let response = Self::process_sftp_data(state_clone, data_vec).await;

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
            && let Some(state) = &self.sftp_state
        {
            let mut state = state.lock().await;
            state.cleanup();
        }
        Ok(())
    }
}
