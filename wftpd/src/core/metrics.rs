//! Prometheus 监控指标导出
//!
//! 提供 FTP/SFTP 服务器的连接数、传输速度等关键性能指标

use lazy_static::lazy_static;
use prometheus::{register_counter, register_gauge, register_histogram_vec, Counter, Gauge, HistogramVec};
use std::time::Instant;

lazy_static! {
    // 连接相关指标
    pub static ref CONNECTIONS_TOTAL: Gauge = register_gauge!(
        "wftpg_connections_total",
        "Total number of connections"
    ).unwrap();

    pub static ref CONNECTIONS_ACTIVE: Gauge = register_gauge!(
        "wftpg_connections_active",
        "Number of active connections"
    ).unwrap();

    pub static ref CONNECTIONS_REJECTED: Counter = register_counter!(
        "wftpg_connections_rejected_total",
        "Number of rejected connections (IP limits)"
    ).unwrap();

    // FTP 特定指标
    pub static ref FTP_COMMANDS_TOTAL: Counter = register_counter!(
        "wftpg_ftp_commands_total",
        "Total number of FTP commands processed"
    ).unwrap();

    pub static ref FTP_UPLOAD_BYTES: Counter = register_counter!(
        "wftpg_ftp_upload_bytes_total",
        "Total bytes uploaded via FTP"
    ).unwrap();

    pub static ref FTP_DOWNLOAD_BYTES: Counter = register_counter!(
        "wftpg_ftp_download_bytes_total",
        "Total bytes downloaded via FTP"
    ).unwrap();

    // SFTP 特定指标
    pub static ref SFTP_COMMANDS_TOTAL: Counter = register_counter!(
        "wftpg_sftp_commands_total",
        "Total number of SFTP commands processed"
    ).unwrap();

    pub static ref SFTP_UPLOAD_BYTES: Counter = register_counter!(
        "wftpg_sftp_upload_bytes_total",
        "Total bytes uploaded via SFTP"
    ).unwrap();

    pub static ref SFTP_DOWNLOAD_BYTES: Counter = register_counter!(
        "wftpg_sftp_download_bytes_total",
        "Total bytes downloaded via SFTP"
    ).unwrap();

    // 认证指标
    pub static ref AUTH_SUCCESS_TOTAL: Counter = register_counter!(
        "wftpg_auth_success_total",
        "Total successful authentications"
    ).unwrap();

    pub static ref AUTH_FAILURE_TOTAL: Counter = register_counter!(
        "wftpg_auth_failure_total",
        "Total failed authentications"
    ).unwrap();

    // 性能指标
    pub static ref COMMAND_DURATION: HistogramVec = register_histogram_vec!(
        "wftpg_command_duration_seconds",
        "Command processing duration in seconds",
        &["protocol", "command"],
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    ).unwrap();

    pub static ref TRANSFER_DURATION: HistogramVec = register_histogram_vec!(
        "wftpg_transfer_duration_seconds",
        "File transfer duration in seconds",
        &["protocol", "operation"],
        vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0]
    ).unwrap();

    // 错误指标
    pub static ref ERRORS_TOTAL: Counter = register_counter!(
        "wftpg_errors_total",
        "Total number of errors"
    ).unwrap();
}

/// 命令执行时间观察器
pub struct CommandTimer {
    protocol: &'static str,
    command: &'static str,
    start: Instant,
}

impl CommandTimer {
    pub fn new(protocol: &'static str, command: &'static str) -> Self {
        CommandTimer {
            protocol,
            command,
            start: Instant::now(),
        }
    }
}

impl Drop for CommandTimer {
    fn drop(&mut self) {
        let duration = self.start.elapsed().as_secs_f64();
        COMMAND_DURATION
            .with_label_values(&[self.protocol, self.command])
            .observe(duration);
    }
}

/// 文件传输时间观察器
pub struct TransferTimer {
    protocol: &'static str,
    operation: &'static str,
    start: Instant,
}

impl TransferTimer {
    pub fn new(protocol: &'static str, operation: &'static str) -> Self {
        TransferTimer {
            protocol,
            operation,
            start: Instant::now(),
        }
    }
}

impl Drop for TransferTimer {
    fn drop(&mut self) {
        let duration = self.start.elapsed().as_secs_f64();
        TRANSFER_DURATION
            .with_label_values(&[self.protocol, self.operation])
            .observe(duration);
    }
}

/// 记录认证成功
pub fn record_auth_success() {
    AUTH_SUCCESS_TOTAL.inc();
}

/// 记录认证失败
pub fn record_auth_failure() {
    AUTH_FAILURE_TOTAL.inc();
}

/// 记录连接被拒绝
pub fn record_connection_rejected() {
    CONNECTIONS_REJECTED.inc();
}

/// 记录上传字节数
pub fn record_upload_bytes(protocol: &str, bytes: u64) {
    match protocol {
        "FTP" => FTP_UPLOAD_BYTES.inc_by(bytes as f64),
        "SFTP" => SFTP_UPLOAD_BYTES.inc_by(bytes as f64),
        _ => {}
    }
}

/// 记录下载字节数
pub fn record_download_bytes(protocol: &str, bytes: u64) {
    match protocol {
        "FTP" => FTP_DOWNLOAD_BYTES.inc_by(bytes as f64),
        "SFTP" => SFTP_DOWNLOAD_BYTES.inc_by(bytes as f64),
        _ => {}
    }
}

/// 记录错误
pub fn record_error(_error_type: &str, _protocol: &str) {
    // 简化版本，不使用标签
    ERRORS_TOTAL.inc();
}

/// 获取所有指标的文本格式（用于 Prometheus 抓取）
pub fn gather_metrics() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        return format!("Error encoding metrics: {}", e);
    }
    
    String::from_utf8_lossy(&buffer).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_recording() {
        // 测试连接指标
        CONNECTIONS_ACTIVE.set(5.0);
        assert_eq!(CONNECTIONS_ACTIVE.get(), 5.0);
        
        // 测试认证指标
        let before = AUTH_SUCCESS_TOTAL.get();
        record_auth_success();
        assert_eq!(AUTH_SUCCESS_TOTAL.get(), before + 1.0);
        
        // 测试错误指标（ERRORS_TOTAL 不带标签）
        let before = ERRORS_TOTAL.get();
        record_error("test", "FTP");
        assert_eq!(ERRORS_TOTAL.get(), before + 1.0);
    }

    #[test]
    fn test_command_timer() {
        {
            let _timer = CommandTimer::new("FTP", "RETR");
            // 模拟一些工作
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // Timer automatically records on drop
    }
}
