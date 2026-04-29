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
                    username = %meta.username,
                    path = %path,
                    bytes = bytes,
                    action = "UPLOAD",
                    protocol = "FTP",
                    "User {} uploaded {} bytes to {}", meta.username, bytes, path
                );
            }
            DataEvent::Got { path, bytes } => {
                tracing::info!(
                    username = %meta.username,
                    path = %path,
                    bytes = bytes,
                    action = "DOWNLOAD",
                    protocol = "FTP",
                    "User {} downloaded {} bytes from {}", meta.username, bytes, path
                );
            }
            DataEvent::Deleted { path } => {
                tracing::info!(
                    username = %meta.username,
                    path = %path,
                    action = "DELETE",
                    protocol = "FTP",
                    "User {} deleted {}", meta.username, path
                );
            }
            DataEvent::MadeDir { path } => {
                tracing::info!(
                    username = %meta.username,
                    path = %path,
                    action = "MKDIR",
                    protocol = "FTP",
                    "User {} created directory {}", meta.username, path
                );
            }
            DataEvent::RemovedDir { path } => {
                tracing::info!(
                    username = %meta.username,
                    path = %path,
                    action = "RMDIR",
                    protocol = "FTP",
                    "User {} removed directory {}", meta.username, path
                );
            }
            DataEvent::Renamed { from, to } => {
                tracing::info!(
                    username = %meta.username,
                    from = %from,
                    to = %to,
                    action = "RENAME",
                    protocol = "FTP",
                    "User {} renamed {} to {}", meta.username, from, to
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
                tracing::info!(
                    username = %meta.username,
                    trace_id = %meta.trace_id,
                    action = "SESSION_START",
                    protocol = "FTP",
                    "User {} logged in (session {})", meta.username, meta.trace_id
                );
            }
            PresenceEvent::LoggedOut => {
                if let Some(client_ip) = self.session_tracker.unregister(&meta.username) {
                    self.config.unregister_connection(&client_ip);
                    tracing::debug!(
                        username = %meta.username,
                        ip = %client_ip,
                        "Connection unregistered for user {} from {}", meta.username, client_ip
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
