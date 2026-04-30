# listeners.rs - 事件监听器

## 功能概述

本文件实现libunftp的事件监听器，用于记录文件操作日志和管理会话状态。

## 设计原理

### 事件驱动架构

libunftp在关键操作点触发事件，监听器可以：
- 记录操作日志
- 执行额外处理
- 管理会话状态

```
FTP操作 → libunftp → 触发事件 → 监听器处理
                                      ↓
                              日志记录/状态更新
```

### 监听器类型

| 监听器 | 监听事件 | 用途 |
|--------|----------|------|
| FtpDataListener | 文件操作 | 记录上传/下载/删除日志 |
| FtpPresenceListener | 会话状态 | 管理连接/断开/登录 |

## 核心结构

### FtpDataListener - 数据操作监听器

```rust
#[derive(Debug)]
pub struct FtpDataListener {
    config: Arc<Mutex<Config>>,
}
```

**功能**: 监听并记录所有文件数据操作

**监听的操作**:
- 上传 (PUT)
- 下载 (GET)
- 删除文件 (DEL)
- 创建目录 (MKD)
- 删除目录 (RMD)
- 重命名 (RENAME)

### FtpPresenceListener - 在线状态监听器

```rust
#[derive(Debug)]
pub struct FtpPresenceListener {
    session_tracker: Arc<SessionTracker>,
    config: Arc<Mutex<Config>>,
}
```

**功能**: 监听用户会话状态变化

**监听的事件**:
- 登录成功 (LoggedIn)
- 登出 (LoggedOut)

## EventListener trait实现

### FtpDataListener实现

```rust
#[async_trait]
impl EventListener for FtpDataListener {
    type Event = DataEvent;
    
    async fn on_event(&self, event: Self::Event) {
        match event {
            DataEvent::Put { .. } => self.handle_put(event).await,
            DataEvent::Got { .. } => self.handle_get(event).await,
            DataEvent::Deleted { .. } => self.handle_delete(event).await,
            DataEvent::MadeDir { .. } => self.handle_mkdir(event).await,
            DataEvent::RemovedDir { .. } => self.handle_rmdir(event).await,
            DataEvent::Renamed { .. } => self.handle_rename(event).await,
        }
    }
}
```

### FtpPresenceListener实现

```rust
#[async_trait]
impl EventListener for FtpPresenceListener {
    type Event = PresenceEvent;
    
    async fn on_event(&self, event: Self::Event) {
        match event {
            PresenceEvent::LoggedIn { .. } => self.handle_login(event).await,
            PresenceEvent::LoggedOut { .. } => self.handle_logout(event).await,
        }
    }
}
```

## 事件处理详解

### 文件上传 (PUT)

```rust
async fn handle_put(&self, event: DataEvent) {
    if let DataEvent::Put { username, path, bytes, .. } = event {
        tracing::info!(
            "FTP UPLOAD: user={}, path={}, bytes={}",
            username.as_deref().unwrap_or("unknown"),
            path.display(),
            bytes
        );
    }
}
```

**日志格式**: `FTP UPLOAD: user=<用户>, path=<路径>, bytes=<字节数>`

### 文件下载 (GET)

```rust
async fn handle_get(&self, event: DataEvent) {
    if let DataEvent::Got { username, path, bytes, .. } = event {
        tracing::info!(
            "FTP DOWNLOAD: user={}, path={}, bytes={}",
            username.as_deref().unwrap_or("unknown"),
            path.display(),
            bytes
        );
    }
}
```

**日志格式**: `FTP DOWNLOAD: user=<用户>, path=<路径>, bytes=<字节数>`

### 文件删除 (DEL)

```rust
async fn handle_delete(&self, event: DataEvent) {
    if let DataEvent::Deleted { username, path, .. } = event {
        tracing::info!(
            "FTP DELETE: user={}, path={}",
            username.as_deref().unwrap_or("unknown"),
            path.display()
        );
    }
}
```

**日志格式**: `FTP DELETE: user=<用户>, path=<路径>`

### 创建目录 (MKD)

```rust
async fn handle_mkdir(&self, event: DataEvent) {
    if let DataEvent::MadeDir { username, path, .. } = event {
        tracing::info!(
            "FTP MKDIR: user={}, path={}",
            username.as_deref().unwrap_or("unknown"),
            path.display()
        );
    }
}
```

**日志格式**: `FTP MKDIR: user=<用户>, path=<路径>`

### 删除目录 (RMD)

```rust
async fn handle_rmdir(&self, event: DataEvent) {
    if let DataEvent::RemovedDir { username, path, .. } = event {
        tracing::info!(
            "FTP RMDIR: user={}, path={}",
            username.as_deref().unwrap_or("unknown"),
            path.display()
        );
    }
}
```

**日志格式**: `FTP RMDIR: user=<用户>, path=<路径>`

### 重命名 (RENAME)

```rust
async fn handle_rename(&self, event: DataEvent) {
    if let DataEvent::Renamed { username, from, to, .. } = event {
        tracing::info!(
            "FTP RENAME: user={}, from={}, to={}",
            username.as_deref().unwrap_or("unknown"),
            from.display(),
            to.display()
        );
    }
}
```

**日志格式**: `FTP RENAME: user=<用户>, from=<原路径>, to=<新路径>`

