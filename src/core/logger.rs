use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;
use tracing::Level;
use tracing_subscriber::{Layer, layer::SubscriberExt, filter};
use tracing_appender::non_blocking::WorkerGuard;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
}

impl LogLevel {
    pub fn from_tracing_level(level: Level) -> Self {
        match level {
            Level::TRACE | Level::DEBUG => LogLevel::Debug,
            Level::INFO => LogLevel::Info,
            Level::WARN => LogLevel::Warning,
            Level::ERROR => LogLevel::Error,
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "TRACE" | "DEBUG" => Some(LogLevel::Debug),
            "INFO" => Some(LogLevel::Info),
            "WARN" | "WARNING" => Some(LogLevel::Warning),
            "ERROR" => Some(LogLevel::Error),
            _ => None,
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warning => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

impl Serialize for LogLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for LogLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("Unknown log level: {}", s))
        })
    }
}

mod custom_datetime_format {
    use chrono::{DateTime, Local, TimeZone};
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(date: &DateTime<Local>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&date.to_rfc3339())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Local>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        
        if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
            return Ok(dt.with_timezone(&Local));
        }
        
        for fmt in &["%Y-%m-%dT%H:%M:%S%.f%:z", "%Y-%m-%dT%H:%M:%S%.fZ", "%Y-%m-%dT%H:%M:%S%:z", "%Y-%m-%dT%H:%M:%SZ"] {
            if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&s, fmt) {
                return Ok(Local.from_utc_datetime(&dt));
            }
        }
        
        Err(serde::de::Error::custom("Invalid datetime format"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    #[serde(with = "custom_datetime_format")]
    pub timestamp: DateTime<Local>,
    pub level: LogLevel,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub fields: LogFields,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogFields {
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOpLogEntry {
    #[serde(with = "custom_datetime_format")]
    pub timestamp: DateTime<Local>,
    pub level: LogLevel,
    #[serde(default)]
    pub fields: FileOpFields,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileOpFields {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub client_ip: String,
    #[serde(default)]
    pub operation: String,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub message: String,
}

pub struct LogBuffer {
    buffer: Arc<RwLock<VecDeque<LogEntry>>>,
    max_size: usize,
}

impl LogBuffer {
    pub fn new(max_size: usize) -> Self {
        Self {
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(max_size))),
            max_size,
        }
    }

    pub fn push(&self, entry: LogEntry) {
        let mut buf = self.buffer.write();
        if buf.len() >= self.max_size {
            buf.pop_front();
        }
        buf.push_back(entry);
    }

    pub fn get_recent(&self, count: usize) -> Vec<LogEntry> {
        let buf = self.buffer.read();
        buf.iter().rev().take(count).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.buffer.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.read().is_empty()
    }

    pub fn clone_inner(&self) -> Arc<RwLock<VecDeque<LogEntry>>> {
        Arc::clone(&self.buffer)
    }
}

impl Clone for LogBuffer {
    fn clone(&self) -> Self {
        Self {
            buffer: Arc::clone(&self.buffer),
            max_size: self.max_size,
        }
    }
}

pub struct FileOpBuffer {
    buffer: Arc<RwLock<VecDeque<FileOpLogEntry>>>,
    max_size: usize,
}

impl FileOpBuffer {
    pub fn new(max_size: usize) -> Self {
        Self {
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(max_size))),
            max_size,
        }
    }

    pub fn push(&self, entry: FileOpLogEntry) {
        let mut buf = self.buffer.write();
        if buf.len() >= self.max_size {
            buf.pop_front();
        }
        buf.push_back(entry);
    }

    pub fn get_recent(&self, count: usize) -> Vec<FileOpLogEntry> {
        let buf = self.buffer.read();
        buf.iter().rev().take(count).cloned().collect()
    }

    pub fn clone_inner(&self) -> Arc<RwLock<VecDeque<FileOpLogEntry>>> {
        Arc::clone(&self.buffer)
    }
}

impl Clone for FileOpBuffer {
    fn clone(&self) -> Self {
        Self {
            buffer: Arc::clone(&self.buffer),
            max_size: self.max_size,
        }
    }
}

pub struct BufferLayer {
    buffer: LogBuffer,
}

impl BufferLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for BufferLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let metadata = event.metadata();
        let target = metadata.target();
        
        if target.starts_with("file_op") {
            return;
        }

        let level = *metadata.level();
        let log_level = LogLevel::from_tracing_level(level);

        let mut visitor = FieldVisitor::new();
        event.record(&mut visitor);

        let entry = LogEntry {
            timestamp: Local::now(),
            level: log_level,
            target: target.to_string(),
            fields: LogFields {
                message: visitor.message.unwrap_or_default(),
                client_ip: visitor.client_ip,
                username: visitor.username,
                action: visitor.action,
            },
        };

        self.buffer.push(entry);
    }
}

pub struct FileOpLayer {
    buffer: FileOpBuffer,
}

impl FileOpLayer {
    pub fn new(buffer: FileOpBuffer) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for FileOpLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let metadata = event.metadata();
        let target = metadata.target();
        
        if !target.starts_with("file_op") {
            return;
        }

