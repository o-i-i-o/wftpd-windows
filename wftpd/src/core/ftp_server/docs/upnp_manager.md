# upnp_manager.rs - UPnP/IGD管理

## 功能概述

本文件实现UPnP/IGD协议管理，用于自动配置NAT路由器的端口映射，增强FTP被动模式的NAT穿透能力。

## 设计原理

### UPnP/IGD协议

UPnP (Universal Plug and Play) IGD (Internet Gateway Device) 是一种允许程序自动配置路由器端口转发的协议。

```
┌─────────────┐                    ┌─────────────┐
│   应用程序   │ ──── UPnP发现 ───→ │   NAT路由器  │
│             │                    │             │
│  UpnpManager│ ←─── 网关信息 ──── │  IGD服务    │
│             │                    │             │
│             │ ──── 添加映射 ───→ │             │
│             │                    │             │
│             │ ←─── 映射结果 ──── │             │
└─────────────┘                    └─────────────┘
```

### 工作流程

```
1. 发现网关 (search_gateway)
    ↓
2. 获取外部IP (get_external_ip)
    ↓
3. 添加端口映射 (add_port_mapping)
    ↓
4. 使用映射进行通信
    ↓
5. 移除端口映射 (remove_port_mapping)
```

## 核心结构

### UpnpManager

```rust
#[derive(Debug)]
pub struct UpnpManager {
    gateway: RwLock<Option<Gateway>>,  // 网关实例
    enabled: bool,                      // 是否启用
}
```

**字段说明**:
- `gateway`: 缓存发现的网关实例，使用RwLock支持异步访问
- `enabled`: 控制是否执行UPnP操作

## 主要方法

### new - 创建实例

```rust
pub fn new(enabled: bool) -> Self
```

**功能**: 创建UPnP管理器实例

**参数**:
- `enabled`: 是否启用UPnP功能

```rust
UpnpManager {
    gateway: RwLock::new(None),
    enabled,
}
```

### initialize - 初始化

```rust
pub async fn initialize(&self) -> Result<bool>
```

**功能**: 发现并连接UPnP网关

**流程**:
```
检查是否启用
    ↓ 未启用
返回 Ok(false)
    ↓ 已启用
异步执行网关发现
    ↓
成功 → 缓存网关，返回 Ok(true)
失败 → 记录警告，返回 Ok(false)
```

**实现**:
```rust
pub async fn initialize(&self) -> Result<bool> {
    if !self.enabled {
        info!("UPnP/IGD port mapping is disabled");
        return Ok(false);
    }

    let result = tokio::task::spawn_blocking(|| {
        search_gateway(Default::default())
    }).await?;

    match result {
        Ok(gateway) => {
            info!("UPnP/IGD gateway discovered");
            *self.gateway.write().await = Some(gateway);
            Ok(true)
        }
        Err(e) => {
            warn!("UPnP/IGD gateway not found: {}", e);
            Ok(false)
        }
    }
}
```

### add_port_mapping - 添加端口映射

```rust
pub async fn add_port_mapping(
    &self,
    internal_addr: SocketAddrV4,
    lease_duration: u32,
    service: &str,
) -> Result<u16>
```

**功能**: 在NAT路由器上创建端口映射

**参数**:
- `internal_addr`: 内部地址（服务器IP:端口）
- `lease_duration`: 租约时间（秒）
- `service`: 服务描述

**返回**: 外部端口号

**流程**:
```
检查是否启用
    ↓
获取网关实例
    ↓
调用网关添加端口映射
    ↓
成功 → 记录日志，返回端口
失败 → 记录警告，返回内部端口
```

**实现**:
```rust
let result = tokio::task::spawn_blocking(move || {
    gateway.add_port(
        PortMappingProtocol::TCP,
        internal_port,
        internal_addr,
        lease_duration,
        &format!("WFTPG-{}", service),
    )
}).await?;
```

### remove_port_mapping - 移除端口映射

```rust
pub async fn remove_port_mapping(
    &self,
    external_port: u16,
    protocol: PortMappingProtocol,
) -> Result<()>
```

**功能**: 移除NAT路由器上的端口映射

**参数**:
- `external_port`: 外部端口号
- `protocol`: 协议类型（TCP/UDP）

**流程**:
```
检查是否启用
    ↓
获取网关实例
    ↓
调用网关移除端口映射
    ↓
记录结果
```

### get_external_ip - 获取外部IP

```rust
pub async fn get_external_ip(&self) -> Option<String>
```

**功能**: 获取NAT路由器的外部IP地址

