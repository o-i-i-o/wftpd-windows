use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, BufWriter, Read, Write};
use std::time::Duration;

use crate::core::windows_ipc::{IpcServerInner, IpcStream};

const IPC_TIMEOUT_SECS: u64 = 10;
const MAX_IPC_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadCommand {
    pub action: String,
}

impl ReloadCommand {
    pub fn reload() -> Self {
        ReloadCommand {
            action: "reload".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadResponse {
    pub success: bool,
    pub message: String,
}

impl ReloadResponse {
    pub fn ok() -> Self {
        ReloadResponse {
            success: true,
            message: "Configuration reloaded".to_string(),
        }
    }

    pub fn error(msg: &str) -> Self {
        ReloadResponse {
            success: false,
            message: msg.to_string(),
        }
    }
}

fn read_message<R: Read>(reader: &mut R) -> Result<Vec<u8>> {
    let mut len_bytes = [0u8; 4];
    reader
        .read_exact(&mut len_bytes)
        .context("Failed to read message length")?;
    let len = u32::from_be_bytes(len_bytes) as usize;

    if len > MAX_IPC_MESSAGE_SIZE {
        anyhow::bail!("Message too large: {} bytes", len);
    }

    let mut buffer = vec![0u8; len];
    reader
        .read_exact(&mut buffer)
        .context("Failed to read message content")?;

    Ok(buffer)
}

fn write_message<W: Write>(writer: &mut W, data: &[u8]) -> Result<()> {
    let len = data.len() as u32;
    writer
        .write_all(&len.to_be_bytes())
        .context("Failed to write message length")?;
    writer
        .write_all(data)
        .context("Failed to write message content")?;
    writer.flush().context("Failed to flush message buffer")?;
    Ok(())
}

pub struct IpcConnection {
    stream: IpcStream,
}

impl IpcConnection {
    fn new(stream: IpcStream) -> Self {
        IpcConnection { stream }
    }

    pub fn receive_command(&mut self) -> Result<ReloadCommand> {
        self.stream
            .set_read_timeout(Some(Duration::from_secs(IPC_TIMEOUT_SECS)))
            .context("Failed to set read timeout")?;

        let mut reader = BufReader::new(&self.stream);

        let buffer = read_message(&mut reader).context("Failed to receive IPC command")?;

        let command: ReloadCommand =
            serde_json::from_slice(&buffer).context("Failed to parse IPC command")?;

        Ok(command)
    }

    pub fn send_response(&mut self, response: &ReloadResponse) -> Result<()> {
        self.stream
            .set_write_timeout(Some(Duration::from_secs(IPC_TIMEOUT_SECS)))
            .context("Failed to set write timeout")?;

        let json = serde_json::to_vec(response).context("Failed to serialize response")?;
        let mut writer = BufWriter::new(&self.stream);
        write_message(&mut writer, &json).context("Failed to send IPC response")?;
        Ok(())
    }
}

pub struct IpcServer {
    inner: IpcServerInner,
}

impl IpcServer {
    pub fn new() -> Result<Self> {
        Ok(IpcServer {
            inner: IpcServerInner::new().context("Failed to create IPC server")?,
        })
    }

    pub fn accept(&self) -> Result<IpcConnection> {
        let stream = self
            .inner
            .accept()
            .context("Failed to accept IPC connection")?;
        Ok(IpcConnection::new(stream))
    }

    pub fn accept_timeout(&self, timeout: Duration) -> Result<Option<IpcConnection>> {
        match self.inner.accept_timeout(timeout)? {
            Some(stream) => Ok(Some(IpcConnection::new(stream))),
            None => Ok(None),
        }
    }
}

pub struct IpcClient;

impl IpcClient {
    fn send_command_internal(cmd: ReloadCommand) -> Result<ReloadResponse> {
        let stream = IpcStream::connect().context("Failed to connect to IPC server")?;

        stream
            .set_read_timeout(Some(Duration::from_secs(IPC_TIMEOUT_SECS)))
            .context("Failed to set read timeout")?;
        stream
            .set_write_timeout(Some(Duration::from_secs(IPC_TIMEOUT_SECS)))
            .context("Failed to set write timeout")?;

        let mut writer = BufWriter::new(&stream);
        let json = serde_json::to_vec(&cmd).context("Failed to serialize command")?;
        write_message(&mut writer, &json).context("Failed to send command")?;

        let mut reader = BufReader::new(&stream);
        let buffer = read_message(&mut reader).context("Failed to read response")?;

        let response: ReloadResponse =
            serde_json::from_slice(&buffer).context("Failed to parse response")?;
        Ok(response)
    }

    pub fn notify_reload() -> Result<ReloadResponse> {
        Self::send_command_internal(ReloadCommand::reload())
            .context("Failed to notify config reload")
    }

    pub fn is_server_running() -> bool {
        IpcStream::connect().is_ok()
    }
}
