//! FTP 数据传输引擎
//!
//! 实现文件上传、下载和目录列表的底层数据传输

use anyhow::Result;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::passive::PassiveManager;
use super::tls::AsyncTlsTcpStream;
use crate::core::rate_limiter::RateLimiter;

const DEFAULT_BUFFER_SIZE: usize = 128 * 1024;
const MAX_ENTRIES_PER_BATCH: usize = 2000;
const MAX_LISTING_SIZE: usize = 4 * 1024 * 1024;

pub enum DataStream {
    Plain(TcpStream),
    Tls(Box<AsyncTlsTcpStream>),
}

impl DataStream {
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            DataStream::Plain(s) => s.read(buf).await,
            DataStream::Tls(s) => s.read(buf).await,
        }
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            DataStream::Plain(s) => s.write_all(buf).await,
            DataStream::Tls(s) => s.write_all(buf).await,
        }
    }
}

pub async fn get_data_connection(
    passive_mode: bool,
    data_port: Option<u16>,
    data_addr: &Option<String>,
    remote_ip: &str,
    passive_manager: &mut PassiveManager,
    data_protection: bool,
    tls_acceptor: Option<&tokio_rustls::TlsAcceptor>,
) -> Result<DataStream> {
    let port = match data_port {
        Some(p) => p,
        None => anyhow::bail!("No data port specified"),
    };

    tracing::debug!(
        "get_data_connection: passive_mode={}, port={}, remote_ip={}, data_protection={}",
        passive_mode,
        port,
        remote_ip,
        data_protection
    );

    let tcp_stream = if passive_mode {
        match passive_manager.accept_with_validation(port).await {
            Ok(stream) => stream,
            Err(e) => {
                tracing::error!("Failed to accept validated passive connection: {}", e);
                anyhow::bail!("Failed to accept passive connection: {}", e);
            }
        }
    } else if let Some(addr) = data_addr {
        tracing::debug!("Active mode: connecting to {}", addr);
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("Invalid address {}: {}", addr, e))?
            .next()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve address: {}", addr))?;
        tokio::time::timeout(Duration::from_secs(30), TcpStream::connect(&socket_addr))
            .await
            .map_err(|_| anyhow::anyhow!("Connection timeout"))?
            .map_err(|e| anyhow::anyhow!("Failed to connect: {}", e))?
    } else {
        let addr = format!("{}:{}", remote_ip, port);
        tracing::debug!("Active mode (fallback): connecting to {}", addr);
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("Invalid address {}: {}", addr, e))?
            .next()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve address: {}", addr))?;
        tokio::time::timeout(Duration::from_secs(30), TcpStream::connect(&socket_addr))
            .await
            .map_err(|_| anyhow::anyhow!("Connection timeout"))?
            .map_err(|e| anyhow::anyhow!("Failed to connect: {}", e))?
    };

    if data_protection {
        let acceptor = tls_acceptor
            .ok_or_else(|| anyhow::anyhow!("TLS acceptor not available for data connection"))?;
        match acceptor.accept(tcp_stream).await {
            Ok(tls_stream) => {
                tracing::debug!("Data connection upgraded to TLS");
                Ok(DataStream::Tls(Box::new(tls_stream)))
            }
            Err(e) => {
                tracing::error!("Data channel TLS handshake failed: {}", e);
                anyhow::bail!("Data channel TLS handshake failed: {}", e);
            }
        }
    } else {
        Ok(DataStream::Plain(tcp_stream))
    }
}

