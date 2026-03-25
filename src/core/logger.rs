use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::Level;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter, Layer, Registry};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Local>,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub fn from_tracing_level(level: Level) -> Self {
        match level {
            Level::DEBUG => LogLevel::Debug,
            Level::INFO => LogLevel::Info,
            Level::WARN => LogLevel::Warning,
            Level::ERROR => LogLevel::Error,
            _ => LogLevel::Info,
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

pub fn init_file_logging(log_dir: &str, max_size: u64, max_files: usize) -> Result<(), String> {
    let log_dir = PathBuf::from(log_dir);

    if let Err(e) = fs::create_dir_all(&log_dir) {
        return Err(format!("创建日志目录失败: {}", e));
    }

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .with_thread_ids(false)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(move || FileWriter::new(log_dir.clone(), max_size, max_files))
        .with_filter(EnvFilter::from_default_env().add_directive(Level::INFO.into()));

    let subscriber = Registry::default().with(file_layer);

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| format!("设置日志 subscriber 失败: {}", e))?;

    Ok(())
}

struct FileWriter {
    log_dir: PathBuf,
    max_size: u64,
    max_files: usize,
    current_file: Option<File>,
    current_size: u64,
}

impl FileWriter {
    fn new(log_dir: PathBuf, max_size: u64, max_files: usize) -> Self {
        let (file, size) = Self::open_log_file(&log_dir);
        Self {
            log_dir,
            max_size,
            max_files,
            current_file: file,
            current_size: size,
        }
    }

    fn open_log_file(log_dir: &Path) -> (Option<File>, u64) {
        let (path, size) = get_log_file_path(log_dir);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        (file, size)
    }

    fn rotate_if_needed(&mut self) -> std::io::Result<()> {
        if self.current_size >= self.max_size {
            let _ = self.current_file.take();
            let _ = self.cleanup_old_logs();
            let (file, size) = Self::open_log_file(&self.log_dir);
            self.current_file = file;
            self.current_size = size;
        }
        Ok(())
    }

    fn cleanup_old_logs(&self) -> std::io::Result<()> {
        let mut log_files: Vec<_> = fs::read_dir(&self.log_dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                e.metadata().ok().map(|m| {
                    let modified = m.modified().ok();
                    (e, modified)
                })
            })
            .collect();

        log_files.sort_by(|a, b| match (&a.1, &b.1) {
            (Some(a_time), Some(b_time)) => a_time.cmp(b_time),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });

        while log_files.len() > self.max_files {
            if let Some((old_file, _)) = log_files.first() {
                let _ = fs::remove_file(old_file.path());
                log_files.remove(0);
            }
        }

        Ok(())
    }
}

impl Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.rotate_if_needed()?;

        if let Some(ref mut file) = self.current_file {
            let result = file.write(buf);
            if result.is_ok() {
                self.current_size += buf.len() as u64;
            }
            result
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(ref mut file) = self.current_file {
            file.flush()
        } else {
            Ok(())
        }
    }
}

fn get_log_file_path(log_dir: &Path) -> (PathBuf, u64) {
    let date_str = Local::now().format("%Y-%m-%d");
    let mut seq = 1;

    loop {
        let filename = format!("wftpg-{}-{:04}.log", date_str, seq);
        let log_path = log_dir.join(&filename);

        if !log_path.exists() {
            return (log_path, 0);
        }

        if let Ok(metadata) = fs::metadata(&log_path) {
            let size = metadata.len();
            if size < 2 * 1024 * 1024 {
                return (log_path, size);
            }
        }

        seq += 1;
    }
}

pub struct Logger {
    log_dir: PathBuf,
    max_size: u64,
    max_files: usize,
    current_file: Option<File>,
    current_size: u64,
    buffer: Arc<Mutex<VecDeque<LogEntry>>>,
    max_buffer_size: usize,
}

