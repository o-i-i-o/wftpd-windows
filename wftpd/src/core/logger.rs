//! Global logging system
//!
//! Based on tracing, supports log level control and log file rotation

use parking_lot::RwLock;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{Layer, filter, layer::SubscriberExt};

#[path = "log_types.rs"]
mod log_types;
pub use log_types::{LogBuffer, LogEntry, LogFields, LogLevel};

#[path = "log_layers.rs"]
mod log_layers;
use log_layers::{FileOpLogLayer, SystemLogLayer};

use std::sync::OnceLock;

static GLOBAL_LOGGER: OnceLock<GlobalLogger> = OnceLock::new();

struct GlobalLogger {
    buffer: LogBuffer<LogEntry>,
    file_op_buffer: LogBuffer<LogEntry>,
    _guard: WorkerGuard,
    _file_op_guard: WorkerGuard,
}

pub struct TracingLogger {
    buffer: LogBuffer<LogEntry>,
    file_op_buffer: LogBuffer<LogEntry>,
}

impl TracingLogger {
    pub fn init(log_dir: &str, max_files: usize, log_level: &str) -> Result<Self, String> {
        if let Some(global) = GLOBAL_LOGGER.get() {
            return Ok(TracingLogger {
                buffer: global.buffer.clone(),
                file_op_buffer: global.file_op_buffer.clone(),
            });
        }

        let path = PathBuf::from(log_dir);

        if let Err(e) = std::fs::create_dir_all(&path) {
            eprintln!(
                "Warning: Failed to create log directory {}: {}",
                path.display(),
                e
            );
        }

        let buffer = LogBuffer::new(1000);
        let file_op_buffer = LogBuffer::new(2000);

        let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .max_log_files(max_files)
            .filename_prefix("wftpg")
            .filename_suffix("log")
            .build(&path)
            .map_err(|e| format!("Failed to create log file: {}", e))?;

        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let file_op_appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .max_log_files(max_files)
            .filename_prefix("file-ops")
            .filename_suffix("log")
            .build(&path)
            .map_err(|e| format!("Failed to create file operation log file: {}", e))?;

        let (file_op_non_blocking, file_op_guard) =
            tracing_appender::non_blocking(file_op_appender);

        let level_filter: tracing::Level = log_level
            .to_lowercase()
            .parse()
            .unwrap_or(tracing::Level::INFO);

        let buffer_layer = SystemLogLayer::new(buffer.clone());
        let file_op_buffer_layer = FileOpLogLayer::new(file_op_buffer.clone());

        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_target(false)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
            .json()
            .with_filter(filter::filter_fn(|metadata| {
                !metadata.target().starts_with("file_op")
            }));

        let file_op_fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(file_op_non_blocking)
            .with_ansi(false)
            .with_target(false)
            .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
            .json()
            .with_filter(filter::filter_fn(|metadata| {
                metadata.target().starts_with("file_op")
            }));

        let console_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .with_target(true)
            .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
            .with_filter(filter::filter_fn(|metadata| {
                !metadata.target().starts_with("file_op")
            }));

        let subscriber = tracing_subscriber::registry()
            .with(tracing::level_filters::LevelFilter::from_level(
                level_filter,
            ))
            .with(buffer_layer)
            .with(file_op_buffer_layer)
            .with(fmt_layer)
            .with(file_op_fmt_layer)
            .with(console_layer);

        tracing::subscriber::set_global_default(subscriber)
            .map_err(|e| format!("Failed to set tracing logger: {}", e))?;

        if GLOBAL_LOGGER
            .set(GlobalLogger {
                buffer: buffer.clone(),
                file_op_buffer: file_op_buffer.clone(),
                _guard: guard,
                _file_op_guard: file_op_guard,
            })
            .is_err()
        {
            tracing::debug!("GlobalLogger already initialized, skipping set");
        }

        Ok(TracingLogger {
            buffer,
            file_op_buffer,
        })
    }

    pub fn get_recent_logs(&self, count: usize) -> Vec<LogEntry> {
        self.buffer.get_recent(count)
    }

    pub fn get_recent_file_ops(&self, count: usize) -> Vec<LogEntry> {
        self.file_op_buffer.get_recent(count)
    }

    pub fn buffer(&self) -> LogBuffer<LogEntry> {
        self.buffer.clone()
    }

    pub fn file_op_buffer(&self) -> LogBuffer<LogEntry> {
        self.file_op_buffer.clone()
    }
}

impl Clone for TracingLogger {
    fn clone(&self) -> Self {
        Self {
            buffer: self.buffer.clone(),
            file_op_buffer: self.file_op_buffer.clone(),
        }
    }
}

pub struct LogReader {
    log_dir: PathBuf,
    buffer: Arc<RwLock<VecDeque<LogEntry>>>,
    max_buffer_size: usize,
}

