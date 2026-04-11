//! Server manager
//!
//! Manages FTP and SFTP server lifecycle, supports start, stop and status query

use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::config::Config;
use crate::core::ftp_server::FtpServer;
use crate::core::sftp_server::SftpServer;
use crate::core::users::UserManager;

struct FtpState {
    server: Option<FtpServer>,
    runtime: Option<tokio::runtime::Runtime>,
}

struct SftpState {
    server: Option<SftpServer>,
    runtime: Option<tokio::runtime::Runtime>,
}

pub struct ServerManager {
    ftp_state: Arc<Mutex<FtpState>>,
    sftp_state: Arc<Mutex<SftpState>>,
    ftp_starting: Arc<AtomicBool>,
    sftp_starting: Arc<AtomicBool>,
}

impl ServerManager {
    pub fn new() -> Self {
        ServerManager {
            ftp_state: Arc::new(Mutex::new(FtpState {
                server: None,
                runtime: None,
            })),
            sftp_state: Arc::new(Mutex::new(SftpState {
                server: None,
                runtime: None,
            })),
            ftp_starting: Arc::new(AtomicBool::new(false)),
            sftp_starting: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start_ftp(
        &self,
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> anyhow::Result<()> {
        // Use CAS operation to prevent concurrent start race condition
        if self
            .ftp_starting
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_err()
        {
            tracing::debug!("FTP server is already starting or running");
            return Ok(());
        }

        {
            let state = self.ftp_state.lock();
            if state.server.is_some() {
                self.ftp_starting.store(false, Ordering::SeqCst);
                return Ok(());
            }
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;

        let server = FtpServer::new(config, user_manager);

        runtime.block_on(server.start())?;

        {
            let mut state = self.ftp_state.lock();
            state.server = Some(server);
            state.runtime = Some(runtime);
        }

        Ok(())
    }

    pub fn stop_ftp(&self) {
        self.ftp_starting.store(false, Ordering::SeqCst);

        let (maybe_server, maybe_runtime) = {
            let mut state = self.ftp_state.lock();
            (state.server.take(), state.runtime.take())
        };

        if let (Some(server), Some(runtime)) = (maybe_server, maybe_runtime) {
            runtime.block_on(server.stop());
            runtime.shutdown_background();
        }

        tracing::info!("FTP server stopped");
    }

    pub fn is_ftp_running(&self) -> bool {
        let state = self.ftp_state.lock();
        state.server.as_ref().is_some_and(|s| s.is_running())
    }

    pub fn start_sftp(
        &self,
        config: Arc<Mutex<Config>>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> anyhow::Result<()> {
        // Use CAS operation to prevent concurrent start race condition
        if self
            .sftp_starting
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
            .is_err()
        {
            tracing::debug!("SFTP server is already starting or running");
            return Ok(());
        }

        {
            let state = self.sftp_state.lock();
            if state.server.is_some() {
                self.sftp_starting.store(false, Ordering::SeqCst);
                return Ok(());
            }
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;

        let server = SftpServer::new(config, user_manager);

        runtime.block_on(server.start())?;

        {
            let mut state = self.sftp_state.lock();
            state.server = Some(server);
            state.runtime = Some(runtime);
        }

        Ok(())
    }

    pub fn stop_sftp(&self) {
        self.sftp_starting.store(false, Ordering::SeqCst);

        let (maybe_server, maybe_runtime) = {
            let mut state = self.sftp_state.lock();
            (state.server.take(), state.runtime.take())
        };

        if let (Some(server), Some(runtime)) = (maybe_server, maybe_runtime) {
            runtime.block_on(server.stop());
            runtime.shutdown_background();
        }

        tracing::info!("SFTP server stopped");
    }

    pub fn is_sftp_running(&self) -> bool {
        let state = self.sftp_state.lock();
        state.server.as_ref().is_some_and(|s| s.is_running())
    }
}

impl Default for ServerManager {
    fn default() -> Self {
        Self::new()
    }
}
