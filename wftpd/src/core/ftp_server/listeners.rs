//! libunftp event listeners
//!
//! Implements DataListener and PresenceListener for logging
//! PresenceListener also handles connection count cleanup on session end

use async_trait::async_trait;
use libunftp::notification::{DataEvent, DataListener, EventMeta, PresenceEvent, PresenceListener};
use parking_lot::Mutex;
use std::sync::Arc;

use crate::core::config::Config;

use super::auth::SessionTracker;

#[derive(Debug)]
pub struct FtpDataListener;

impl FtpDataListener {
    pub fn new() -> Self {
        FtpDataListener
    }
}

#[async_trait]
impl DataListener for FtpDataListener {
    async fn receive_data_event(&self, event: DataEvent, meta: EventMeta) {
        match event {
            DataEvent::Put { path, bytes } => {
                tracing::info!(
                    target: "file_op",
                    username = %meta.username,
                    client_ip = %meta.trace_id,
                    operation = "UPLOAD",
                    file_path = %path,
                    file_size = bytes,
                    protocol = "FTP",
                    success = true,
                    "File uploaded successfully"
                );
            }
            DataEvent::Got { path, bytes } => {
                tracing::info!(
                    target: "file_op",
                    username = %meta.username,
                    client_ip = %meta.trace_id,
                    operation = "DOWNLOAD",
                    file_path = %path,
                    file_size = bytes,
                    protocol = "FTP",
                    success = true,
                    "File downloaded successfully"
                );
            }
            DataEvent::Deleted { path } => {
                tracing::info!(
                    target: "file_op",
                    username = %meta.username,
                    client_ip = %meta.trace_id,
                    operation = "DELETE",
                    file_path = %path,
                    file_size = 0u64,
                    protocol = "FTP",
                    success = true,
                    "File deleted successfully"
                );
            }
            DataEvent::MadeDir { path } => {
                tracing::info!(
                    target: "file_op",
                    username = %meta.username,
                    client_ip = %meta.trace_id,
                    operation = "MKDIR",
                    file_path = %path,
                    file_size = 0u64,
                    protocol = "FTP",
                    success = true,
                    "Directory created successfully"
                );
            }
            DataEvent::RemovedDir { path } => {
                tracing::info!(
                    target: "file_op",
                    username = %meta.username,
                    client_ip = %meta.trace_id,
                    operation = "RMDIR",
                    file_path = %path,
                    file_size = 0u64,
                    protocol = "FTP",
                    success = true,
                    "Directory deleted successfully"
                );
            }
            DataEvent::Renamed { from, to } => {
                tracing::info!(
                    target: "file_op",
                    username = %meta.username,
                    client_ip = %meta.trace_id,
                    operation = "RENAME",
                    file_path = %format!("{} -> {}", from, to),
                    file_size = 0u64,
                    protocol = "FTP",
                    success = true,
                    "File renamed successfully"
                );
            }
        }
    }
}

impl Default for FtpDataListener {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct FtpPresenceListener {
    session_tracker: Arc<SessionTracker>,
    config: Arc<Mutex<Config>>,
}

impl FtpPresenceListener {
    pub fn new(session_tracker: Arc<SessionTracker>, config: Arc<Mutex<Config>>) -> Self {
        FtpPresenceListener {
            session_tracker,
            config,
        }
    }
}

#[async_trait]
impl PresenceListener for FtpPresenceListener {
    async fn receive_presence_event(&self, event: PresenceEvent, meta: EventMeta) {
        match event {
            PresenceEvent::LoggedIn => {
                if meta.username != "unknown"
                    && let Some(client_ip) = self.session_tracker.get_ip_for_user(&meta.username)
                {
                    self.session_tracker.register_trace(
                        meta.trace_id.to_string(),
                        &meta.username,
                        client_ip,
                    );
                }
                tracing::info!(
                    username = %meta.username,
                    trace_id = %meta.trace_id,
                    action = "SESSION_START",
                    protocol = "FTP",
                    "User {} logged in (session {})", meta.username, meta.trace_id
                );
            }
            PresenceEvent::LoggedOut => {
                let client_ip = if meta.username != "unknown" {
                    self.session_tracker.unregister(&meta.username)
                } else {
                    self.session_tracker
                        .unregister_by_trace(&meta.trace_id.to_string())
                };

                if let Some(ip) = client_ip {
                    self.config.lock().unregister_connection(&ip);
                    tracing::debug!(
                        username = %meta.username,
                        trace_id = %meta.trace_id,
                        ip = %ip,
                        "Connection unregistered for user {} from {}", meta.username, ip
                    );
                }
                tracing::info!(
                    username = %meta.username,
                    trace_id = %meta.trace_id,
                    action = "SESSION_END",
                    protocol = "FTP",
                    "User {} logged out (session {})", meta.username, meta.trace_id
                );
            }
        }
    }
}
