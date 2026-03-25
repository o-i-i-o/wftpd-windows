use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, oneshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLogEntry {
    pub timestamp: DateTime<Local>,
    pub username: String,
    pub client_ip: String,
    pub operation: String,
    pub file_path: String,
    pub file_size: u64,
    pub protocol: String,
    pub success: bool,
    pub message: String,
}

pub struct FileLogInfo<'a> {
    pub username: &'a str,
    pub client_ip: &'a str,
    pub operation: &'a str,
    pub file_path: &'a str,
    pub file_size: u64,
    pub protocol: &'a str,
    pub success: bool,
    pub message: &'a str,
}

enum FileLogCommand {
    Write(FileLogEntry),
    GetRecent(usize, oneshot::Sender<Vec<FileLogEntry>>),
    Shutdown,
}

#[derive(Clone)]
pub struct AsyncFileLogger {
    sender: mpsc::UnboundedSender<FileLogCommand>,
    buffer: Arc<Mutex<VecDeque<FileLogEntry>>>,
}

impl AsyncFileLogger {
    pub async fn new(log_dir: &str, max_file_size: u64) -> Self {
        let path = PathBuf::from(log_dir);
        
        if let Err(e) = tokio::fs::create_dir_all(&path).await {
            eprintln!("Warning: Failed to create file log directory {}: {}", path.display(), e);
        }
        
        let (log_path, size) = Self::get_available_log_path(&path).await;
        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(2000)));
        
        let (sender, mut receiver) = mpsc::unbounded_channel();
        
        let buffer_clone = Arc::clone(&buffer);
        let log_dir_clone = path.clone();
        
        tokio::spawn(async move {
            let mut current_file: Option<tokio::fs::File> = None;
            let mut current_size = size;
            let mut current_path = log_path;
            
            while let Some(cmd) = receiver.recv().await {
                match cmd {
                    FileLogCommand::Write(entry) => {
                        {
                            let mut buf = buffer_clone.lock().await;
                            if buf.len() >= 2000 {
                                buf.pop_front();
                            }
                            buf.push_back(entry.clone());
                        }
                        
                        if current_file.is_none() || current_size >= max_file_size {
                            let new_path = Self::get_new_log_path(&log_dir_clone).await;
                            match tokio::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(&new_path)
                                .await
                            {
                                Ok(f) => {
                                    current_file = Some(f);
                                    current_size = 0;
                                    current_path = new_path;
                                }
                                Err(e) => {
                                    eprintln!("Warning: Failed to open file log: {}", e);
                                    continue;
                                }
                            }
                        }
                        
                        let json = serde_json::to_string(&entry)
                            .unwrap_or_else(|_| format!("{{\"message\": \"{}\"}}", entry.message));
                        let line = format!("{}\n", json);
                        let bytes = line.as_bytes();
                        
                        if let Some(ref mut file) = current_file {
                            use tokio::io::AsyncWriteExt;
                            if let Err(e) = file.write_all(bytes).await {
                                eprintln!("Failed to write file log: {}", e);
                            } else {
                                current_size += bytes.len() as u64;
                            }
                        }
                    }
                    FileLogCommand::GetRecent(count, response) => {
                        let buf = buffer_clone.lock().await;
                        let logs: Vec<FileLogEntry> = buf.iter().rev().take(count).cloned().collect();
                        let _ = response.send(logs);
                    }
                    FileLogCommand::Shutdown => {
                        if let Some(mut file) = current_file {
                            use tokio::io::AsyncWriteExt;
                            let _ = file.flush().await;
                        }
                        break;
                    }
                }
            }
        });
        
        AsyncFileLogger {
            sender,
            buffer,
        }
    }

    async fn get_available_log_path(log_dir: &Path) -> (PathBuf, u64) {
        let date_str = Local::now().format("%Y-%m-%d");
        let mut seq = 1;
        
        loop {
            let filename = format!("file-ops-{}-{:04}.log", date_str, seq);
            let log_path = log_dir.join(&filename);
            
            if !log_path.exists() {
                return (log_path, 0);
            }
            
            if let Ok(metadata) = tokio::fs::metadata(&log_path).await {
                let size = metadata.len();
                if size < 2 * 1024 * 1024 {
                    return (log_path, size);
                }
            }
            
            seq += 1;
        }
    }

    async fn get_new_log_path(log_dir: &Path) -> PathBuf {
        let date_str = Local::now().format("%Y-%m-%d");
        let mut seq = 1;
        
        loop {
            let filename = format!("file-ops-{}-{:04}.log", date_str, seq);
            let log_path = log_dir.join(&filename);
            
            if !log_path.exists() {
                return log_path;
            }
            
            seq += 1;
        }
    }

    pub fn log(&self, info: FileLogInfo<'_>) {
        let entry = FileLogEntry {
            timestamp: Local::now(),
            username: info.username.to_string(),
            client_ip: info.client_ip.to_string(),
            operation: info.operation.to_string(),
            file_path: info.file_path.to_string(),
            file_size: info.file_size,
            protocol: info.protocol.to_string(),
            success: info.success,
            message: info.message.to_string(),
        };
        let _ = self.sender.send(FileLogCommand::Write(entry));
    }

    pub async fn get_recent_logs(&self, count: usize) -> Vec<FileLogEntry> {
        let (tx, rx) = oneshot::channel();
        if self.sender.send(FileLogCommand::GetRecent(count, tx)).is_ok() {
            rx.await.unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    pub fn log_upload(&self, username: &str, client_ip: &str, file_path: &str, file_size: u64, protocol: &str) {
        self.log(FileLogInfo {
            username,
            client_ip,
            operation: "UPLOAD",
            file_path,
            file_size,
            protocol,
            success: true,
            message: "文件上传成功",
        });
    }

    pub fn log_update(&self, username: &str, client_ip: &str, file_path: &str, file_size: u64, protocol: &str) {
        self.log(FileLogInfo {
            username,
            client_ip,
            operation: "UPDATE",
            file_path,
            file_size,
            protocol,
            success: true,
            message: "文件更新成功",
        });
    }

    pub fn log_download(&self, username: &str, client_ip: &str, file_path: &str, file_size: u64, protocol: &str) {
        self.log(FileLogInfo {
            username,
            client_ip,
            operation: "DOWNLOAD",
            file_path,
            file_size,
            protocol,
            success: true,
            message: "文件下载成功",
        });
    }

    pub fn log_delete(&self, username: &str, client_ip: &str, file_path: &str, protocol: &str) {
        self.log(FileLogInfo {
            username,
            client_ip,
            operation: "DELETE",
            file_path,
            file_size: 0,
            protocol,
            success: true,
            message: "文件删除成功",
        });
    }

    pub fn log_rename(&self, username: &str, client_ip: &str, old_path: &str, new_path: &str, protocol: &str) {
        self.log(FileLogInfo {
            username,
            client_ip,
            operation: "RENAME",
            file_path: &format!("{} -> {}", old_path, new_path),
            file_size: 0,
            protocol,
            success: true,
            message: "文件重命名成功",
        });
    }

    pub fn log_mkdir(&self, username: &str, client_ip: &str, dir_path: &str, protocol: &str) {
        self.log(FileLogInfo {
            username,
            client_ip,
            operation: "MKDIR",
            file_path: dir_path,
            file_size: 0,
            protocol,
            success: true,
            message: "目录创建成功",
        });
    }

    pub fn log_rmdir(&self, username: &str, client_ip: &str, dir_path: &str, protocol: &str) {
        self.log(FileLogInfo {
            username,
            client_ip,
            operation: "RMDIR",
            file_path: dir_path,
            file_size: 0,
            protocol,
            success: true,
            message: "目录删除成功",
        });
    }

    pub fn log_failed(&self, username: &str, client_ip: &str, operation: &str, file_path: &str, protocol: &str, error: &str) {
        self.log(FileLogInfo {
            username,
            client_ip,
            operation,
            file_path,
            file_size: 0,
            protocol,
            success: false,
            message: error,
        });
    }

    pub fn shutdown(&self) {
        let _ = self.sender.send(FileLogCommand::Shutdown);
    }
}