impl Logger {
    pub fn new(log_dir: &str, max_size: u64, max_files: usize) -> Self {
        let path = PathBuf::from(log_dir);

        if let Err(e) = fs::create_dir_all(&path) {
            eprintln!("Warning: Failed to create log directory {}: {}", path.display(), e);
        }

        let (log_path, size) = get_log_file_path(&path);
        let file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("Warning: Failed to open log file: {}", e);
                None
            }
        };

        Logger {
            log_dir: path,
            max_size,
            max_files,
            current_file: file,
            current_size: size,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(1000))),
            max_buffer_size: 1000,
        }
    }

    fn rotate_if_needed(&mut self) -> std::io::Result<()> {
        if self.current_size >= self.max_size {
            let new_path = self.get_new_log_path();
            self.current_file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&new_path)?,
            );
            self.current_size = 0;
            self.cleanup_old_logs()?;
        }

        Ok(())
    }

    fn get_new_log_path(&self) -> PathBuf {
        let date_str = Local::now().format("%Y-%m-%d");
        let mut seq = 1;

        loop {
            let filename = format!("wftpg-{}-{:04}.log", date_str, seq);
            let log_path = self.log_dir.join(&filename);

            if !log_path.exists() {
                return log_path;
            }

            seq += 1;
        }
    }

    fn cleanup_old_logs(&self) -> std::io::Result<()> {
        let mut log_files: Vec<_> = fs::read_dir(&self.log_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("wftpg-"))
            .filter_map(|e| {
                e.metadata().ok().map(|m| {
                    let modified = m.modified().ok();
                    (e, modified)
                })
            })
            .collect();

        log_files.sort_by(|a, b| match (&a.1, &b.1) {
            (Some(a_time), Some(b_time)) => a_time.cmp(b_time),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });

        while log_files.len() > self.max_files {
            if let Some((old_file, _)) = log_files.first() {
                fs::remove_file(old_file.path())?;
                log_files.remove(0);
            }
        }

        Ok(())
    }

    pub fn log(
        &mut self,
        level: LogLevel,
        source: &str,
        message: &str,
        client_ip: Option<&str>,
        username: Option<&str>,
        action: Option<&str>,
    ) {
        let entry = LogEntry {
            timestamp: Local::now(),
            level: level.clone(),
            target: source.to_string(),
            message: message.to_string(),
            client_ip: client_ip.map(|s| s.to_string()),
            username: username.map(|s| s.to_string()),
            action: action.map(|s| s.to_string()),
        };

        {
            let mut buffer = self.buffer.lock().unwrap();
            if buffer.len() >= self.max_buffer_size {
                buffer.pop_front();
            }
            buffer.push_back(entry.clone());
        }

        if let Err(e) = self.write_to_file(&entry) {
            eprintln!("Failed to write log: {}", e);
        }

        let level_str = match level {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warning => "WARN",
            LogLevel::Error => "ERROR",
        };

        println!(
            "[{}] [{}] {} - {}",
            entry.timestamp.format("%Y-%m-%d %H:%M:%S"),
            level_str,
            entry.target,
            entry.message
        );
    }

    fn write_to_file(&mut self, entry: &LogEntry) -> std::io::Result<()> {
        if self.current_file.is_none() || self.current_size >= self.max_size {
            self.rotate_if_needed()?;
        }

        let json = serde_json::to_string(entry)
            .unwrap_or_else(|_| format!("{{\"message\": \"{}\"}}", entry.message));

        if let Some(ref mut file) = self.current_file {
            let line = format!("{}\n", json);
            let bytes = line.as_bytes();
            file.write_all(bytes)?;
            self.current_size += bytes.len() as u64;
        }

        Ok(())
    }

    pub fn get_recent_logs(&self, count: usize) -> Vec<LogEntry> {
        let buffer = self.buffer.lock().unwrap();
        buffer.iter().rev().take(count).cloned().collect()
    }

    pub fn info(&mut self, source: &str, message: &str) {
        self.log(LogLevel::Info, source, message, None, None, None);
    }

    pub fn debug(&mut self, source: &str, message: &str) {
        self.log(LogLevel::Debug, source, message, None, None, None);
    }

    pub fn warning(&mut self, source: &str, message: &str) {
        self.log(LogLevel::Warning, source, message, None, None, None);
    }

    pub fn error(&mut self, source: &str, message: &str) {
        self.log(LogLevel::Error, source, message, None, None, None);
    }

    pub fn client_action(
        &mut self,
        source: &str,
        message: &str,
        client_ip: &str,
        username: Option<&str>,
        action: &str,
    ) {
        self.log(
            LogLevel::Info,
            source,
            message,
            Some(client_ip),
            username,
            Some(action),
        );
    }

    pub fn client_action_debug(
        &mut self,
        source: &str,
        message: &str,
        client_ip: &str,
        username: Option<&str>,
        action: &str,
    ) {
        self.log(
            LogLevel::Debug,
            source,
            message,
            Some(client_ip),
            username,
            Some(action),
        );
    }
}

pub struct LogReader {
    log_dir: PathBuf,
    buffer: Arc<Mutex<VecDeque<LogEntry>>>,
    max_buffer_size: usize,
}

impl LogReader {
    pub fn new(log_dir: &str) -> Self {
        Self {
            log_dir: PathBuf::from(log_dir),
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(1000))),
            max_buffer_size: 1000,
        }
    }

    pub fn read_logs(&self, count: usize) -> Vec<LogEntry> {
        let mut logs = Vec::new();
        let log_files = match fs::read_dir(&self.log_dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with("wftpg-") && name.ends_with(".log"))
                })
                .collect::<Vec<_>>(),
            Err(_) => return logs,
        };

        for entry in log_files.iter().rev() {
            if logs.len() >= count {
                break;
            }

            if let Ok(file) = File::open(entry.path()) {
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
            let mut buffer = self.buffer.lock().unwrap();
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

    pub fn get_recent_logs(&self, count: usize) -> Vec<LogEntry> {
        let buffer = self.buffer.lock().unwrap();
        buffer.iter().rev().take(count).cloned().collect()
    }
}

impl Default for LogReader {
    fn default() -> Self {
        Self::new("C:\\ProgramData\\wftpg\\logs")
    }
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
}