//! SFTP command dispatcher
//!
//! Dispatches incoming SFTP packets to appropriate handler functions

use crate::core::sftp_server::SftpState;

impl SftpState {
    pub async fn dispatch_packet(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        if data.is_empty() {
            tracing::warn!(
                client_ip = %self.client_ip,
                username = ?self.username,
                action = "EMPTY_PACKET",
                "SFTP: received empty packet"
            );
            return Ok(vec![]);
        }

        let cmd = data[0];

        match cmd {
            1 => self.handle_init(data).await,
            2 => self.handle_version(data).await,
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
            200 => self.handle_extended(data).await,
            _ => {
                tracing::debug!(
                    client_ip = %self.client_ip,
                    username = ?self.username,
                    action = "UNKNOWN_CMD",
                    cmd = cmd,
                    "SFTP: unknown command"
                );
                if data.len() >= 5 {
                    let id = self.parse_u32(data, 1)?;
                    Ok(self.build_status_packet(id, 8, "Unsupported command", ""))
                } else {
                    Ok(vec![])
                }
            }
        }
    }

    pub async fn handle_init(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        if data.len() < 5 {
            tracing::warn!(
                client_ip = %self.client_ip,
                username = ?self.username,
                action = "INIT_INVALID",
                "SFTP INIT: packet too short"
            );
            return Ok(self.build_version_packet(3));
        }

        let client_version = self.parse_u32(data, 1)?;

        tracing::info!(
            client_ip = %self.client_ip,
            username = ?self.username,
            action = "INIT",
            client_version = client_version,
            "SFTP INIT received"
        );

        if client_version < 3 {
            tracing::warn!(
                client_ip = %self.client_ip,
                username = ?self.username,
                action = "INIT_VERSION_TOO_LOW",
                client_version = client_version,
                "SFTP INIT: client version too low, minimum is 3"
            );
            return Ok(self.build_status_packet(
                0,
                8,
                "Protocol version too old, minimum is 3",
                "",
            ));
        }

        let server_version = client_version.min(6);

        tracing::info!(
            client_ip = %self.client_ip,
            username = ?self.username,
            action = "INIT_COMPLETE",
            client_version = client_version,
            server_version = server_version,
            "SFTP version negotiated"
        );

        Ok(self.build_version_packet(server_version))
    }

    pub async fn handle_version(&mut self, data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        if data.len() < 5 {
            return Ok(vec![]);
        }

        let version = self.parse_u32(data, 1)?;

        tracing::info!(
            client_ip = %self.client_ip,
            username = ?self.username,
            action = "VERSION",
            version = version,
            "SFTP VERSION received"
        );

        Ok(vec![])
    }
}
