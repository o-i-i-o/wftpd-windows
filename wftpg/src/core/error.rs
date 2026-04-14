//! 统一的错误类型定义
//!
//! 本模块提供应用程序所有可能的错误类型，使用 thiserror 库进行定义。

use thiserror::Error;

/// 配置相关错误
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Configuration file read failed: {0}")]
    ReadFailed(#[source] std::io::Error),

    #[error("Configuration file parse failed: {0}")]
    ParseFailed(#[from] toml::de::Error),

    #[error("Configuration file serialize failed: {0}")]
    SerializeFailed(#[from] toml::ser::Error),

    #[error("Configuration file write failed: {0}")]
    WriteFailed(#[source] std::io::Error),

    #[error("Invalid configuration path: {0}")]
    InvalidPath(String),

    #[error("Configuration validation failed: {0}")]
    ValidationError(String),
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        // 根据错误上下文判断是读取还是写入错误
        // 这里默认作为读取错误处理
        ConfigError::ReadFailed(err)
    }
}

/// 用户管理相关错误
#[derive(Error, Debug)]
pub enum UserError {
    #[error("User file read failed: {0}")]
    ReadFailed(#[from] std::io::Error),

    #[error("User file parse failed: {0}")]
    ParseFailed(#[from] serde_json::Error),

    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("User already exists: {0}")]
    UserAlreadyExists(String),

    #[error("Password hash failed: {0}")]
    PasswordHashFailed(String),

    #[error("Password verification failed")]
    PasswordVerificationFailed,

    #[error("Invalid user home directory: {0}")]
    InvalidHomeDirectory(String),

    #[error("User disabled: {0}")]
    UserDisabled(String),
}

/// 路径处理相关错误
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    #[error("Path escape detected")]
    PathEscape,

    #[error("Path is not a directory")]
    NotADirectory,

    #[error("Path not found")]
    NotFound,

    #[error("Path depth exceeds maximum limit")]
    PathTooDeep,

    #[error("Home directory not found")]
    HomeDirectoryNotFound,

    #[error("Path canonicalization failed")]
    CanonicalizeFailed,

    #[error("Invalid path")]
    InvalidPath,

    #[error("Symlinks not allowed")]
    SymlinkNotAllowed,

    #[error("Path not under home directory")]
    PathNotUnderHome,
}

/// IPC 通信相关错误
#[derive(Error, Debug)]
pub enum IpcError {
    #[error("IPC connection failed: {0}")]
    ConnectionFailed(String),

    #[error("IPC message send failed: {0}")]
    SendFailed(String),

    #[error("IPC message receive failed: {0}")]
    ReceiveFailed(String),

    #[error("IPC message parse failed: {0}")]
    ParseFailed(#[from] serde_json::Error),

    #[error("IPC timeout: {0}")]
    Timeout(String),

    #[error("IPC message too large: {0}")]
    MessageTooLarge(usize),
}

/// Windows 服务管理相关错误
#[derive(Error, Debug)]
pub enum ServiceError {
    #[error("Failed to open service control manager: {0}")]
    SCMOpenFailed(String),

    #[error("Failed to create service: {0}")]
    ServiceCreateFailed(String),

    #[error("Failed to open service: {0}")]
    ServiceOpenFailed(String),

    #[error("Failed to query service: {0}")]
    ServiceQueryFailed(String),

    #[error("Failed to start service: {0}")]
    ServiceStartFailed(String),

    #[error("Failed to stop service: {0}")]
    ServiceStopFailed(String),

    #[error("Failed to delete service: {0}")]
    ServiceDeleteFailed(String),

    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    #[error("Windows API error: {0}")]
    WindowsApiError(String),
}

/// 日志相关错误
#[derive(Error, Debug)]
pub enum LoggerError {
    #[error("Log directory creation failed: {0}")]
    LogDirCreateFailed(String),

    #[error("Log file creation failed: {0}")]
    LogFileCreateFailed(String),

    #[error("Logger initialization failed: {0}")]
    InitFailed(String),
}

#[derive(Error, Debug)]
pub enum ServerError {
    #[error("Service not installed: {0}")]
    ServiceNotInstalled(String),

    #[error("Service not running: {0}")]
    ServiceNotRunning(String),

    #[error("Service operation failed: {0}")]
    OperationFailed(String),
}

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("User error: {0}")]
    User(#[from] UserError),

    #[error("Path error: {0}")]
    Path(#[from] PathError),

    #[error("IPC error: {0}")]
    Ipc(#[from] IpcError),

    #[error("Service error: {0}")]
    Service(#[from] ServiceError),

    #[error("Logger error: {0}")]
    Logger(#[from] LoggerError),

    #[error("Server error: {0}")]
    Server(#[from] ServerError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(String),
}

impl From<String> for AppError {
    fn from(err: String) -> Self {
        AppError::Other(err)
    }
}

impl From<&str> for AppError {
    fn from(err: &str) -> Self {
        AppError::Other(err.to_string())
    }
}

/// 通用结果类型别名
pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ConfigError::ReadFailed(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(err.to_string().contains("Configuration file read failed"));

        let err = UserError::UserNotFound("test".to_string());
        assert_eq!(err.to_string(), "User not found: test");
    }

    #[test]
    fn test_error_conversion() {
        let io_err = std::io::Error::other("test");
        let app_err: AppError = io_err.into();
        assert!(matches!(app_err, AppError::Io(_)));

        let str_err: AppError = "test error".into();
        assert!(matches!(str_err, AppError::Other(_)));
    }
}