        let level = *metadata.level();
        let log_level = LogLevel::from_tracing_level(level);

        let mut visitor = FileOpFieldVisitor::new();
        event.record(&mut visitor);

        let entry = FileOpLogEntry {
            timestamp: Local::now(),
            level: log_level,
            fields: FileOpFields {
                username: visitor.username.unwrap_or_default(),
                client_ip: visitor.client_ip.unwrap_or_default(),
                operation: visitor.operation.unwrap_or_default(),
                file_path: visitor.file_path.unwrap_or_default(),
                file_size: visitor.file_size.unwrap_or(0),
                protocol: visitor.protocol.unwrap_or_default(),
                success: visitor.success.unwrap_or(true),
                message: visitor.message.unwrap_or_default(),
            },
        };

        self.buffer.push(entry);
    }
}

struct FieldVisitor {
    message: Option<String>,
    client_ip: Option<String>,
    username: Option<String>,
    action: Option<String>,
}

impl FieldVisitor {
    fn new() -> Self {
        Self {
            message: None,
            client_ip: None,
            username: None,
            action: None,
        }
    }
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.to_string()),
            "client_ip" => self.client_ip = Some(value.to_string()),
            "username" => self.username = Some(value.to_string()),
            "action" => self.action = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_i64(&mut self, _field: &tracing::field::Field, _value: i64) {}
    fn record_u64(&mut self, _field: &tracing::field::Field, _value: u64) {}
    fn record_bool(&mut self, _field: &tracing::field::Field, _value: bool) {}
}

struct FileOpFieldVisitor {
    message: Option<String>,
    client_ip: Option<String>,
    username: Option<String>,
    operation: Option<String>,
    file_path: Option<String>,
    file_size: Option<u64>,
    protocol: Option<String>,
    success: Option<bool>,
}

impl FileOpFieldVisitor {
    fn new() -> Self {
        Self {
            message: None,
            client_ip: None,
            username: None,
            operation: None,
            file_path: None,
            file_size: None,
            protocol: None,
            success: None,
        }
    }
}

impl tracing::field::Visit for FileOpFieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.to_string()),
            "client_ip" => self.client_ip = Some(value.to_string()),
            "username" => self.username = Some(value.to_string()),
            "operation" => self.operation = Some(value.to_string()),
            "file_path" => self.file_path = Some(value.to_string()),
            "protocol" => self.protocol = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "file_size" {
            self.file_size = Some(value);
        }
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == "success" {
            self.success = Some(value);
        }
    }

    fn record_i64(&mut self, _field: &tracing::field::Field, _value: i64) {}
}

static mut GLOBAL_LOGGER: Option<GlobalLogger> = None;

struct GlobalLogger {
    buffer: LogBuffer,
    file_op_buffer: FileOpBuffer,
    _guard: WorkerGuard,
    _file_op_guard: WorkerGuard,
}

pub struct TracingLogger {
    buffer: LogBuffer,
    file_op_buffer: FileOpBuffer,
}

impl TracingLogger {
    pub fn init(log_dir: &str, _max_size: u64, max_files: usize, log_level: &str) -> Result<Self, String> {
        unsafe {
            if let Some(ref global) = GLOBAL_LOGGER {
                return Ok(TracingLogger {
                    buffer: global.buffer.clone(),
                    file_op_buffer: global.file_op_buffer.clone(),
                });
            }
        }

        let path = PathBuf::from(log_dir);

        if let Err(e) = std::fs::create_dir_all(&path) {
            eprintln!("Warning: Failed to create log directory {}: {}", path.display(), e);
        }

        let buffer = LogBuffer::new(1000);
        let file_op_buffer = FileOpBuffer::new(2000);

        let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .max_log_files(max_files)
            .filename_prefix("wftpg")
            .filename_suffix("log")
            .build(&path)
            .map_err(|e| format!("创建日志文件失败: {}", e))?;

        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let file_op_appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .max_log_files(max_files)
            .filename_prefix("file-ops")
            .filename_suffix("log")
            .build(&path)
            .map_err(|e| format!("创建文件操作日志文件失败: {}", e))?;

        let (file_op_non_blocking, file_op_guard) = tracing_appender::non_blocking(file_op_appender);

        let level_filter: tracing::Level = log_level.to_lowercase().parse()
            .unwrap_or(tracing::Level::INFO);

        let buffer_layer = BufferLayer::new(buffer.clone());
        let file_op_buffer_layer = FileOpLayer::new(file_op_buffer.clone());
        
        // 程序日志层：只记录非 file_op 的日志
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_target(false)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
            .json()
            .with_filter(filter::filter_fn(|metadata| {
                // 排除 file_op 相关的日志
                !metadata.target().starts_with("file_op")
            }));

        // 文件操作日志层：只记录 file_op 的日志
        let file_op_fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(file_op_non_blocking)
            .with_ansi(false)
            .with_target(false)
            .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
            .json()
            .with_filter(filter::filter_fn(|metadata| {
                // 只保留 file_op 相关的日志
                metadata.target().starts_with("file_op")
            }));

        let subscriber = tracing_subscriber::registry()
            .with(tracing::level_filters::LevelFilter::from_level(level_filter))
            .with(buffer_layer)
            .with(file_op_buffer_layer)
            .with(fmt_layer)
            .with(file_op_fmt_layer);

        tracing::subscriber::set_global_default(subscriber)
            .map_err(|e| format!("设置 tracing 日志失败: {}", e))?;

        unsafe {
            GLOBAL_LOGGER = Some(GlobalLogger {
                buffer: buffer.clone(),
                file_op_buffer: file_op_buffer.clone(),
                _guard: guard,
                _file_op_guard: file_op_guard,
            });
        }

        Ok(TracingLogger { 
            buffer,
            file_op_buffer,
        })
    }

