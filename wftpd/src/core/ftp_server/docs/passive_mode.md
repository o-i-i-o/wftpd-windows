# passive_mode.rs - 被动模式地址选择

## 功能概述

本文件实现FTP被动模式（PASV）的IP地址选择逻辑，提供IP地址分类和本地地址获取功能。

## 设计原理

### FTP被动模式工作流程

```
1. 客户端连接服务器21端口
2. 客户端发送PASV命令
3. 服务器返回IP:端口
4. 客户端主动连接服务器返回的IP:端口
5. 数据传输开始
```

### 地址选择策略（在unftp.rs中实现）

使用libunftp的`PassiveHost`枚举：

```rust
pub enum PassiveHost {
    FromConnection,    // 使用TCP连接的本地端IP（动态）
    Ip(Ipv4Addr),      // 使用固定IP
    Dns(String),       // 解析DNS名称
}
```

### 选择优先级

| 优先级 | 条件 | PASV返回地址 |
|--------|------|--------------|
| 1 | UPnP启用且有外部IP | UPnP公网IP |
| 2 | 配置了masquerade_address | 伪装地址 |
| 3 | 通配符绑定(0.0.0.0/::) | TCP连接的目的IP |
| 4 | 绑定特定IP | 绑定地址 |

### FromConnection模式

当服务器绑定 0.0.0.0 或 :: 且未配置 UPnP/masq 时：

```
客户端请求 127.0.0.1:21 → PASV返回 127.0.0.1
客户端请求 192.168.3.97:21 → PASV返回 192.168.3.97
客户端请求 203.0.113.50:21 → PASV返回 203.0.113.50
```

libunftp 通过 `tcp_stream.local_addr()` 获取客户端连接的实际目的地址。
```

## 核心结构

### PassiveAddressSource - 地址来源枚举

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassiveAddressSource {
    Upnp,          // UPnP网关获取
    Masquerade,    // 配置的伪装地址
    BindAddress,   // 指定绑定地址
    Loopback,      // 环回地址
    Private,       // 私有网络地址
    Public,        // 公网地址
    Ipv6Fallback,  // IPv6绑定的IPv4回退
}
```

### PassiveModeConfig - 配置

```rust
#[derive(Debug, Clone)]
pub struct PassiveModeConfig {
    pub upnp_enabled: bool,              // 是否启用UPnP
    pub upnp_external_ip: Option<Ipv4Addr>, // UPnP外部IP
    pub masquerade_address: Option<Ipv4Addr>, // 伪装地址
    pub bind_address: IpAddr,            // 绑定地址
    pub server_local_ips: Vec<Ipv4Addr>, // 服务器本地IP列表
}
```

### PassiveAddressResult - 选择结果

```rust
#[derive(Debug, Clone)]
pub struct PassiveAddressResult {
    pub address: Ipv4Addr,              // 选择的IP地址
    pub source: PassiveAddressSource,   // 地址来源
    pub use_epsv_recommended: bool,     // 是否建议使用EPSV
}
```

### BindAddressType - 绑定地址类型

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindAddressType {
    Wildcard,       // 0.0.0.0 或 ::
    SpecificIpv4,   // 指定IPv4
    SpecificIpv6,   // 指定IPv6
}
```

### ConnectionSource - 连接来源类型

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionSource {
    Loopback,       // 环回连接
    PrivateNetwork, // 私有网络
    PublicNetwork,  // 公网
}
```

### PassiveModeInfo - 完整信息

```rust
#[derive(Debug, Clone)]
pub struct PassiveModeInfo {
    pub listen_address: IpAddr,              // 监听地址
    pub pasv_response_ip: Ipv4Addr,          // PASV响应IP
    pub address_source: PassiveAddressSource, // 地址来源
    pub connection_source: Option<ConnectionSource>, // 连接来源
    pub use_epsv_recommended: bool,          // 是否建议EPSV
}
```

### LocalIpAddress - 本地IP集合

```rust
#[derive(Debug, Clone)]
pub struct LocalIpAddress {
    pub ipv4: Vec<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Addr>,
}
```

## 核心函数

### select_passive_address - 选择被动地址

