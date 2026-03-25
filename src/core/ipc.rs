use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write, BufReader, BufWriter};
use std::time::Duration;

use crate::core::windows_ipc::{IpcServerInner, IpcStream};

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
            message: "配置已重新加载".to_string(),
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
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    
    let mut buffer = vec![0u8; len];
    reader.read_exact(&mut buffer)?;
    
    Ok(buffer)
}

fn write_message<W: Write>(writer: &mut W, data: &[u8]) -> Result<()> {
    let len = data.len() as u32;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(data)?;
    writer.flush()?;
    Ok(())
}

pub struct IpcServer {
    inner: IpcServerInner,
}

impl IpcServer {
    pub fn new() -> Result<Self> {
        Ok(IpcServer {
            inner: IpcServerInner::new()?,
        })
    }
    
    pub fn accept(&self) -> Result<(IpcStream, ReloadCommand)> {
        let stream = self.inner.accept()?;
        
        let mut reader = BufReader::new(&stream);
        let buffer = read_message(&mut reader)?;
        
        let command: ReloadCommand = serde_json::from_slice(&buffer)?;
        
        Ok((stream, command))
    }
    
    pub fn accept_timeout(&self, timeout: Duration) -> Result<Option<(IpcStream, ReloadCommand)>> {
        match self.inner.accept_timeout(timeout)? {
            Some(stream) => {
                let mut reader = BufReader::new(&stream);
                let buffer = read_message(&mut reader)?;
                
                let command: ReloadCommand = serde_json::from_slice(&buffer)?;
                
                Ok(Some((stream, command)))
            }
            None => Ok(None),
        }
    }
    
    pub fn send_response(stream: &IpcStream, response: &ReloadResponse) -> Result<()> {
        let json = serde_json::to_vec(response)?;
        let mut writer = BufWriter::new(stream);
        write_message(&mut writer, &json)
    }
}

pub struct IpcClient;

impl IpcClient {
    fn send_command_internal(cmd: ReloadCommand) -> Result<ReloadResponse> {
        let stream = IpcStream::connect()?;
        
        let mut writer = BufWriter::new(&stream);
        let json = serde_json::to_vec(&cmd)?;
        write_message(&mut writer, &json)?;
        
        let mut reader = BufReader::new(&stream);
        let buffer = read_message(&mut reader)?;
        
        let response: ReloadResponse = serde_json::from_slice(&buffer)?;
        Ok(response)
    }
    
    pub fn notify_reload() -> Result<ReloadResponse> {
        Self::send_command_internal(ReloadCommand::reload())
    }
    
    pub fn is_server_running() -> bool {
        IpcStream::connect().is_ok()
    }
}