    pub fn get_recent_logs(&self, count: usize) -> Vec<LogEntry> {
        self.buffer.get_recent(count)
    }

    pub fn get_recent_file_ops(&self, count: usize) -> Vec<FileOpLogEntry> {
        self.file_op_buffer.get_recent(count)
    }

    pub fn buffer(&self) -> LogBuffer {
        self.buffer.clone()
    }

    pub fn file_op_buffer(&self) -> FileOpBuffer {
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

    pub fn read_logs(&self, count: usize) -> Vec<LogEntry> {
        let mut logs = Vec::new();
        let log_files = match std::fs::read_dir(&self.log_dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|name| {
                            name.starts_with("wftpg-") &&
                            name.ends_with(".log")
                        })
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
                    if let Ok(log_entry) = serde_json::from_str::<LogEntry>(&line) {
                        logs.push(log_entry);
                    }
                }
            }
        }

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

    pub fn read_file_ops(&self, count: usize) -> Vec<FileOpLogEntry> {
        let mut logs = Vec::new();
        let log_files = match std::fs::read_dir(&self.log_dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|name| {
                            name.starts_with("file-ops-") &&
                            name.ends_with(".log")
                        })
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
                    if let Ok(log_entry) = serde_json::from_str::<FileOpLogEntry>(&line) {
                        logs.push(log_entry);
                    }
                }
            }
        }

        logs
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
            "文件上传成功"
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
            "文件更新成功"
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
            "文件下载成功"
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
            "文件删除成功"
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
            "文件重命名成功"
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
            "目录创建成功"
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
            "目录删除成功"
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
            "操作失败: {}",
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

    #[test]
    fn test_log_level_from_tracing() {
        assert_eq!(LogLevel::from_tracing_level(Level::DEBUG), LogLevel::Debug);
        assert_eq!(LogLevel::from_tracing_level(Level::INFO), LogLevel::Info);
        assert_eq!(LogLevel::from_tracing_level(Level::WARN), LogLevel::Warning);
        assert_eq!(LogLevel::from_tracing_level(Level::ERROR), LogLevel::Error);
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
                target: format!("test{}", i),
                fields: LogFields {
                    message: format!("message{}", i),
                    client_ip: None,
                    username: None,
                    action: None,
                },
            });
        }
        assert_eq!(buffer.len(), 5);
        let recent = buffer.get_recent(3);
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_file_op_buffer() {
        let buffer = FileOpBuffer::new(5);
        for i in 0..10 {
            buffer.push(FileOpLogEntry {
                timestamp: Local::now(),
                level: LogLevel::Info,
                fields: FileOpFields {
                    username: format!("user{}", i),
                    client_ip: "127.0.0.1".to_string(),
                    operation: "UPLOAD".to_string(),
                    file_path: format!("/path/{}", i),
                    file_size: 100,
                    protocol: "FTP".to_string(),
                    success: true,
                    message: "test".to_string(),
                },
            });
        }
        assert_eq!(buffer.get_recent(10).len(), 5);
    }

    #[test]
    fn test_log_entry_json_parse() {
        let json = r#"{"timestamp":"2026-03-26T01:41:16.932086200+08:00","level":"INFO","fields":{"message":"WFTPD Service - SFTP/FTP Server Daemon v3.1.17"}}"#;
        let result: Result<LogEntry, _> = serde_json::from_str(json);
        match &result {
            Ok(entry) => {
                assert_eq!(entry.level, LogLevel::Info);
                assert_eq!(entry.target, "");
                assert!(entry.fields.message.contains("WFTPD Service"));
            }
            Err(e) => {
                panic!("Failed to parse log entry: {}", e);
            }
        }
    }

    #[test]
    fn test_log_entry_json_parse_with_client_ip() {
        let json = r#"{"timestamp":"2026-03-26T01:41:21.730255100+08:00","level":"INFO","fields":{"message":"Client connected from 192.168.139.9","client_ip":"192.168.139.9","action":"CONNECT"}}"#;
        let result: Result<LogEntry, _> = serde_json::from_str(json);
        match &result {
            Ok(entry) => {
                assert_eq!(entry.level, LogLevel::Info);
                assert_eq!(entry.fields.client_ip, Some("192.168.139.9".to_string()));
                assert_eq!(entry.fields.action, Some("CONNECT".to_string()));
            }
            Err(e) => {
                panic!("Failed to parse log entry: {}", e);
            }
        }
    }
}