pub async fn receive_file(
    data_stream: &mut DataStream,
    file_path: &Path,
    offset: u64,
    abort: Arc<AtomicBool>,
    is_ascii: bool,
) -> Result<u64> {
    tracing::debug!(
        "receive_file: path={}, offset={}, is_ascii={}",
        file_path.display(),
        offset,
        is_ascii
    );

    let file_result = if offset > 0 {
        tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(file_path)
            .await
    } else {
        tokio::fs::File::create(file_path).await
    };

    let mut file = match file_result {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(
                "Failed to create/open file '{}': {}",
                file_path.display(),
                e
            );
            return Err(anyhow::anyhow!("Failed to create file: {}", e));
        }
    };

    if offset > 0
        && let Err(e) = file.seek(std::io::SeekFrom::Start(offset)).await
    {
        tracing::error!("Failed to seek in file '{}': {}", file_path.display(), e);
        return Err(anyhow::anyhow!("Seek failed: {}", e));
    }

    let mut buf = [0u8; DEFAULT_BUFFER_SIZE];
    let mut total_written: u64 = 0;
    let mut transfer_error: Option<anyhow::Error> = None;

    loop {
        if abort.load(Ordering::Relaxed) {
            tracing::debug!("Transfer aborted by client");
            break;
        }
        match data_stream.read(&mut buf).await {
            Ok(0) => {
                tracing::debug!(
                    "Data connection closed (EOF), total written: {}",
                    total_written
                );
                break;
            }
            Ok(n) => {
                let data = if is_ascii {
                    convert_crlf_to_lf(&buf[..n])
                } else {
                    buf[..n].to_vec()
                };
                if let Err(e) = file.write_all(&data).await {
                    tracing::error!("STOR write error for file '{}': {}", file_path.display(), e);
                    transfer_error = Some(anyhow::anyhow!("Write error: {}", e));
                    break;
                }
                total_written += data.len() as u64;
            }
            Err(e) => {
                tracing::error!("STOR read error from data stream: {}", e);
                transfer_error = Some(anyhow::anyhow!("Read error: {}", e));
                break;
            }
        }
    }

    if let Err(e) = file.sync_all().await {
        tracing::error!("Failed to sync file {:?}: {}", file_path, e);
    }

    if let Some(e) = transfer_error {
        tracing::error!(
            "STOR failed: {} bytes written before error to {}",
            total_written,
            file_path.display()
        );
        return Err(e);
    }

    tracing::debug!(
        "STOR completed: {} bytes written to {}",
        total_written,
        file_path.display()
    );
    Ok(total_written)
}

pub async fn receive_file_append(
    data_stream: &mut DataStream,
    file_path: &Path,
    abort: Arc<AtomicBool>,
    is_ascii: bool,
) -> Result<u64> {
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(file_path)
        .await?;

    let mut buf = [0u8; DEFAULT_BUFFER_SIZE];
    let mut total_written: u64 = 0;
    let mut transfer_error: Option<anyhow::Error> = None;

    loop {
        if abort.load(Ordering::Relaxed) {
            break;
        }
        match data_stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let data = if is_ascii {
                    convert_crlf_to_lf(&buf[..n])
                } else {
                    buf[..n].to_vec()
                };
                if let Err(e) = file.write_all(&data).await {
                    tracing::error!("APPE write error for file '{}': {}", file_path.display(), e);
                    transfer_error = Some(anyhow::anyhow!("Write error: {}", e));
                    break;
                }
                total_written += data.len() as u64;
            }
            Err(e) => {
                tracing::error!("APPE read error from data stream: {}", e);
                transfer_error = Some(anyhow::anyhow!("Read error: {}", e));
                break;
            }
        }
    }

    if let Err(e) = file.sync_all().await {
        tracing::error!("Failed to sync file {:?}: {}", file_path, e);
    }

    if let Some(e) = transfer_error {
        return Err(e);
    }

    Ok(total_written)
}