```rust
pub fn select_passive_address(
    config: &PassiveModeConfig,
    client_ip: IpAddr,
    connection_local_ip: Option<Ipv4Addr>,
) -> PassiveAddressResult
```

**选择流程**:

#### 1. UPnP优先
```rust
if config.upnp_enabled && let Some(upnp_ip) = config.upnp_external_ip {
    return PassiveAddressResult {
        address: upnp_ip,
        source: PassiveAddressSource::Upnp,
        use_epsv_recommended: false,
    };
}
```

#### 2. 伪装地址次之
```rust
if let Some(masq_ip) = config.masquerade_address && !masq_ip.is_unspecified() {
    return PassiveAddressResult {
        address: masq_ip,
        source: PassiveAddressSource::Masquerade,
        use_epsv_recommended: false,
    };
}
```

#### 3. 分类绑定地址
```rust
let bind_type = classify_bind_address(&config.bind_address);
```

#### 4. 根据绑定类型处理

**指定IPv4**:
```rust
BindAddressType::SpecificIpv4 => {
    PassiveAddressResult {
        address: bind_ipv4,
        source: PassiveAddressSource::BindAddress,
        use_epsv_recommended: false,
    }
}
```

**指定IPv6**:
```rust
BindAddressType::SpecificIpv6 => {
    // PASV不支持IPv6，需要回退到IPv4
    let fallback_ip = find_ipv4_fallback(&config.server_local_ips, &client_ip);
    PassiveAddressResult {
        address: fallback_ip,
        source: PassiveAddressSource::Ipv6Fallback,
        use_epsv_recommended: matches!(client_ip, IpAddr::V6(_)),
    }
}
```

**通配符绑定**:
```rust
BindAddressType::Wildcard => handle_wildcard_bind(config, &client_ip, connection_local_ip)
```

### handle_wildcard_bind - 处理通配符绑定

```rust
fn handle_wildcard_bind(
    config: &PassiveModeConfig,
    client_ip: &IpAddr,
    connection_local_ip: Option<Ipv4Addr>,
) -> PassiveAddressResult
```

**分类处理**:

| 客户端来源 | 返回地址 | 来源标记 |
|------------|----------|----------|
| Loopback | 127.0.0.1 | Loopback |
| PrivateNetwork | 连接本地IP或匹配的私有IP | Private |
| PublicNetwork | 连接本地IP或公网IP | Public |

### classify_bind_address - 分类绑定地址

```rust
pub fn classify_bind_address(bind_addr: &IpAddr) -> BindAddressType
```

**分类逻辑**:
```rust
match bind_addr {
    IpAddr::V4(ip) if ip.is_unspecified() => BindAddressType::Wildcard,  // 0.0.0.0
    IpAddr::V6(ip) if ip.is_unspecified() => BindAddressType::Wildcard,  // ::
    IpAddr::V4(_) => BindAddressType::SpecificIpv4,
    IpAddr::V6(_) => BindAddressType::SpecificIpv6,
}
```

### classify_connection_source - 分类连接来源

```rust
pub fn classify_connection_source(client_ip: &IpAddr) -> ConnectionSource
```

**分类逻辑**:
```rust
// IPv4
if ip.is_loopback() → ConnectionSource::Loopback
else if is_private_ipv4(ip) → ConnectionSource::PrivateNetwork
else → ConnectionSource::PublicNetwork

// IPv6
if ip.is_loopback() → ConnectionSource::Loopback
else if is_private_ipv6(ip) → ConnectionSource::PrivateNetwork
else → ConnectionSource::PublicNetwork
```

### get_local_ipv4_addresses - 获取本地IPv4地址

```rust
pub fn get_local_ipv4_addresses() -> Vec<Ipv4Addr>
```

**Windows实现**:
```rust
Command::new("ipconfig").args(["/all"]).output()
    ↓
解析输出，提取IPv4地址
    ↓
格式: "IPv4 地址 . . . : 192.168.1.100(首选)"
```

**Linux实现**:
```rust
读取 /sys/class/net/*/address
```

### get_local_ip_addresses - 获取所有本地IP

```rust
pub fn get_local_ip_addresses() -> LocalIpAddress
```

**返回**: 包含IPv4和IPv6地址列表