**返回**: 外部IP字符串，失败返回None

**流程**:
```
检查是否启用
    ↓
获取网关实例
    ↓
调用网关获取外部IP
    ↓
成功 → 返回 Some(ip_string)
失败 → 返回 None
```

### refresh_all_mappings - 刷新所有映射

```rust
pub async fn refresh_all_mappings(
    &self,
    mappings: &[(u16, SocketAddrV4)],
) -> Result<()>
```

**功能**: 刷新多个端口映射（续租）

**参数**:
- `mappings`: 端口映射列表 [(外部端口, 内部地址)]

**用途**: 租约到期前续期

## 使用示例

### 基本使用

```rust
// 创建管理器
let manager = UpnpManager::new(true);

// 初始化（发现网关）
if manager.initialize().await? {
    println!("UPnP网关已发现");
    
    // 获取外部IP
    if let Some(ip) = manager.get_external_ip().await {
        println!("外部IP: {}", ip);
    }
    
    // 添加端口映射
    let internal = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 50000);
    let external_port = manager.add_port_mapping(
        internal,
        3600,  // 1小时
        "ftp-passive-50000"
    ).await?;
    
    // 使用映射...
    
    // 移除映射
    manager.remove_port_mapping(external_port, PortMappingProtocol::TCP).await?;
}
```

### 与FTP服务器集成

```rust
// 在服务器启动时
let upnp_manager = Arc::new(UpnpManager::new(config.ftp.upnp_enabled));
if let Err(e) = upnp_manager.initialize().await {
    tracing::warn!("UPnP初始化失败: {}", e);
}

// 获取外部IP用于PASV响应
let external_ip = upnp_manager.get_external_ip().await;

// 创建绑定器
let binder = UpnpBinderBuilder::new()
    .upnp_manager(upnp_manager.clone())
    .local_ip(local_ip)
    .build();
```

## 错误处理策略

### 优雅降级

所有UPnP操作失败都不会导致服务器崩溃：

| 操作 | 失败处理 |
|------|----------|
| 初始化 | 记录警告，继续运行 |
| 添加映射 | 记录警告，使用内部端口 |
| 移除映射 | 记录警告，忽略 |
| 获取外部IP | 返回None，使用备用方案 |

### 日志级别

| 情况 | 级别 |
|------|------|
| 网关发现成功 | INFO |
| 网关发现失败 | WARN |
| 端口映射成功 | INFO |
| 端口映射失败 | WARN |
| 外部IP获取成功 | INFO |
| 外部IP获取失败 | WARN |

## 异步设计

### 为什么使用spawn_blocking

UPnP操作（网关发现、端口映射）是阻塞操作，使用`spawn_blocking`在独立线程执行：

```rust
tokio::task::spawn_blocking(move || {
    search_gateway(Default::default())
}).await?
```

**好处**:
1. 不阻塞异步运行时
2. 允许其他任务并发执行
3. 保持API异步友好

## 安全考虑

### UPnP风险

1. **自动开放端口**: 可能被恶意程序利用
2. **无认证**: 传统UPnP没有认证机制
3. **暴露内网**: 可能暴露内网服务

### 安全建议

1. **仅在可信网络启用**: 内网环境可安全使用
2. **限制端口范围**: 只开放必要端口
3. **监控映射状态**: 定期检查开放的端口
4. **使用UPnP2**: 支持认证的新版本

### 本实现的安全措施

1. **可配置开关**: 默认可禁用
2. **明确的服务描述**: 映射带有"WFTPG-"前缀
3. **自动清理**: 绑定器销毁时自动移除映射
4. **租约时间**: 映射有有效期，不会永久开放

## 兼容性

### 支持的路由器

- 支持UPnP/IGD v1.0的路由器
- 支持UPnP/IGD v2.0的路由器
- 大多数家用路由器默认启用

### 不支持的情况

- 企业级路由器（通常禁用UPnP）
- 严格安全策略的网络
- 多层NAT环境

## 故障排除

### 常见问题

| 问题 | 可能原因 | 解决方案 |
|------|----------|----------|
| 网关发现失败 | 路由器禁用UPnP | 启用路由器UPnP |
| 端口映射失败 | 端口被占用 | 更换端口范围 |
| 外部IP获取失败 | 网关不支持 | 使用配置的外部IP |
| 映射不生效 | 防火墙阻止 | 检查防火墙设置 |

### 调试建议

1. 检查路由器UPnP设置
2. 查看服务器日志
3. 使用UPnP调试工具
4. 验证端口映射状态
