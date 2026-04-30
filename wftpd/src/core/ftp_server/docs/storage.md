# storage.rs - 存储后端

## 功能概述

本文件实现了支持配额管理和速率限制的文件系统存储后端，包装libunftp的Filesystem存储并添加配额跟踪功能。

## 设计原理

### 存储层次结构

```
QuotaFilesystem（最外层）
    ├── 配额管理
    ├── 速率限制
    └── Filesystem（内层）
        └── 实际文件操作
```

### 配额管理流程

```
上传文件 (PUT):
    1. 预留配额（100MB估算值）
    2. 执行写入（应用速率限制）
    3. 成功 → 提交实际字节数
    4. 失败 → 回滚预留配额

删除文件 (DEL):
    1. 获取文件大小
    2. 删除文件
    3. 减少配额使用量

删除目录 (RMD):
    1. 计算目录总大小
    2. 删除目录
    3. 减少配额使用量
```

## 核心结构

### QuotaFilesystem

```rust
#[derive(Debug)]
pub struct QuotaFilesystem {
    inner: Filesystem,                      // 内层文件系统
    quota_manager: Arc<QuotaManager>,       // 配额管理器
    user_manager: Arc<Mutex<UserManager>>,  // 用户管理器
}
```

### 常量定义

```rust
const ESTIMATED_MAX_FILE_SIZE: u64 = 100 * 1024 * 1024; // 100MB
```

**说明**: 上传时预留的配额大小，用于防止上传过程中超出配额

## StorageBackend trait实现

### 支持的特性

```rust
fn supported_features(&self) -> u32 {
    <Filesystem as StorageBackend<WftpdUser>>::supported_features(&self.inner)
}
```

### 元数据操作

```rust
async fn metadata<P: AsRef<Path> + Send + Debug>(
    &self,
    user: &WftpdUser,
    path: P,
) -> Result<Self::Metadata>
```

**功能**: 获取文件/目录元数据，直接委托给内层

### 目录列表

```rust
async fn list<P: AsRef<Path> + Send + Debug>(
    &self,
    user: &WftpdUser,
    path: P,
) -> Result<Vec<Fileinfo<PathBuf, Self::Metadata>>>
```

**功能**: 列出目录内容，直接委托给内层

### 文件下载 (GET)

```rust
async fn get<P: AsRef<Path> + Send + Debug>(
    &self,
    user: &WftpdUser,
    path: P,
    start_pos: u64,
) -> Result<Box<dyn AsyncRead + Send + Sync + Unpin>>
```

**流程**:
```
获取文件读取器
    ↓
检查用户速率限制配置
    ↓
有限制 → 包装为RateLimitedReader
无限制 → 直接返回
```

**速率限制实现**:
```rust
if let Some(speed_limit) = user.permissions.speed_limit_kbps {
    if speed_limit > 0 {
        let limited_reader = RateLimitedReader::new(reader, speed_limit);
        return Ok(Box::new(limited_reader));
    }
}
Ok(reader)
```

### 文件上传 (PUT)

```rust
async fn put<P: AsRef<Path> + Send + Debug, R: AsyncRead + Send + Sync + Unpin + 'static>(
    &self,
    user: &WftpdUser,
    input: R,
    path: P,
    start_pos: u64,
) -> Result<u64>
```

**详细流程**:

#### 1. 配额预留
```rust
let reserved_bytes = if let Some(quota_mb) = self.get_user_quota_mb(username) {
    if !self.quota_manager.try_reserve_quota(username, ESTIMATED_MAX_FILE_SIZE, quota_mb)? {
        return Err(Self::quota_exceeded());
    }
    Some(ESTIMATED_MAX_FILE_SIZE)
} else {
    None
};
```

#### 2. 速率限制应用
```rust
let upload_result = if let Some(limit) = speed_limit {
    let limited_input = RateLimitedReader::new(input, limit);
    self.inner.put(user, limited_input, path, start_pos).await
} else {
    self.inner.put(user, input, path, start_pos).await
};
```

#### 3. 结果处理
```rust
match upload_result {
    Ok(bytes_written) => {
        // 提交实际使用量
        if let Some(reserved) = reserved_bytes {
            self.quota_manager.commit_usage(username, reserved, bytes_written)?;
        }
        Ok(bytes_written)
    }
    Err(e) => {
        // 回滚预留
        if let Some(reserved) = reserved_bytes {
            self.quota_manager.rollback_reservation(username, reserved)?;
        }
        Err(e)
    }
}
```

### 文件删除 (DEL)

```rust
async fn del<P: AsRef<Path> + Send + Debug>(
    &self,
    user: &WftpdUser,
    path: P,
) -> Result<()>
```

**流程**:
```
获取文件元数据（大小）
    ↓
删除文件
    ↓
减少配额使用量
```

### 创建目录 (MKD)

