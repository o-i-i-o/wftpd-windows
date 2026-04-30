# mod.rs - FTP模块入口

## 功能概述

本文件是FTP服务器模块的入口点，负责组织和管理所有子模块，并导出公共API供外部使用。

## 模块结构

```
ftp_server/
├── mod.rs           ← 当前文件（模块入口）
├── unftp.rs         # 主FTP服务器实现
├── auth.rs          # 认证和会话管理
├── storage.rs       # 存储后端（支持配额）
├── active_mode.rs   # 主动模式NAT检测
├── passive_mode.rs  # 被动模式地址选择
├── ip_utils.rs      # IP地址分类工具
├── binder.rs        # UPnP端口绑定器
├── upnp_manager.rs  # UPnP/IGD管理
├── cert_gen.rs      # TLS证书生成
└── listeners.rs     # 事件监听器
```

## 模块可见性

| 模块 | 可见性 | 说明 |
|------|--------|------|
| `active_mode` | 私有 | 仅模块内部使用 |
| `auth` | 私有 | 仅模块内部使用 |
| `binder` | 私有 | 仅模块内部使用 |
| `cert_gen` | crate公开 | 供其他模块使用 |
| `ip_utils` | 私有 | 仅模块内部使用 |
| `listeners` | 私有 | 仅模块内部使用 |
| `passive_mode` | 私有 | 仅模块内部使用 |
| `storage` | 私有 | 仅模块内部使用 |
| `unftp` | 私有 | 仅模块内部使用 |
| `upnp_manager` | 公开 | 供外部直接使用 |

## 公共导出

### 主动模式相关
```rust
pub use active_mode::{
    ActiveModeConfig,      // 主动模式配置
    ActiveModeInfo,        // 主动模式连接信息
    ClientNatStatus,       // 客户端NAT状态枚举
    analyze_active_mode_connection,  // 分析主动模式连接
    detect_client_nat,     // 检测客户端NAT
    should_accept_port_command,      // 判断是否接受PORT命令
};
```

### 认证相关
```rust
pub use auth::{
    SessionTracker,        // 会话跟踪器
    WftpdAuthenticator,    // 认证器
    WftpdUser,             // 用户详情结构
    WftpdUserDetailProvider, // 用户详情提供者
};
```

### UPnP绑定相关
```rust
pub use binder::{
    UpnpBinder,            // UPnP绑定器
    UpnpBinderBuilder,     // UPnP绑定器构建器
};
```

### IP工具相关
```rust
pub use ip_utils::{
    IpAddressClass,        // IP地址分类枚举
    classify_ip_address,   // 分类IP地址
    is_loopback_ip,        // 判断是否为环回地址
    is_private_ip,         // 判断是否为私有地址
    is_private_ipv4,       // 判断IPv4私有地址
    is_private_ipv6,       // 判断IPv6私有地址
};
```

### 事件监听相关
```rust
pub use listeners::{
    FtpDataListener,       // 数据事件监听器
    FtpPresenceListener,   // 在线状态监听器
};
```

### 被动模式相关
```rust
pub use passive_mode::{
    BindAddressType,       // 绑定地址类型
    ConnectionSource,      // 连接来源类型
    LocalIpAddress,        // 本地IP地址集合
    PassiveAddressResult,  // 被动地址选择结果
    PassiveAddressSource,  // 被动地址来源
    PassiveModeConfig,     // 被动模式配置
    PassiveModeInfo,       // 被动模式信息
    build_passive_mode_info,    // 构建被动模式信息
    classify_bind_address,      // 分类绑定地址
    classify_connection_source, // 分类连接来源
    determine_listen_address,   // 确定监听地址
    format_listen_addresses,    // 格式化监听地址
    get_local_ip_addresses,     // 获取本地IP地址
    get_local_ipv4_addresses,   // 获取本地IPv4地址
    is_ipv6_bind,               // 判断是否IPv6绑定
    is_wildcard_bind,           // 判断是否通配符绑定
    select_passive_address,     // 选择被动地址
};
```

### 存储相关
```rust
pub use storage::{
    QuotaFilesystem,       // 支持配额的文件系统
};
```

### 服务器相关
```rust
pub use unftp::{
    FtpServer,             // FTP服务器实例
};
```

## 设计原则

1. **封装性**: 内部实现细节通过私有模块隐藏
2. **API简洁**: 只导出外部需要使用的类型和函数
3. **模块化**: 各子模块职责单一，相互独立
4. **可扩展**: 新功能可以作为新模块添加

## 使用示例

```rust
use crate::core::ftp_server::{FtpServer, WftpdUser, QuotaFilesystem};

// 创建FTP服务器
let ftp_server = FtpServer::new(config, user_manager);

// 启动服务器
ftp_server.start().await?;

// 检查运行状态
if ftp_server.is_running() {
    println!("FTP服务器正在运行");
}

// 停止服务器
ftp_server.stop().await;
```