pub async fn receive_file_with_limits(
    data_stream: &mut DataStream,
    file_path: &Path,
    offset: u64,
    abort: Arc<AtomicBool>,
    is_ascii: bool,
    rate_limiter: Option<&RateLimiter>,
) -> Result<u64> {
    tracing::debug!(
        "receive_file_with_limits: path={}, offset={}, is_ascii={}",
        file_path.display(),
        offset,
        is_ascii
    );

    let file_result = if offset > 0 {
        tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(file_path)
            .await
    } else {
        tokio::fs::File::create(file_path).await
    };

    let mut file = match file_result {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(
                "Failed to create/open file '{}': {}",
                file_path.display(),
                e
            );
            return Err(anyhow::anyhow!("Failed to create file: {}", e));
        }
    };

    if offset > 0
        && let Err(e) = file.seek(std::io::SeekFrom::Start(offset)).await
    {
        tracing::error!("Failed to seek in file '{}': {}", file_path.display(), e);
        return Err(anyhow::anyhow!("Seek failed: {}", e));
    }

    let mut buf = [0u8; DEFAULT_BUFFER_SIZE];
    let mut total_written: u64 = 0;
    let mut transfer_error: Option<anyhow::Error> = None;

    loop {
        if abort.load(Ordering::Relaxed) {
            tracing::debug!("Transfer aborted by client");
            break;
        }
        match data_stream.read(&mut buf).await {
            Ok(0) => {
                tracing::debug!(
                    "Data connection closed (EOF), total written: {}",
                    total_written
                );
                break;
            }
            Ok(n) => {
                if let Some(limiter) = rate_limiter {
                    limiter.acquire(n).await;
                }

                let data = if is_ascii {
                    convert_crlf_to_lf(&buf[..n])
                } else {
                    buf[..n].to_vec()
                };
                if let Err(e) = file.write_all(&data).await {
                    tracing::error!("STOR write error for file '{}': {}", file_path.display(), e);
                    transfer_error = Some(anyhow::anyhow!("Write error: {}", e));
                    break;
                }
                total_written += data.len() as u64;
            }
            Err(e) => {
                tracing::error!("STOR read error from data stream: {}", e);
                transfer_error = Some(anyhow::anyhow!("Read error: {}", e));
                break;
            }
        }
    }

    if let Err(e) = file.sync_all().await {
        tracing::error!("Failed to sync file {:?}: {}", file_path, e);
    }

    if let Some(e) = transfer_error {
        tracing::error!(
            "STOR failed: {} bytes written before error to {}",
            total_written,
            file_path.display()
        );
        return Err(e);
    }

    tracing::debug!(
        "STOR completed: {} bytes written to {}",
        total_written,
        file_path.display()
    );
    Ok(total_written)
}

pub async fn send_file_with_limits(
    data_stream: &mut DataStream,
    file_path: &Path,
    offset: u64,
    abort: Arc<AtomicBool>,
    is_ascii: bool,
    rate_limiter: Option<&RateLimiter>,
) -> Result<()> {
    let mut file = tokio::fs::File::open(file_path).await?;

    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset)).await?;
    }

    let mut buf = [0u8; DEFAULT_BUFFER_SIZE];
    let mut transfer_error: Option<anyhow::Error> = None;

    loop {
        if abort.load(Ordering::Relaxed) {
            break;
        }
        match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                if let Some(limiter) = rate_limiter {
                    limiter.acquire(n).await;
                }

                let data = if is_ascii {
                    convert_lf_to_crlf(&buf[..n])
                } else {
                    buf[..n].to_vec()
                };
                if let Err(e) = data_stream.write_all(&data).await {
                    tracing::error!("RETR write error for file '{}': {}", file_path.display(), e);
                    transfer_error = Some(anyhow::anyhow!("Write error: {}", e));
                    break;
                }
            }
            Err(e) => {
                tracing::error!("RETR read error from file '{}': {}", file_path.display(), e);
                transfer_error = Some(anyhow::anyhow!("Read error: {}", e));
                break;
            }
        }
    }

    if let Some(e) = transfer_error {
        return Err(e);
    }

    Ok(())
}

pub async fn send_directory_listing(
    data_stream: &mut DataStream,
    dir_path: &Path,
    username: &str,
    is_nlst: bool,
    _is_ascii: bool,
) -> Result<()> {
    let mut entries_data = Vec::new();
    let mut entry_count = 0usize;

    let mut dir = tokio::fs::read_dir(dir_path).await?;

    while let Ok(Some(entry)) = dir.next_entry().await {
        if entry_count >= MAX_ENTRIES_PER_BATCH {
            tracing::warn!(
                "Directory listing truncated: {} entries limit reached for {:?}",
                MAX_ENTRIES_PER_BATCH,
                dir_path
            );
            break;
        }

        if let Ok(metadata) = entry.metadata().await {
            let name = entry.file_name().to_string_lossy().to_string();

            if is_nlst {
                let line = format!("{}\r\n", name);
                entries_data.extend_from_slice(line.as_bytes());
            } else {
                let is_dir = metadata.is_dir();
                let perms = if is_dir { "drwxr-xr-x" } else { "-rw-r--r--" };
                let size = metadata.len();
                let mtime = get_file_mtime(&metadata);
                let nlink = if is_dir { 2 } else { 1 };
                let line = format!(
                    "{} {:>2} {:<8} {:<8} {:>10} {} {}\r\n",
                    perms, nlink, username, username, size, mtime, name
                );
                entries_data.extend_from_slice(line.as_bytes());
            }

            entry_count += 1;

            if entries_data.len() >= MAX_LISTING_SIZE {
                data_stream.write_all(&entries_data).await?;
                entries_data.clear();
            }
        }
    }

    if !entries_data.is_empty() {
        data_stream.write_all(&entries_data).await?;
    }

    Ok(())
}

