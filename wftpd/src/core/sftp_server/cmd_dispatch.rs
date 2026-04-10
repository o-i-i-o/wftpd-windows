//! SFTP command dispatcher
//!
//! Implements handle_sftp_packet main dispatcher and handle_init initialization command

use crate::core::sftp_server::SftpState;

impl SftpState {
    pub async fn handle_sftp_packet(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        if data.is_empty() {
            return Ok(self.build_status_packet(0, 5, "Bad packet", ""));
        }

        if self.last_handle_cleanup.elapsed() > std::time::Duration::from_secs(60) {
            self.cleanup_expired_handles().await;
            self.last_handle_cleanup = std::time::Instant::now();
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

    pub async fn handle_init(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let version = if data.len() >= 5 {
            u32::from_be_bytes([data[1], data[2], data[3], data[4]])
        } else {
            3
        };

        self.sftp_version = version.min(6);

        self.refresh_permissions();

        let mut payload = vec![2];
        payload.extend_from_slice(&self.sftp_version.to_be_bytes());

        let extensions: &[(&str, &str)] = &[
            ("limits@openssh.com", "1"),
            ("statvfs@openssh.com", "2"),
            ("md5-hash@openssh.com", "1"),
            ("sha256-hash@openssh.com", "2"),
            ("hardlink@openssh.com", "1"),
        ];

        for (name, data_val) in extensions {
            payload.extend_from_slice(&(name.len() as u32).to_be_bytes());
            payload.extend_from_slice(name.as_bytes());
            payload.extend_from_slice(&(data_val.len() as u32).to_be_bytes());
            payload.extend_from_slice(data_val.as_bytes());
        }

        Ok(self.build_packet(&payload))
    }
}
