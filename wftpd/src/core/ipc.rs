use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, BufWriter, Read, Write};
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

/// IPC 消息协议头（4 字节长度前缀）
fn read_message<R: Read>(reader: &mut R) -> Result<Vec<u8>> {
    let mut len_bytes = [0u8; 4];
    reader
        .read_exact(&mut len_bytes)
        .context("读取消息长度失败")?;
    let len = u32::from_be_bytes(len_bytes) as usize;

    // 限制最大消息大小，防止内存溢出
    if len > 10 * 1024 * 1024 {
        anyhow::bail!("消息过大：{} 字节", len);
    }

    let mut buffer = vec![0u8; len];
    reader.read_exact(&mut buffer).context("读取消息内容失败")?;

    Ok(buffer)
}

fn write_message<W: Write>(writer: &mut W, data: &[u8]) -> Result<()> {
    let len = data.len() as u32;
    writer
        .write_all(&len.to_be_bytes())
        .context("写入消息长度失败")?;
    writer.write_all(data).context("写入消息内容失败")?;
    writer.flush().context("刷新消息缓冲区失败")?;
    Ok(())
}

/// IPC 服务器端连接处理器
pub struct IpcConnection {
    stream: IpcStream,
}

impl IpcConnection {
    fn new(stream: IpcStream) -> Self {
        IpcConnection { stream }
    }

    /// 接收命令（带超时）
    pub fn receive_command(&mut self) -> Result<ReloadCommand> {
        let mut reader = BufReader::new(&self.stream);

        // 设置读取超时（通过底层实现）
        let buffer = read_message(&mut reader).context("接收 IPC 命令失败")?;

        let command: ReloadCommand =
            serde_json::from_slice(&buffer).context("解析 IPC 命令失败")?;

        Ok(command)
    }

    /// 发送响应
    pub fn send_response(&mut self, response: &ReloadResponse) -> Result<()> {
        let json = serde_json::to_vec(response).context("序列化响应失败")?;
        let mut writer = BufWriter::new(&self.stream);
        write_message(&mut writer, &json).context("发送 IPC 响应失败")?;
        Ok(())
    }
}

/// IPC 服务器
pub struct IpcServer {
    inner: IpcServerInner,
}

impl IpcServer {
    pub fn new() -> Result<Self> {
        Ok(IpcServer {
            inner: IpcServerInner::new().context("创建 IPC 服务器失败")?,
        })
    }

    /// 接受客户端连接（阻塞）
    pub fn accept(&self) -> Result<IpcConnection> {
        let stream = self.inner.accept().context("接受 IPC 连接失败")?;
        Ok(IpcConnection::new(stream))
    }

    /// 接受客户端连接（带超时）
    pub fn accept_timeout(&self, timeout: Duration) -> Result<Option<IpcConnection>> {
        match self.inner.accept_timeout(timeout)? {
            Some(stream) => Ok(Some(IpcConnection::new(stream))),
            None => Ok(None),
        }
    }
}

/// IPC 客户端
pub struct IpcClient;

impl IpcClient {
    /// 内部方法：发送命令并接收响应
    fn send_command_internal(cmd: ReloadCommand) -> Result<ReloadResponse> {
        let stream = IpcStream::connect().context("连接 IPC 服务器失败")?;

        let mut writer = BufWriter::new(&stream);
        let json = serde_json::to_vec(&cmd).context("序列化命令失败")?;
        write_message(&mut writer, &json).context("发送命令失败")?;

        let mut reader = BufReader::new(&stream);
        let buffer = read_message(&mut reader).context("读取响应失败")?;

        let response: ReloadResponse = serde_json::from_slice(&buffer).context("解析响应失败")?;
        Ok(response)
    }

    /// 通知后端重新加载配置
    pub fn notify_reload() -> Result<ReloadResponse> {
        Self::send_command_internal(ReloadCommand::reload()).context("通知重载配置失败")
    }

    /// 检查后端服务是否运行
    pub fn is_server_running() -> bool {
        IpcStream::connect().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reload_command_serialization() {
        let cmd = ReloadCommand::reload();
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("reload"));
    }

    #[test]
    fn test_reload_response_serialization() {
        let resp = ReloadResponse::ok();
        assert!(resp.success);
        assert!(resp.message.contains("重新加载"));
    }

    #[test]
    fn test_reload_response_error() {
        let resp = ReloadResponse::error("test error");
        assert!(!resp.success);
        assert_eq!(resp.message, "test error");
    }
}