### 用户登录 (LoggedIn)

```rust
async fn handle_login(&self, event: PresenceEvent) {
    if let PresenceEvent::LoggedIn { username, meta } = event {
        // 注册trace映射
        if username != "unknown"
            && let Some(client_ip) = self.session_tracker.get_ip_for_user(&username)
        {
            self.session_tracker.register_trace(
                meta.trace_id.to_string(),
                &username,
                client_ip,
            );
        }
        
        // 记录日志
        tracing::info!(
            "FTP SESSION_START: user={}, ip={}",
            username,
            meta.client_ip.as_deref().unwrap_or("unknown")
        );
    }
}
```

**处理流程**:
```
用户登录成功
    ↓
检查用户名是否有效
    ↓
获取用户IP
    ↓
注册trace映射
    ↓
记录会话开始日志
```

### 用户登出 (LoggedOut)

```rust
async fn handle_logout(&self, event: PresenceEvent) {
    if let PresenceEvent::LoggedOut { username, meta } = event {
        // 注销连接
        self.config.unregister_connection(&meta.client_ip.unwrap_or_default());
        
        // 记录日志
        tracing::info!(
            "FTP SESSION_END: user={}, ip={}",
            username,
            meta.client_ip.as_deref().unwrap_or("unknown")
        );
    }
}
```

**处理流程**:
```
用户登出
    ↓
注销连接（释放连接计数）
    ↓
记录会话结束日志
```

## 使用示例

### 创建监听器

```rust
use crate::core::ftp_server::{FtpDataListener, FtpPresenceListener};

// 创建数据监听器
let data_listener = Arc::new(FtpDataListener::new(config.clone()));

// 创建状态监听器
let presence_listener = Arc::new(FtpPresenceListener::new(
    session_tracker.clone(),
    config.clone(),
));
```

### 配置到服务器

```rust
let server = ServerBuilder::new(storage_factory)
    .authenticator(authenticator)
    .event_listener(data_listener)
    .event_listener(presence_listener)
    .build()?;
```

## 日志输出示例

### 操作日志

```
2024-01-15T10:30:15Z INFO FTP SESSION_START: user=testuser, ip=192.168.1.100
2024-01-15T10:30:20Z INFO FTP UPLOAD: user=testuser, path=/upload/file.txt, bytes=1024
2024-01-15T10:30:25Z INFO FTP DOWNLOAD: user=testuser, path=/download/data.zip, bytes=5120
2024-01-15T10:30:30Z INFO FTP MKDIR: user=testuser, path=/new_folder
2024-01-15T10:30:35Z INFO FTP RENAME: user=testuser, from=/old.txt, to=/new.txt
2024-01-15T10:30:40Z INFO FTP DELETE: user=testuser, path=/temp.txt
2024-01-15T10:30:45Z INFO FTP RMDIR: user=testuser, path=/old_folder
2024-01-15T10:30:50Z INFO FTP SESSION_END: user=testuser, ip=192.168.1.100
```

## 与其他组件的交互

### 与SessionTracker交互

```
用户登录
    ↓
FtpPresenceListener.handle_login()
    ↓
session_tracker.register_trace(trace_id, username, ip)
    ↓
建立trace_id → username → ip 映射
```

### 与Config交互

```
用户登出
    ↓
FtpPresenceListener.handle_logout()
    ↓
config.unregister_connection(ip)
    ↓
释放IP连接计数
```

## 扩展可能性

### 可添加的功能

1. **审计日志**: 记录更详细的操作信息
2. **统计收集**: 收集操作统计数据
3. **告警触发**: 异常操作触发告警
4. **实时通知**: WebSocket推送操作通知

### 扩展示例

```rust
async fn handle_put(&self, event: DataEvent) {
    if let DataEvent::Put { username, path, bytes, .. } = event {
        // 原有日志
        tracing::info!("FTP UPLOAD: ...");
        
        // 扩展：检查是否为大文件
        if bytes > 100 * 1024 * 1024 {
            self.send_large_file_alert(&username, &path, bytes).await;
        }
        
        // 扩展：更新统计
        self.stats.record_upload(bytes).await;
    }
}
```

## 性能考虑

### 异步处理

所有事件处理都是异步的，不会阻塞FTP操作：
```rust
async fn on_event(&self, event: Self::Event) {
    // 异步处理，不阻塞主流程
}
```

### 轻量级操作

- 仅记录必要信息
- 避免复杂计算
- 使用异步IO

## 错误处理

监听器中的错误不应影响FTP服务：
- 日志记录失败：忽略
- 状态更新失败：记录警告
- 其他错误：记录错误日志

```rust
async fn handle_login(&self, event: PresenceEvent) {
    if let Err(e) = self.do_handle_login(&event).await {
        tracing::error!("Failed to handle login event: {}", e);
    }
}
```

## 构造函数

### FtpDataListener::new

```rust
impl FtpDataListener {
    pub fn new(config: Arc<Mutex<Config>>) -> Self {
        FtpDataListener { config }
    }
}
```

### FtpPresenceListener::new

```rust
impl FtpPresenceListener {
    pub fn new(
        session_tracker: Arc<SessionTracker>,
        config: Arc<Mutex<Config>>,
    ) -> Self {
        FtpPresenceListener {
            session_tracker,
            config,
        }
    }
}
```