pub async fn send_mlsd_listing(
    data_stream: &mut DataStream,
    dir_path: &Path,
    owner: &str,
) -> Result<()> {
    let mut entries_data = Vec::new();
    let mut entry_count = 0usize;

    let mut dir = tokio::fs::read_dir(dir_path).await?;

    while let Ok(Some(entry)) = dir.next_entry().await {
        if entry_count >= MAX_ENTRIES_PER_BATCH {
            tracing::warn!(
                "MLSD listing truncated: {} entries limit reached for {:?}",
                MAX_ENTRIES_PER_BATCH,
                dir_path
            );
            break;
        }

        if let Ok(metadata) = entry.metadata().await {
            let name = entry.file_name().to_string_lossy().to_string();
            let facts = build_mlst_facts(&metadata, owner);
            let line = format!("{} {}\r\n", facts, name);
            entries_data.extend_from_slice(line.as_bytes());

            entry_count += 1;

            if entries_data.len() >= MAX_LISTING_SIZE {
                data_stream.write_all(&entries_data).await?;
                entries_data.clear();
            }
        }
    }

    if !entries_data.is_empty() {
        data_stream.write_all(&entries_data).await?;
    }

    Ok(())
}

pub fn get_file_mtime(metadata: &std::fs::Metadata) -> String {
    if let Ok(time) = metadata.modified() {
        let dt: chrono::DateTime<chrono::Local> = time.into();
        return dt.format("%Y-%m-%d %H:%M").to_string();
    }
    "1970-01-01 00:00".to_string()
}

pub fn get_file_mtime_raw(metadata: &std::fs::Metadata) -> String {
    use std::time::UNIX_EPOCH;
    if let Ok(time) = metadata.modified()
        && let Ok(_d) = time.duration_since(UNIX_EPOCH)
    {
        let dt: chrono::DateTime<chrono::Local> = time.into();
        return dt.format("%Y%m%d%H%M%S").to_string();
    }
    "19700101000000".to_string()
}

pub fn build_mlst_facts(metadata: &std::fs::Metadata, owner: &str) -> String {
    let mut facts: Vec<String> = Vec::new();

    if metadata.is_dir() {
        facts.push("type=dir".to_string());
    } else {
        facts.push("type=file".to_string());
    }

    facts.push(format!("size={}", metadata.len()));

    if let Ok(time) = metadata.modified() {
        let dt: chrono::DateTime<chrono::Utc> = time.into();
        facts.push(format!("modify={}", dt.format("%Y%m%d%H%M%S")));
    }

    facts.push(format!("UNIX.owner={}", owner));
    facts.push(format!("UNIX.group={}", owner));

    format!("{};", facts.join(";"))
}

fn convert_lf_to_crlf(data: &[u8]) -> Vec<u8> {
    let lf_count = data.iter().filter(|&&b| b == b'\n').count();
    let mut result = Vec::with_capacity(data.len() + lf_count);
    let mut prev_was_cr = false;
    for &byte in data {
        if byte == b'\n' {
            if !prev_was_cr {
                result.push(b'\r');
            }
            result.push(b'\n');
            prev_was_cr = false;
        } else {
            result.push(byte);
            prev_was_cr = byte == b'\r';
        }
    }
    result
}

fn convert_crlf_to_lf(data: &[u8]) -> Vec<u8> {
    let crlf_count = data
        .windows(2)
        .filter(|window| window[0] == b'\r' && window[1] == b'\n')
        .count();

    let mut result = Vec::with_capacity(data.len() - crlf_count);
    let mut iter = data.iter().peekable();
    while let Some(&byte) = iter.next() {
        if byte == b'\r' {
            if iter.peek() == Some(&&b'\n') {
                result.push(b'\n');
                iter.next();
            } else {
                result.push(b'\r');
            }
        } else {
            result.push(byte);
        }
    }
    result
}
