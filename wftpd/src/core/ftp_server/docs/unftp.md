# unftp.rs - FTP服务器核心实现

## 功能概述

本文件是基于libunftp库实现的FTP/FTPS服务器核心，负责服务器的初始化、配置、启动和关闭管理。

## 设计原理

### 架构设计

服务器采用分层架构：
1. **配置层**: 从Config读取FTP相关配置
2. **资源层**: 管理UserManager、QuotaManager、Fail2BanManager等共享资源
3. **服务层**: 构建libunftp服务器实例并运行

### 生命周期

```
创建(FtpServer::new)
    ↓
启动(start)
    ↓
验证配置 → 初始化UPnP → 创建存储工厂 → 配置认证器
    ↓
配置被动模式 → 设置FTPS → 构建服务器 → 监听端口
    ↓
运行中...
    ↓
停止(stop) → 发送关闭信号 → 优雅关闭
```

## 核心结构

### FtpServer

```rust
pub struct FtpServer {
    config: Arc<Mutex<Config>>,           // 全局配置
    user_manager: Arc<Mutex<UserManager>>, // 用户管理器
    quota_manager: Arc<QuotaManager>,      // 配额管理器
    fail2ban_manager: Arc<Fail2BanManager>, // 防暴力破解
    upnp_manager: Arc<UpnpManager>,        // UPnP管理
    session_tracker: Arc<SessionTracker>,  // 会话跟踪
    running: Arc<Mutex<bool>>,             // 运行状态
    shutdown_tx: Arc<TokioMutex<Option<oneshot::Sender<()>>>>, // 关闭通道
}
```

### FtpServerConfig（内部配置）

```rust
struct FtpServerConfig {
    bind_ip: String,              // 绑定IP地址
    ftp_port: u16,                // FTP端口
    welcome_msg: String,          // 欢迎消息
    passive_ports: (u16, u16),    // 被动模式端口范围
    idle_timeout: u64,            // 空闲超时（秒）
    ftps_enabled: bool,           // 是否启用FTPS
    ftps_cert_path: Option<String>, // 证书路径
    ftps_key_path: Option<String>,  // 密钥路径
    ftps_require_ssl: bool,       // 是否强制SSL
    pooled_listener_mode: bool,   // 池化监听模式
    upnp_enabled: bool,           // 是否启用UPnP
    masquerade_address: Option<String>, // 伪装地址
    allow_nat_clients: bool,      // 是否允许NAT客户端
    ftp_root: Option<String>,     // FTP根目录
}
```

## 主要方法

### new - 创建服务器实例

```rust
pub fn new(config: Arc<Mutex<Config>>, user_manager: Arc<Mutex<UserManager>>) -> Self
```

**功能**: 初始化FTP服务器及其依赖组件

**流程**:
1. 创建QuotaManager实例
2. 从配置读取Fail2Ban设置并创建Fail2BanManager
3. 创建UpnpManager实例
4. 创建SessionTracker实例
5. 返回FtpServer实例

### start - 启动服务器

```rust
pub async fn start(&self) -> Result<()>
```

**功能**: 异步启动FTP服务器

**流程**:
1. 验证配置路径有效性
2. 记录监听地址信息
3. 启动Fail2Ban清理任务
4. 创建关闭信号通道
5. 在后台任务中运行FTP服务器

### run_ftp_server - 运行服务器（内部）

```rust
async fn run_ftp_server(
    config: FtpServerConfig,
    resources: FtpServerResources,
    shutdown_rx: oneshot::Receiver<()>,
) -> Result<()>
```

**功能**: 构建并运行libunftp服务器

**详细流程**:

#### 1. 准备存储后端
```
确定FTP根目录（配置或用户目录）
    ↓
创建Filesystem存储实例
    ↓
包装为QuotaFilesystem（支持配额）
    ↓
包装为RestrictingVfs（支持权限控制）
```

#### 2. 配置认证
```
创建WftpdAuthenticator
    ├── 关联UserManager
    ├── 关联Fail2BanManager
    └── 关联SessionTracker

创建WftpdUserDetailProvider
    └── 提供用户详情（主目录、权限）
```

#### 3. 配置被动模式
```
判断被动模式地址策略：
    ├── UPnP启用且有外部IP → PassiveHost::Ip(upnp_ip)
    ├── 伪装地址配置且有效 → PassiveHost::Ip(masq_ip)
    ├── 绑定特定IP（非通配符）→ PassiveHost::Ip(bind_ip)
    └── 通配符绑定 → PassiveHost::FromConnection
    ↓
设置PassiveHost到server_builder
```

