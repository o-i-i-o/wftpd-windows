//! libunftp event listeners
//!
//! Implements DataListener and PresenceListener for logging

use async_trait::async_trait;
use libunftp::notification::{DataEvent, DataListener, EventMeta, PresenceEvent, PresenceListener};

#[derive(Debug)]
pub struct QuotaDataListener;

impl QuotaDataListener {
    pub fn new() -> Self {
        QuotaDataListener
    }
}

#[async_trait]
impl DataListener for QuotaDataListener {
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

impl Default for QuotaDataListener {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct LoggingPresenceListener;

impl LoggingPresenceListener {
    pub fn new() -> Self {
        LoggingPresenceListener
    }
}

#[async_trait]
impl PresenceListener for LoggingPresenceListener {
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

impl Default for LoggingPresenceListener {
    fn default() -> Self {
        Self::new()
    }
}