impl LogReader {
    pub fn new(log_dir: &str) -> Self {
        Self {
            log_dir: PathBuf::from(log_dir),
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            max_buffer_size: 1000,
        }
    }

    fn read_entries<F>(&self, count: usize, file_prefix: &str, filter: F) -> Vec<LogEntry>
    where
        F: Fn(&LogEntry) -> bool,
    {
        let mut logs = Vec::new();
        let log_files = match std::fs::read_dir(&self.log_dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with(file_prefix) && name.ends_with(".log"))
                })
                .collect::<Vec<_>>(),
            Err(_) => return logs,
        };

        for entry in log_files.iter().rev() {
            if logs.len() >= count {
                break;
            }

            if let Ok(file) = std::fs::File::open(entry.path()) {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(file);
                for line in reader.lines().map_while(Result::ok) {
                    if logs.len() >= count {
                        break;
                    }
                    if let Ok(log_entry) = serde_json::from_str::<LogEntry>(&line)
                        && filter(&log_entry)
                    {
                        logs.push(log_entry);
                    }
                }
            }
        }

        logs
    }

    pub fn read_logs(&self, count: usize) -> Vec<LogEntry> {
        let logs = self.read_entries(count, "wftpg-", |entry| entry.fields.operation.is_none());

        {
            let mut buffer = self.buffer.write();
            buffer.clear();
            for log in logs.iter().rev() {
                if buffer.len() >= self.max_buffer_size {
                    buffer.pop_front();
                }
                buffer.push_back(log.clone());
            }
        }

        logs
    }

    pub fn read_file_ops(&self, count: usize) -> Vec<LogEntry> {
        self.read_entries(count, "file-ops-", |entry| entry.fields.operation.is_some())
    }

    pub fn get_recent_logs(&self, count: usize) -> Vec<LogEntry> {
        let buffer = self.buffer.read();
        buffer.iter().rev().take(count).cloned().collect()
    }
}

impl Default for LogReader {
    fn default() -> Self {
        Self::new("C:\\ProgramData\\wftpg\\logs")
    }
}

