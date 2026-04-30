# active_mode.rs - 主动模式NAT检测

## 功能概述

本文件实现FTP主动模式下的客户端NAT检测功能，通过比较PORT命令中的IP地址与实际TCP连接IP来判断客户端是否位于NAT后面。

## 设计原理

### FTP主动模式工作流程

```
1. 客户端连接服务器21端口
2. 客户端发送PORT命令（包含IP:端口）
3. 服务器主动连接客户端指定的IP:端口
4. 数据传输开始
```

### NAT检测原理

当客户端位于NAT后面时：
- PORT命令中的IP是内网IP（如192.168.x.x）
- 服务器看到的TCP连接IP是公网IP（经过NAT转换）

```
客户端内网IP: 192.168.1.100
    ↓ NAT转换
服务器看到IP: 203.0.113.50
    ↓
PORT命令IP: 192.168.1.100 ≠ TCP连接IP: 203.0.113.50
    ↓
结论: 客户端在NAT后面
```

## 核心结构

### ClientNatStatus - NAT状态枚举

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientNatStatus {
    Direct,      // 直连，客户端无NAT
    BehindNat,   // 客户端在NAT后面
}
```

### ActiveModeInfo - 主动模式信息

```rust
#[derive(Debug, Clone)]
pub struct ActiveModeInfo {
    pub port_ip: IpAddr,       // PORT命令中的IP
    pub tcp_ip: IpAddr,        // TCP连接的实际IP
    pub nat_status: ClientNatStatus,  // NAT状态
}
```

### ActiveModeConfig - 配置

```rust
#[derive(Debug, Clone)]
pub struct ActiveModeConfig {
    pub allow_nat_clients: bool,          // 是否允许NAT客户端
    pub validate_port_ip_reachable: bool, // 是否验证PORT IP可达性（预留）
}
```

**默认配置**:
```rust
impl Default for ActiveModeConfig {
    fn default() -> Self {
        ActiveModeConfig {
            allow_nat_clients: true,           // 默认允许NAT客户端
            validate_port_ip_reachable: false, // 默认不验证可达性
        }
    }
}
```

## 核心函数

### detect_client_nat - 检测客户端NAT

```rust
pub fn detect_client_nat(
    port_command_ip: IpAddr,
    tcp_connection_ip: IpAddr
) -> ClientNatStatus
```

**功能**: 判断客户端是否在NAT后面

**算法**:
```
if PORT命令IP == TCP连接IP:
    return Direct（直连）
else:
    return BehindNat（NAT后面）
```

**示例**:
```rust
// 直连情况
let status = detect_client_nat(
    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
);
assert_eq!(status, ClientNatStatus::Direct);

// NAT情况
let status = detect_client_nat(
    IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),      // 内网IP
    IpAddr::V4(Ipv4Addr::new(203, 0, 113, 50)),  // 公网IP
);
assert_eq!(status, ClientNatStatus::BehindNat);
```

### should_accept_port_command - 是否接受PORT命令

```rust
pub fn should_accept_port_command(
    port_ip: IpAddr,
    tcp_ip: IpAddr,
    allow_nat_clients: bool,
) -> bool
```

**功能**: 根据配置决定是否接受PORT命令

**决策流程**:
```
检测NAT状态
    ↓
┌─────────────────┐
│ 直连？          │──是──→ 接受
└─────────────────┘
    │否
    ▼
┌─────────────────┐
│ 允许NAT客户端？ │──是──→ 接受（记录日志）
└─────────────────┘
    │否
    ▼
拒绝（记录警告）
```

**日志输出**:
- 允许NAT客户端: `INFO: Active mode: Client behind NAT detected (PORT IP: x, TCP IP: y), allowing`
- 拒绝NAT客户端: `WARN: Active mode: Client behind NAT detected (PORT IP: x, TCP IP: y), rejected`

### analyze_active_mode_connection - 分析主动模式连接

```rust
pub fn analyze_active_mode_connection(
    port_ip: IpAddr,
    tcp_ip: IpAddr,
    config: &ActiveModeConfig,
) -> ActiveModeInfo
```

**功能**: 全面分析主动模式连接

**流程**:
```
检测NAT状态
    ↓