#### 4. 配置FTPS（如果启用）
```
检查证书路径配置
    ↓
确保证书存在（自动生成自签名证书）
    ↓
配置TLS选项
```

#### 5. 构建并启动服务器
```
ServerBuilder配置
    ├── 存储工厂
    ├── 认证器
    ├── 用户详情提供者
    ├── 欢迎消息
    ├── 被动端口范围
    ├── 空闲超时
    ├── 事件监听器
    ├── 失败登录策略
    └── 关闭指示器

添加可选特性
    ├── 池化监听模式
    ├── UPnP绑定器
    └── FTPS

构建服务器 → 监听端口
```

### stop - 停止服务器

```rust
pub async fn stop(&self)
```

**功能**: 发送关闭信号，优雅停止服务器

### is_running - 检查运行状态

```rust
pub fn is_running(&self) -> bool
```

**功能**: 返回服务器是否正在运行

## 辅助函数

### get_local_ip_for_bind

```rust
fn get_local_ip_for_bind(bind_ip: &str) -> Option<Ipv4Addr>
```

**功能**: 根据绑定IP确定本地IP

**逻辑**:
- 如果绑定0.0.0.0或::，调用get_local_ip获取本机IP
- 否则解析绑定IP

### get_local_ip

```rust
fn get_local_ip() -> std::io::Result<Ipv4Addr>
```

**功能**: 获取本机IP地址

**原理**: 通过UDP连接外部地址（223.5.5.5:53）获取本地绑定的IP

## 被动模式地址选择策略

### 优先级

| 优先级 | 条件 | PASV返回地址 |
|--------|------|--------------|
| 1 | UPnP启用且有外部IP | UPnP公网IP |
| 2 | 配置了masquerade_address | 伪装地址 |
| 3 | 通配符绑定(0.0.0.0/::) | TCP连接的目的IP |
| 4 | 绑定特定IP | 绑定地址 |

### 实现代码

```rust
let passive_host = if config.upnp_enabled
    && let Some(ref upnp_ip) = upnp_external_ip
    && let Ok(ip) = upnp_ip.parse::<Ipv4Addr>()
{
    PassiveHost::Ip(ip)  // 优先级1: UPnP公网IP
} else if let Some(masq_ip) = masquerade_ip
    && !masq_ip.is_unspecified()
{
    PassiveHost::Ip(masq_ip)  // 优先级2: 伪装地址
} else if !is_wildcard_bind(&bind_address) {
    PassiveHost::Ip(bind_ipv4)  // 优先级4: 绑定地址
} else {
    PassiveHost::FromConnection  // 优先级3: TCP目的IP
};
```

### FromConnection模式

当服务器绑定 0.0.0.0 或 :: 且未配置 UPnP/masq 时：

```
客户端请求 127.0.0.1:21 → PASV返回 127.0.0.1
客户端请求 192.168.3.97:21 → PASV返回 192.168.3.97
客户端请求 203.0.113.50:21 → PASV返回 203.0.113.50
```

libunftp 通过 `tcp_stream.local_addr()` 获取客户端连接的实际目的地址。

## FTPS配置

### 证书管理
- 自动检测证书是否存在
- 不存在时自动生成自签名证书
- 证书有效期10年
- 包含localhost和本地IP的SAN

### TLS模式
- `ftps_require_ssl = false`: 允许明文连接，数据通道可选加密
- `ftps_require_ssl = true`: 强制TLS加密

## 性能优化

### 池化监听模式
启用后使用连接池处理被动模式端口，减少端口绑定开销，适合高并发场景。

### 异步设计
所有操作使用tokio异步运行时，支持大量并发连接。

## 错误处理

| 错误场景 | 处理方式 |
|----------|----------|
| 配置路径无效 | 返回错误，拒绝启动 |
| FTP根目录不存在 | 返回错误，拒绝启动 |
| UPnP初始化失败 | 记录警告，继续运行 |
| 证书生成失败 | 返回错误，拒绝启动 |
| 端口监听失败 | 返回错误，服务器停止 |

## 日志记录

服务器在关键节点记录日志：
- 启动时：监听地址、配置信息
- 认证时：登录成功/失败
- 文件操作：上传/下载/删除等
- 会话管理：连接/断开
- UPnP：端口映射状态
- 关闭时：优雅关闭完成
