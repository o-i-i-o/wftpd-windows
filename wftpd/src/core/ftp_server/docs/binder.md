# binder.rs - UPnP端口绑定器

## 功能概述

本文件实现libunftp的Binder trait扩展，为FTP被动模式添加UPnP端口映射支持。

## 设计原理

### UPnP与FTP被动模式

```
┌─────────────────────────────────────────────────────┐
│                    互联网                            │
│                                                     │
│              客户端 (公网IP)                         │
│                    │                                │
└────────────────────┼────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────┐
│                  NAT路由器                           │
│                                                     │
│   外部IP: 203.0.113.50                              │
│   内部IP: 192.168.1.1                               │
│                                                     │
│   UPnP端口映射:                                     │
│   203.0.113.50:50000 → 192.168.1.100:50000        │
└─────────────────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────┐
│                  FTP服务器                           │
│                                                     │
│   内部IP: 192.168.1.100                             │
│   PASV端口: 50000                                   │
└─────────────────────────────────────────────────────┘
```

### 工作流程

```
1. 客户端发送PASV命令
2. 服务器绑定被动端口
3. UpnpBinder自动创建端口映射
4. 服务器返回外部IP:端口
5. 客户端连接外部IP:端口
6. NAT路由器转发到服务器
```

## 核心结构

### UpnpBinder - UPnP绑定器

```rust
#[derive(Debug)]
pub struct UpnpBinder {
    upnp_manager: Arc<UpnpManager>,           // UPnP管理器
    local_ip: Ipv4Addr,                       // 本地IP
    passive_ports: Option<RangeInclusive<u16>>, // 被动端口范围
    mapped_ports: Mutex<Vec<u16>>,            // 已映射端口列表
}
```

### UpnpBinderBuilder - 构建器

```rust
pub struct UpnpBinderBuilder {
    upnp_manager: Option<Arc<UpnpManager>>,
    local_ip: Option<Ipv4Addr>,
    passive_ports: Option<RangeInclusive<u16>>,
}
```

**使用构建器模式**:
```rust
let binder = UpnpBinderBuilder::new()
    .upnp_manager(manager)
    .local_ip(Ipv4Addr::new(192, 168, 1, 100))
    .passive_ports(50000..=51000)
    .build();
```

## Binder trait实现

### bind方法

```rust
#[async_trait]
impl Binder for UpnpBinder {
    async fn bind(
        &mut self,
        local_addr: IpAddr,
        passive_ports: RangeInclusive<u16>,
    ) -> io::Result<TcpSocket>
}
```

**执行流程**:

#### 1. 创建TCP Socket
```rust
let socket = match local_addr {
    IpAddr::V4(_) => TcpSocket::new_v4()?,
    IpAddr::V6(_) => TcpSocket::new_v6()?,
};
socket.set_reuseaddr(true)?;
```

#### 2. 绑定端口
```rust
let bind_addr = SocketAddr::new(local_addr, 0);  // 0表示随机端口
socket.bind(bind_addr)?;

let bound_addr = socket.local_addr()?;
let port = bound_addr.port();
```

#### 3. 验证端口范围
```rust
let effective_ports = self.passive_ports.clone().unwrap_or(passive_ports);
if !effective_ports.contains(&port) {
    tracing::warn!(
        "Bound port {} is outside configured passive port range {:?}",
        port, effective_ports
    );
}
```

#### 4. 添加UPnP映射
```rust
match self.upnp_manager.add_port_mapping(
    SocketAddrV4::new(self.local_ip, port),
    3600,  // 租约时间：1小时
    &format!("ftp-passive-{}", port),
).await {
    Ok(external_port) => {
        self.mapped_ports.lock().push(external_port);
    }
    Err(e) => {
        tracing::warn!("Failed to add UPnP port mapping: {}", e);
    }
}
```

#### 5. 返回Socket
```rust
Ok(socket)
```

## 资源清理

### Drop实现