如果NAT客户端被拒绝 → 记录警告
    ↓
返回ActiveModeInfo
```

## 使用场景

### 场景1: 安全策略严格

```rust
let config = ActiveModeConfig {
    allow_nat_clients: false,  // 不允许NAT客户端
    ..Default::default()
};

// NAT客户端会被拒绝
if !should_accept_port_command(port_ip, tcp_ip, config.allow_nat_clients) {
    return Err("NAT clients not allowed");
}
```

### 场景2: 兼容性优先

```rust
let config = ActiveModeConfig {
    allow_nat_clients: true,  // 允许NAT客户端
    ..Default::default()
};

// 所有客户端都被接受
if should_accept_port_command(port_ip, tcp_ip, config.allow_nat_clients) {
    // 处理PORT命令
}
```

## 安全考虑

### 为什么可能拒绝NAT客户端

1. **安全风险**: NAT客户端可能尝试连接内网地址
2. **数据泄露**: 可能暴露内网结构
3. **攻击向量**: 可能用于端口扫描

### 为什么通常允许NAT客户端

1. **兼容性**: 大多数客户端在NAT后面
2. **实用性**: 拒绝NAT客户端会导致大量用户无法使用
3. **现代实践**: 使用被动模式更安全

### 最佳实践

```
推荐配置:
- 主动模式: 允许NAT客户端（兼容性）
- 被动模式: 优先使用（更安全）
- FTPS: 启用加密（数据安全）
```

## 与被动模式的对比

| 特性 | 主动模式 | 被动模式 |
|------|----------|----------|
| 数据连接方向 | 服务器→客户端 | 客户端→服务器 |
| NAT兼容性 | 差 | 好 |
| 防火墙友好 | 差 | 好 |
| 安全性 | 较低 | 较高 |
| NAT检测 | 需要 | 不需要 |

## 扩展功能（预留）

### validate_port_ip_reachable

**功能**: 验证PORT命令中的IP是否可达

**实现思路**:
```
1. 尝试TCP连接PORT命令中的IP:端口
2. 连接成功 → IP可达
3. 连接失败 → IP不可达
```

**用途**: 防止客户端提供虚假IP地址

## 测试用例

### 测试直连检测
```rust
#[test]
fn test_detect_client_nat_direct() {
    let port_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
    let tcp_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
    assert_eq!(detect_client_nat(port_ip, tcp_ip), ClientNatStatus::Direct);
}
```

### 测试NAT检测
```rust
#[test]
fn test_detect_client_nat_behind_nat() {
    let port_ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
    let tcp_ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 50));
    assert_eq!(detect_client_nat(port_ip, tcp_ip), ClientNatStatus::BehindNat);
}
```

### 测试PORT命令接受逻辑
```rust
#[test]
fn test_should_accept_port_command() {
    let port_ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));
    let tcp_ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 50));
    
    // 允许NAT客户端
    assert!(should_accept_port_command(port_ip, tcp_ip, true));
    
    // 不允许NAT客户端
    assert!(!should_accept_port_command(port_ip, tcp_ip, false));
}
```

## 集成示例

```rust
use crate::core::ftp_server::{detect_client_nat, should_accept_port_command, ClientNatStatus};

// 处理PORT命令
fn handle_port_command(port_ip: IpAddr, tcp_ip: IpAddr, config: &ActiveModeConfig) -> Result<()> {
    // 检测NAT状态
    let nat_status = detect_client_nat(port_ip, tcp_ip);
    
    // 决定是否接受
    if !should_accept_port_command(port_ip, tcp_ip, config.allow_nat_clients) {
        return Err(anyhow!("NAT client rejected by policy"));
    }
    
    // 处理PORT命令...
    Ok(())
}
```