#[macro_export]
macro_rules! file_op_log {
    (upload, $username:expr, $client_ip:expr, $file_path:expr, $file_size:expr, $protocol:expr) => {
        tracing::info!(
            target: "file_op",
            username = %$username,
            client_ip = %$client_ip,
            operation = "UPLOAD",
            file_path = %$file_path,
            file_size = $file_size,
            protocol = %$protocol,
            success = true,
            "File uploaded successfully"
        )
    };
    (update, $username:expr, $client_ip:expr, $file_path:expr, $file_size:expr, $protocol:expr) => {
        tracing::info!(
            target: "file_op",
            username = %$username,
            client_ip = %$client_ip,
            operation = "UPDATE",
            file_path = %$file_path,
            file_size = $file_size,
            protocol = %$protocol,
            success = true,
            "File update successful"
        )
    };
    (download, $username:expr, $client_ip:expr, $file_path:expr, $file_size:expr, $protocol:expr) => {
        tracing::info!(
            target: "file_op",
            username = %$username,
            client_ip = %$client_ip,
            operation = "DOWNLOAD",
            file_path = %$file_path,
            file_size = $file_size,
            protocol = %$protocol,
            success = true,
            "File downloaded successfully"
        )
    };
    (delete, $username:expr, $client_ip:expr, $file_path:expr, $protocol:expr) => {
        tracing::info!(
            target: "file_op",
            username = %$username,
            client_ip = %$client_ip,
            operation = "DELETE",
            file_path = %$file_path,
            file_size = 0u64,
            protocol = %$protocol,
            success = true,
            "File delete successful"
        )
    };
    (rename, $username:expr, $client_ip:expr, $old_path:expr, $new_path:expr, $protocol:expr) => {
        tracing::info!(
            target: "file_op",
            username = %$username,
            client_ip = %$client_ip,
            operation = "RENAME",
            file_path = %format!("{} -> {}", $old_path, $new_path),
            file_size = 0u64,
            protocol = %$protocol,
            success = true,
            "File renamed successfully"
        )
    };
    (move, $username:expr, $client_ip:expr, $old_path:expr, $new_path:expr, $protocol:expr) => {
        tracing::info!(
            target: "file_op",
            username = %$username,
            client_ip = %$client_ip,
            operation = "MOVE",
            file_path = %format!("{} -> {}", $old_path, $new_path),
            file_size = 0u64,
            protocol = %$protocol,
            success = true,
            "File move successful"
        )
    };
    (mkdir, $username:expr, $client_ip:expr, $dir_path:expr, $protocol:expr) => {
        tracing::info!(
            target: "file_op",
            username = %$username,
            client_ip = %$client_ip,
            operation = "MKDIR",
            file_path = %$dir_path,
            file_size = 0u64,
            protocol = %$protocol,
            success = true,
            "Directory created successfully"
        )
    };
    (rmdir, $username:expr, $client_ip:expr, $dir_path:expr, $protocol:expr) => {
        tracing::info!(
            target: "file_op",
            username = %$username,
            client_ip = %$client_ip,
            operation = "RMDIR",
            file_path = %$dir_path,
            file_size = 0u64,
            protocol = %$protocol,
            success = true,
            "Directory deleted successfully"
        )
    };
    (failed, $username:expr, $client_ip:expr, $operation:expr, $file_path:expr, $protocol:expr, $error:expr) => {
        tracing::error!(
            target: "file_op",
            username = %$username,
            client_ip = %$client_ip,
            operation = %$operation,
            file_path = %$file_path,
            file_size = 0u64,
            protocol = %$protocol,
            success = false,
            "Operation failed: {}",
            $error
        )
    };
    ($username:expr, $client_ip:expr, $operation:expr, $file_path:expr, $file_size:expr, $protocol:expr, $success:expr, $message:expr) => {
        tracing::info!(
            target: "file_op",
            username = %$username,
            client_ip = %$client_ip,
            operation = %$operation,
            file_path = %$file_path,
            file_size = $file_size,
            protocol = %$protocol,
            success = $success,
            "{}",
            $message
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    #[test]
    fn test_log_level_from_tracing() {
        assert_eq!(
            LogLevel::from_tracing_level(tracing::Level::DEBUG),
            LogLevel::Debug
        );
        assert_eq!(
            LogLevel::from_tracing_level(tracing::Level::INFO),
            LogLevel::Info
        );
        assert_eq!(
            LogLevel::from_tracing_level(tracing::Level::WARN),
            LogLevel::Warning
        );
        assert_eq!(
            LogLevel::from_tracing_level(tracing::Level::ERROR),
            LogLevel::Error
        );
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warning);
        assert!(LogLevel::Warning < LogLevel::Error);
    }

    #[test]
    fn test_log_buffer() {
        let buffer = LogBuffer::new(5);
        for i in 0..10 {
            buffer.push(LogEntry {
                timestamp: Local::now(),
                level: LogLevel::Info,
                fields: LogFields {
                    message: format!("message{}", i),
                    client_ip: None,
                    username: None,
                    action: None,
                    protocol: None,
                    operation: None,
                    file_path: None,
                    file_size: None,
                    success: None,
                },
            });
        }
        assert_eq!(buffer.len(), 5);
        let recent = buffer.get_recent(3);
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_file_op_buffer() {
        let buffer = LogBuffer::new(5);
        for i in 0..10 {
            buffer.push(LogEntry {
                timestamp: Local::now(),
                level: LogLevel::Info,
                fields: LogFields {
                    message: "test".to_string(),
                    client_ip: Some("127.0.0.1".to_string()),
                    username: Some(format!("user{}", i)),
                    action: None,
                    protocol: Some("FTP".to_string()),
                    operation: Some("UPLOAD".to_string()),
                    file_path: Some(format!("/path/{}", i)),
                    file_size: Some(100),
                    success: Some(true),
                },
            });
        }
        assert_eq!(buffer.get_recent(10).len(), 5);
    }

    #[test]
    fn test_log_entry_json_roundtrip() {
        let entry = LogEntry {
            timestamp: Local::now(),
            level: LogLevel::Info,
            fields: LogFields {
                message: "Test message".to_string(),
                client_ip: Some("192.168.1.1".to_string()),
                username: Some("testuser".to_string()),
                action: Some("CONNECT".to_string()),
                protocol: Some("SFTP".to_string()),
                operation: None,
                file_path: None,
                file_size: None,
                success: None,
            },
        };

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: LogEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.level, LogLevel::Info);
        assert_eq!(parsed.fields.message, "Test message");
        assert_eq!(parsed.fields.client_ip, Some("192.168.1.1".to_string()));
    }

    #[test]
    fn test_file_op_entry_json_roundtrip() {
        let entry = LogEntry {
            timestamp: Local::now(),
            level: LogLevel::Info,
            fields: LogFields {
                message: "Upload successful".to_string(),
                client_ip: Some("192.168.1.1".to_string()),
                username: Some("user1".to_string()),
                action: None,
                protocol: Some("SFTP".to_string()),
                operation: Some("UPLOAD".to_string()),
                file_path: Some("/test/file.txt".to_string()),
                file_size: Some(1024),
                success: Some(true),
            },
        };

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: LogEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.level, LogLevel::Info);
        assert_eq!(parsed.fields.username, Some("user1".to_string()));
        assert_eq!(parsed.fields.operation, Some("UPLOAD".to_string()));
        assert_eq!(parsed.fields.file_size, Some(1024));
    }
}