### format_listen_addresses - 格式化监听地址

```rust
pub fn format_listen_addresses(bind_ip: &str, port: u16) -> String
```

**输出格式**:
- 通配符绑定: `0.0.0.0:21 / [::]:21 (all interfaces: 192.168.1.100:21, [fe80::1]:21)`
- 指定绑定: `192.168.1.100:21` 或 `[::1]:21`

## IPv6兼容性

### PASV与IPv6

**问题**: PASV命令（RFC 959）只支持IPv4地址格式

**解决方案**:
1. IPv6客户端应使用EPSV命令（RFC 2428）
2. 当服务器绑定IPv6时，返回IPv4回退地址
3. 设置`use_epsv_recommended = true`提示使用EPSV

### EPSV优势

```
PASV响应: 227 Entering Passive Mode (192,168,1,100,4,1)
         → 包含IP地址，只支持IPv4

EPSV响应: 229 Entering Extended Passive Mode (|||1025|)
         → 只包含端口，支持任何IP版本
```

## 辅助函数

### find_matching_local_ip

```rust
fn find_matching_local_ip(local_ips: &[Ipv4Addr], source: ConnectionSource) -> Ipv4Addr
```

**功能**: 根据连接来源找到匹配的本地IP

### find_ipv4_fallback

```rust
fn find_ipv4_fallback(local_ips: &[Ipv4Addr], client_ip: &IpAddr) -> Ipv4Addr
```

**功能**: 为IPv6绑定找到IPv4回退地址

### is_wildcard_bind

```rust
pub fn is_wildcard_bind(bind_addr: &IpAddr) -> bool
```

**功能**: 判断是否通配符绑定

### is_ipv6_bind

```rust
pub fn is_ipv6_bind(bind_addr: &IpAddr) -> bool
```

**功能**: 判断是否IPv6绑定

## 使用示例

```rust
use crate::core::ftp_server::{PassiveModeConfig, select_passive_address};

// 配置
let config = PassiveModeConfig {
    upnp_enabled: true,
    upnp_external_ip: Some(Ipv4Addr::new(203, 0, 113, 50)),
    masquerade_address: Some(Ipv4Addr::new(192, 168, 1, 1)),
    bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
    server_local_ips: vec![Ipv4Addr::new(192, 168, 1, 100)],
};

// 选择地址
let result = select_passive_address(
    &config,
    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 200)),
    Some(Ipv4Addr::new(192, 168, 1, 100)),
);

println!("PASV地址: {}", result.address);  // 203.0.113.50 (UPnP优先)
println!("来源: {:?}", result.source);     // Upnp
```

## 测试用例

### 测试UPnP优先级
```rust
#[test]
fn test_select_passive_address_upnp_priority() {
    let config = PassiveModeConfig {
        upnp_enabled: true,
        upnp_external_ip: Some(Ipv4Addr::new(203, 0, 113, 50)),
        masquerade_address: Some(Ipv4Addr::new(192, 168, 1, 1)),
        // ...
    };
    let result = select_passive_address(&config, client_ip, None);
    assert_eq!(result.address, Ipv4Addr::new(203, 0, 113, 50));
    assert_eq!(result.source, PassiveAddressSource::Upnp);
}
```

### 测试伪装地址优先级
```rust
#[test]
fn test_select_passive_address_masq_priority() {
    let config = PassiveModeConfig {
        upnp_enabled: true,
        upnp_external_ip: None,  // UPnP无外部IP
        masquerade_address: Some(Ipv4Addr::new(203, 0, 113, 50)),
        // ...
    };
    let result = select_passive_address(&config, client_ip, None);
    assert_eq!(result.source, PassiveAddressSource::Masquerade);
}
```

### 测试环回连接
```rust
#[test]
fn test_select_passive_address_wildcard_loopback() {
    let config = PassiveModeConfig {
        upnp_enabled: false,
        masquerade_address: None,
        bind_address: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        // ...
    };
    let result = select_passive_address(&config, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), None);
    assert_eq!(result.address, Ipv4Addr::new(127, 0, 0, 1));
    assert_eq!(result.source, PassiveAddressSource::Loopback);
}
```