```rust
async fn mkd<P: AsRef<Path> + Send + Debug>(
    &self,
    user: &WftpdUser,
    path: P,
) -> Result<()>
```

**功能**: 创建目录，直接委托给内层

### 重命名 (RENAME)

```rust
async fn rename<P: AsRef<Path> + Send + Debug>(
    &self,
    user: &WftpdUser,
    from: P,
    to: P,
) -> Result<()>
```

**功能**: 重命名文件/目录，直接委托给内层（不改变配额）

### 删除目录 (RMD)

```rust
async fn rmd<P: AsRef<Path> + Send + Debug>(
    &self,
    user: &WftpdUser,
    path: P,
) -> Result<()>
```

**流程**:
```
计算目录总大小
    ↓
删除目录
    ↓
减少配额使用量
```

### 切换目录 (CWD)

```rust
async fn cwd<P: AsRef<Path> + Send + Debug>(
    &self,
    user: &WftpdUser,
    path: P,
) -> Result<()>
```

**功能**: 切换工作目录，直接委托给内层

## 辅助方法

### get_user_quota_mb

```rust
fn get_user_quota_mb(&self, username: &str) -> Option<u64>
```

**功能**: 获取用户的配额限制（MB）

### quota_exceeded

```rust
fn quota_exceeded() -> unftp_core::storage::Error
```

**功能**: 创建配额超限错误

### calculate_dir_size

```rust
async fn calculate_dir_size(&self, path: &Path) -> Result<u64>
```

**功能**: 异步计算目录总大小

**实现**: 使用`spawn_blocking`在阻塞线程中执行，避免阻塞异步运行时

### calculate_dir_size_sync

```rust
fn calculate_dir_size_sync(path: &Path) -> std::io::Result<u64>
```

**功能**: 同步递归计算目录大小

**算法**:
```
遍历目录
    ├── 子目录 → 递归计算
    └── 文件 → 累加大小
返回总大小
```

## 配额管理原理

### 预留-提交-回滚机制

```
┌─────────────────────────────────────────────────────┐
│                    上传开始                          │
│                                                     │
│  try_reserve_quota(username, 100MB, quota_mb)      │
│         │                                           │
│         ▼                                           │
│  ┌─────────────┐                                   │
│  │ 预留成功？   │                                   │
│  └─────────────┘                                   │
│      │是        │否                                │
│      ▼           ▼                                  │
│  执行上传    返回配额超限错误                        │
│      │                                              │
│      ▼                                              │
│  ┌─────────────┐                                   │
│  │ 上传成功？   │                                   │
│  └─────────────┘                                   │
│      │是        │否                                │
│      ▼           ▼                                  │
│  commit_usage  rollback_reservation                │
│  (预留, 实际)   (预留)                              │
└─────────────────────────────────────────────────────┘
```

### 为什么使用预留机制

1. **防止竞态条件**: 多个上传同时进行时，先预留再写入
2. **提前检测**: 在写入前就知道是否会超限
3. **原子性**: 预留和回滚是原子操作

### 为什么预留100MB

1. **平衡性能**: 太小可能导致多次预留，太大浪费配额空间
2. **常见文件大小**: 大多数上传文件在100MB以内
3. **实际调整**: 写入完成后会提交实际大小

## 速率限制原理

### RateLimitedReader包装

```rust
pub struct RateLimitedReader<R> {
    inner: R,
    bytes_per_second: usize,
    bucket: TokenBucket,  // 令牌桶算法
}
```

### 工作流程

```
读取请求
    ↓
检查令牌桶
    ├── 有令牌 → 读取数据，消耗令牌
    └── 无令牌 → 等待令牌补充
    ↓
返回数据
```

## 错误处理

| 错误类型 | 处理方式 |
|----------|----------|
| 配额超限 | 返回InsufficientStorageSpaceError |
| 配额操作失败 | 记录错误日志，继续操作 |
| 目录大小计算失败 | 返回错误 |
| 文件操作失败 | 回滚配额预留 |

## 日志记录

| 操作 | 日志级别 | 内容 |
|------|----------|------|
| list | WARN | 用户名、路径 |
| get | WARN | 用户名、路径、起始位置 |
| put | WARN | 用户名、路径、起始位置 |
| mkd | WARN | 用户名、路径 |
| 配额超限 | WARN | 用户名、配额限制 |
| 速率限制应用 | DEBUG | 用户名、限制值 |
| 配额操作失败 | ERROR | 错误详情 |

## 使用示例

```rust
// 创建存储后端
let fs = Filesystem::new("/ftp/root")?;
let quota_fs = QuotaFilesystem::new(
    fs,
    quota_manager,
    user_manager,
);

// 包装为权限控制VFS
let vfs = RestrictingVfs::<_, WftpdUser, Meta>::new(quota_fs);

// 创建存储工厂
let storage_factory = Box::new(move || vfs.clone());
```