```rust
impl Drop for UpnpBinder {
    fn drop(&mut self) {
        let ports: Vec<u16> = self.mapped_ports.lock().drain(..).collect();
        if ports.is_empty() {
            return;
        }
        
        let upnp_manager = Arc::clone(&self.upnp_manager);
        tokio::spawn(async move {
            for port in ports {
                if let Err(e) = upnp_manager
                    .remove_port_mapping(port, PortMappingProtocol::TCP)
                    .await
                {
                    tracing::warn!("Failed to remove UPnP port mapping: {}", e);
                }
            }
        });
    }
}
```

**清理流程**:
```
UpnpBinder被销毁
    ↓
获取所有已映射端口
    ↓
异步任务中移除每个映射
    ↓
日志记录结果
```

## UpnpBinderBuilder实现

### 构建器方法

```rust
impl UpnpBinderBuilder {
    pub fn new() -> Self { /* ... */ }
    
    pub fn upnp_manager(mut self, manager: Arc<UpnpManager>) -> Self {
        self.upnp_manager = Some(manager);
        self
    }
    
    pub fn local_ip(mut self, ip: Ipv4Addr) -> Self {
        self.local_ip = Some(ip);
        self
    }
    
    pub fn passive_ports(mut self, ports: RangeInclusive<u16>) -> Self {
        self.passive_ports = Some(ports);
        self
    }
    
    pub fn build(self) -> Option<UpnpBinder> {
        match (self.upnp_manager, self.local_ip) {
            (Some(manager), Some(ip)) => Some(UpnpBinder::new(
                manager, ip, self.passive_ports
            )),
            _ => None,
        }
    }
}
```

## 使用示例

### 基本使用

```rust
use crate::core::ftp_server::{UpnpBinderBuilder, UpnpManager};

// 创建UPnP管理器
let upnp_manager = Arc::new(UpnpManager::new(true));
upnp_manager.initialize().await?;

// 创建绑定器
let binder = UpnpBinderBuilder::new()
    .upnp_manager(upnp_manager)
    .local_ip(Ipv4Addr::new(192, 168, 1, 100))
    .passive_ports(50000..=51000)
    .build();

// 配置到服务器
if let Some(binder) = binder {
    server_builder = server_builder.binder(binder);
}
```

### 与libunftp集成

```rust
let server = ServerBuilder::new(storage_factory)
    .passive_ports(50000..=51000)
    .binder(UpnpBinderBuilder::new()
        .upnp_manager(upnp_manager)
        .local_ip(local_ip)
        .passive_ports(50000..=51000)
        .build()
        .unwrap())
    .build()?;
```

## 错误处理

| 错误场景 | 处理方式 |
|----------|----------|
| Socket创建失败 | 返回错误 |
| 端口绑定失败 | 返回错误 |
| 端口超出范围 | 记录警告，继续 |
| UPnP映射失败 | 记录警告，继续 |
| 映射移除失败 | 记录警告 |

## 性能考虑

### 异步清理

端口映射移除在异步任务中执行，不阻塞主流程：
```rust
tokio::spawn(async move {
    for port in ports {
        upnp_manager.remove_port_mapping(port, ...).await;
    }
});
```

### 端口跟踪

使用Mutex保护已映射端口列表，支持多线程安全访问。

## 日志记录

| 事件 | 级别 | 内容 |
|------|------|------|
| 端口超出范围 | WARN | 端口号、范围 |
| 映射成功 | DEBUG | 内部端口、外部端口 |
| 映射失败 | WARN | 端口号、错误 |
| 移除成功 | INFO | 端口号 |
| 移除失败 | WARN | 端口号、错误 |

## 注意事项

### UPnP限制

1. **路由器支持**: 需要路由器支持UPnP/IGD协议
2. **安全性**: UPnP可能被滥用，建议仅在可信网络启用
3. **租约时间**: 映射有有效期，需要定期刷新

### 最佳实践

1. **仅在需要时启用**: 如果服务器有公网IP，不需要UPnP
2. **限制端口范围**: 使用明确的被动端口范围
3. **监控映射状态**: 定期检查映射是否有效

### 故障回退

当UPnP不可用时，服务器仍可正常工作：
- 使用本地IP作为PASV响应
- 客户端需要能够访问该IP
- 适用于内网环境
