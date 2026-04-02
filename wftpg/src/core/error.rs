//! 统一的错误类型定义
//!
//! 本模块提供应用程序所有可能的错误类型，使用 thiserror 库进行定义。

use thiserror::Error;

/// 配置相关错误
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("配置文件读取失败：{0}")]
    ReadFailed(#[source] std::io::Error),

    #[error("配置文件解析失败：{0}")]
    ParseFailed(#[from] toml::de::Error),

    #[error("配置文件序列化失败：{0}")]
    SerializeFailed(#[from] toml::ser::Error),

    #[error("配置文件写入失败：{0}")]
    WriteFailed(#[source] std::io::Error),

    #[error("配置路径无效：{0}")]
    InvalidPath(String),

    #[error("配置验证失败：{0}")]
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
    #[error("用户文件读取失败：{0}")]
    ReadFailed(#[from] std::io::Error),

    #[error("用户文件解析失败：{0}")]
    ParseFailed(#[from] serde_json::Error),

    #[error("用户不存在：{0}")]
    UserNotFound(String),

    #[error("用户已存在：{0}")]
    UserAlreadyExists(String),

    #[error("密码哈希失败：{0}")]
    PasswordHashFailed(String),

    #[error("密码验证失败")]
    PasswordVerificationFailed,

    #[error("用户主目录无效：{0}")]
    InvalidHomeDirectory(String),

    #[error("用户已禁用：{0}")]
    UserDisabled(String),
}

/// 路径处理相关错误
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    #[error("路径越界访问")]
    PathEscape,

    #[error("路径不是目录")]
    NotADirectory,

    #[error("路径不存在")]
    NotFound,

    #[error("路径深度超过最大限制")]
    PathTooDeep,

    #[error("主目录不存在")]
    HomeDirectoryNotFound,

    #[error("路径规范化失败")]
    CanonicalizeFailed,

    #[error("无效路径")]
    InvalidPath,

    #[error("不允许符号链接")]
    SymlinkNotAllowed,

    #[error("路径不在主目录下")]
    PathNotUnderHome,
}

/// IPC 通信相关错误
#[derive(Error, Debug)]
pub enum IpcError {
    #[error("IPC 连接失败：{0}")]
    ConnectionFailed(String),

    #[error("IPC 消息发送失败：{0}")]
    SendFailed(String),

    #[error("IPC 消息接收失败：{0}")]
    ReceiveFailed(String),

    #[error("IPC 消息解析失败：{0}")]
    ParseFailed(#[from] serde_json::Error),

    #[error("IPC 超时：{0}")]
    Timeout(String),

    #[error("IPC 消息过大：{0}")]
    MessageTooLarge(usize),
}

/// Windows 服务管理相关错误
#[derive(Error, Debug)]
pub enum ServiceError {
    #[error("服务控制管理器打开失败：{0}")]
    SCMOpenFailed(String),

    #[error("服务创建失败：{0}")]
    ServiceCreateFailed(String),

    #[error("服务打开失败：{0}")]
    ServiceOpenFailed(String),

    #[error("服务查询失败：{0}")]
    ServiceQueryFailed(String),

    #[error("服务启动失败：{0}")]
    ServiceStartFailed(String),

    #[error("服务停止失败：{0}")]
    ServiceStopFailed(String),

    #[error("服务删除失败：{0}")]
    ServiceDeleteFailed(String),

    #[error("服务未找到：{0}")]
    ServiceNotFound(String),

    #[error("Windows API 错误：{0}")]
    WindowsApiError(String),
}

/// 日志相关错误
#[derive(Error, Debug)]
pub enum LoggerError {
    #[error("日志目录创建失败：{0}")]
    LogDirCreateFailed(String),

    #[error("日志文件创建失败：{0}")]
    LogFileCreateFailed(String),

    #[error("日志初始化失败：{0}")]
    InitFailed(String),
}

/// 服务器管理相关错误
#[derive(Error, Debug)]
pub enum ServerError {
    #[error("服务未安装：{0}")]
    ServiceNotInstalled(String),

    #[error("服务未运行：{0}")]
    ServiceNotRunning(String),

    #[error("服务操作失败：{0}")]
    OperationFailed(String),
}

/// 应用程序通用错误类型
#[derive(Error, Debug)]
pub enum AppError {
    #[error("配置错误：{0}")]
    Config(#[from] ConfigError),

    #[error("用户错误：{0}")]
    User(#[from] UserError),

    #[error("路径错误：{0}")]
    Path(#[from] PathError),

    #[error("IPC 错误：{0}")]
    Ipc(#[from] IpcError),

    #[error("服务错误：{0}")]
    Service(#[from] ServiceError),

    #[error("日志错误：{0}")]
    Logger(#[from] LoggerError),

    #[error("服务器错误：{0}")]
    Server(#[from] ServerError),

    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("其他错误：{0}")]
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
        assert!(err.to_string().contains("配置文件读取失败"));

        let err = UserError::UserNotFound("test".to_string());
        assert_eq!(err.to_string(), "用户不存在：test");
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
