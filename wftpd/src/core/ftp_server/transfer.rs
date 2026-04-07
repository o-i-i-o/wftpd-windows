use anyhow::Result;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::passive::PassiveManager;
use crate::core::rate_limiter::RateLimiter;

// 动态缓冲区大小：根据传输场景自动调整
const DEFAULT_BUFFER_SIZE: usize = 128 * 1024;
const MAX_ENTRIES_PER_BATCH: usize = 2000;
const MAX_LISTING_SIZE: usize = 4 * 1024 * 1024; // 128KB 默认值

pub async fn get_data_connection(
    passive_mode: bool,
    data_port: Option<u16>,
    data_addr: &Option<String>,
    remote_ip: &str,
    passive_manager: &mut PassiveManager,
) -> Result<TcpStream> {
    let port = match data_port {
        Some(p) => p,
        None => anyhow::bail!("No data port specified"),
    };

    tracing::debug!(
        "get_data_connection: passive_mode={}, port={}, remote_ip={}",
        passive_mode,
        port,
        remote_ip
    );

    if passive_mode {
        match passive_manager.accept_with_validation(port).await {
            Ok(stream) => Ok(stream),
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
        let stream =
            tokio::time::timeout(Duration::from_secs(30), TcpStream::connect(&socket_addr))
                .await
                .map_err(|_| anyhow::anyhow!("Connection timeout"))?
                .map_err(|e| anyhow::anyhow!("Failed to connect: {}", e))?;
        Ok(stream)
    } else {
        let addr = format!("{}:{}", remote_ip, port);
        tracing::debug!("Active mode (fallback): connecting to {}", addr);
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("Invalid address {}: {}", addr, e))?
            .next()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve address: {}", addr))?;
        let stream =
            tokio::time::timeout(Duration::from_secs(30), TcpStream::connect(&socket_addr))
                .await
                .map_err(|_| anyhow::anyhow!("Connection timeout"))?
                .map_err(|e| anyhow::anyhow!("Failed to connect: {}", e))?;
        Ok(stream)
    }
}

pub async fn receive_file(
    data_stream: &mut TcpStream,
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
    data_stream: &mut TcpStream,
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
    data_stream: &mut TcpStream,
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
    data_stream: &mut TcpStream,
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

/// 优化的目录列表发送（批量缓冲 + 内存限制）
pub async fn send_directory_listing(
    data_stream: &mut TcpStream,
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

            // 分批发送，避免内存峰值
            if entries_data.len() >= MAX_LISTING_SIZE {
                data_stream.write_all(&entries_data).await?;
                entries_data.clear();
            }
        }
    }

    // 发送剩余数据
    if !entries_data.is_empty() {
        data_stream.write_all(&entries_data).await?;
    }

    Ok(())
}

pub async fn send_mlsd_listing(
    data_stream: &mut TcpStream,
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

/// 优化的 LF 到 CRLF 转换（上传时）
/// 使用预分配和迭代器优化
fn convert_lf_to_crlf(data: &[u8]) -> Vec<u8> {
    // 先扫描需要添加的\r数量，精确分配内存
    let lf_count = data.iter().filter(|&&b| b == b'\n').count();

    // 预分配足够空间（假设每个\n 前都需要添加\r）
    let mut result = Vec::with_capacity(data.len() + lf_count);

    // 使用迭代器处理，避免索引边界检查
    let mut prev_was_cr = false;
    for &byte in data {
        if byte == b'\n' {
            // 如果前一个不是\r，则添加\r
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

/// 优化的 CRLF 到 LF 转换（下载时）
/// 使用预分配和迭代器优化
fn convert_crlf_to_lf(data: &[u8]) -> Vec<u8> {
    // 先扫描需要移除的\r数量
    let crlf_count = data
        .windows(2)
        .filter(|window| window[0] == b'\r' && window[1] == b'\n')
        .count();

    // 预分配空间（移除所有 CRLF 中的\r）
    let mut result = Vec::with_capacity(data.len() - crlf_count);

    // 使用迭代器处理
    let mut iter = data.iter().peekable();
    while let Some(&byte) = iter.next() {
        if byte == b'\r' {
            // 如果是\r且下一个是\n，跳过\r直接添加\n
            if iter.peek() == Some(&&b'\n') {
                result.push(b'\n');
                iter.next(); // 消耗\n
            }
            // 否则忽略孤立的\r
        } else {
            result.push(byte);
        }
    }

    result
}
