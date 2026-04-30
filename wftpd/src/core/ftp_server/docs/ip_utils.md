# ip_utils.rs - IP地址工具

## 功能概述

本文件提供IP地址分类和判断的工具函数，用于FTP服务器的网络拓扑分析。

## 设计原理

### IP地址分类

```
IP地址
├── 环回地址 (Loopback)
│   ├── IPv4: 127.0.0.0/8
│   └── IPv6: ::1
│
├── 私有地址 (Private)
│   ├── IPv4:
│   │   ├── 10.0.0.0/8 (A类私有)
│   │   ├── 172.16.0.0/12 (B类私有)
│   │   ├── 192.168.0.0/16 (C类私有)
│   │   └── 169.254.0.0/16 (链路本地)
│   │
│   └── IPv6:
│       ├── fc00::/7 (唯一本地地址 ULA)
│       └── fe80::/10 (链路本地)
│
└── 公网地址 (Public)
    └── 所有其他地址
```

## 核心结构

### IpAddressClass - IP地址分类

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpAddressClass {
    Loopback,  // 环回地址
    Private,   // 私有地址
    Public,    // 公网地址
}
```

## 核心函数

### is_private_ip - 判断私有IP

```rust
pub fn is_private_ip(ip: &IpAddr) -> bool
```

**功能**: 判断任意IP地址是否为私有地址

**实现**:
```rust
match ip {
    IpAddr::V4(ipv4) => is_private_ipv4(ipv4),
    IpAddr::V6(ipv6) => is_private_ipv6(ipv6),
}
```

### is_loopback_ip - 判断环回IP

```rust
pub fn is_loopback_ip(ip: &IpAddr) -> bool
```

**功能**: 判断任意IP地址是否为环回地址

**实现**:
```rust
match ip {
    IpAddr::V4(ipv4) => ipv4.is_loopback(),
    IpAddr::V6(ipv6) => ipv6.is_loopback(),
}
```

### is_private_ipv4 - 判断IPv4私有地址

```rust
pub fn is_private_ipv4(ip: &Ipv4Addr) -> bool
```

**功能**: 判断IPv4地址是否为私有地址

**检测范围**:

| 类型 | 范围 | RFC |
|------|------|-----|
| A类私有 | 10.0.0.0 - 10.255.255.255 | RFC 1918 |
| B类私有 | 172.16.0.0 - 172.31.255.255 | RFC 1918 |
| C类私有 | 192.168.0.0 - 192.168.255.255 | RFC 1918 |
| 链路本地 | 169.254.0.0 - 169.254.255.255 | RFC 3927 |

**实现**:
```rust
pub fn is_private_ipv4(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    
    // 10.0.0.0/8
    if octets[0] == 10 {
        return true;
    }
    
    // 172.16.0.0/12 (172.16.x.x - 172.31.x.x)
    if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31 {
        return true;
    }
    
    // 192.168.0.0/16
    if octets[0] == 192 && octets[1] == 168 {
        return true;
    }
    
    // 169.254.0.0/16 (链路本地)
    if octets[0] == 169 && octets[1] == 254 {
        return true;
    }
    
    false
}
```

### is_private_ipv6 - 判断IPv6私有地址

```rust
pub fn is_private_ipv6(ip: &Ipv6Addr) -> bool
```

**功能**: 判断IPv6地址是否为私有地址

**检测范围**:

| 类型 | 范围 | RFC |
|------|------|-----|
| 唯一本地地址 | fc00::/7 (fc00::-fdff::) | RFC 4193 |
| 链路本地 | fe80::/10 (fe80::-febf::) | RFC 4291 |

**实现**:
```rust
pub fn is_private_ipv6(ip: &Ipv6Addr) -> bool {
    let segments = ip.segments();
    
    // fc00::/7 (ULA)
    if segments[0] == 0xfc00 || segments[0] == 0xfd00 {
        return true;
    }
    
    // fe80::/10 (链路本地)
    if segments[0] == 0xfe80 {
        return true;
    }
    
    false
}
```

### classify_ip_address - 分类IP地址

```rust
pub fn classify_ip_address(ip: &IpAddr) -> IpAddressClass
```

**功能**: 将IP地址分类

**优先级**:
```
1. 环回地址 → IpAddressClass::Loopback
2. 私有地址 → IpAddressClass::Private
3. 其他     → IpAddressClass::Public
```

**实现**:
```rust
pub fn classify_ip_address(ip: &IpAddr) -> IpAddressClass {
    if is_loopback_ip(ip) {
        IpAddressClass::Loopback
    } else if is_private_ip(ip) {
        IpAddressClass::Private
    } else {
        IpAddressClass::Public
    }
}
```

## 使用场景

### 场景1: 被动模式地址选择

```rust
// 判断客户端是否来自私有网络
if is_private_ip(&client_ip) {
    // 返回本地私有IP
    return local_private_ip;
}
```

### 场景2: NAT检测

```rust
// 判断PORT命令IP是否为私有地址
if is_private_ip(&port_ip) && !is_private_ip(&tcp_ip) {
    // 客户端在NAT后面
    return ClientNatStatus::BehindNat;
}
```

### 场景3: 日志分类

```rust
match classify_ip_address(&client_ip) {
    IpAddressClass::Loopback => log::info!("本地连接"),
    IpAddressClass::Private => log::info!("内网连接"),
    IpAddressClass::Public => log::info!("公网连接"),
}
```

## RFC参考

### RFC 1918 - 私有IPv4地址

定义了三个私有IPv4地址块：
- 10.0.0.0/8: A类，大型网络
- 172.16.0.0/12: B类，中型网络
- 192.168.0.0/16: C类，小型网络

### RFC 3927 - 链路本地IPv4地址

定义了169.254.0.0/16用于链路本地通信，当DHCP失败时自动配置。

### RFC 4193 - 唯一本地IPv6地址

定义了fc00::/7用于本地通信，替代已废弃的站点本地地址。

### RFC 4291 - IPv6地址架构

定义了fe80::/10链路本地地址。

## 测试用例

### 测试IPv4私有地址

```rust
#[test]
fn test_is_private_ipv4() {
    // A类私有
    assert!(is_private_ipv4(&Ipv4Addr::new(10, 0, 0, 1)));
    assert!(is_private_ipv4(&Ipv4Addr::new(10, 255, 255, 255)));
    
    // B类私有
    assert!(is_private_ipv4(&Ipv4Addr::new(172, 16, 0, 1)));
    assert!(is_private_ipv4(&Ipv4Addr::new(172, 31, 255, 255)));
    assert!(!is_private_ipv4(&Ipv4Addr::new(172, 15, 0, 1)));  // 边界外
    assert!(!is_private_ipv4(&Ipv4Addr::new(172, 32, 0, 1)));  // 边界外
    
    // C类私有
    assert!(is_private_ipv4(&Ipv4Addr::new(192, 168, 0, 1)));
    assert!(is_private_ipv4(&Ipv4Addr::new(192, 168, 255, 255)));
    
    // 链路本地
    assert!(is_private_ipv4(&Ipv4Addr::new(169, 254, 0, 1)));
    
    // 公网地址
    assert!(!is_private_ipv4(&Ipv4Addr::new(8, 8, 8, 8)));
    assert!(!is_private_ipv4(&Ipv4Addr::new(1, 1, 1, 1)));
}
```

### 测试IPv6私有地址

```rust
#[test]
fn test_is_private_ipv6() {
    // ULA
    assert!(is_private_ipv6(&Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1)));
    assert!(is_private_ipv6(&Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1)));
    
    // 链路本地
    assert!(is_private_ipv6(&Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)));
    
    // 公网地址
    assert!(!is_private_ipv6(&Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)));
}
```

### 测试IP分类

```rust
#[test]
fn test_classify_ip_address() {
    // 环回
    assert_eq!(
        classify_ip_address(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
        IpAddressClass::Loopback
    );
    
    // 私有
    assert_eq!(
        classify_ip_address(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))),
        IpAddressClass::Private
    );
    
    // 公网
    assert_eq!(
        classify_ip_address(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))),
        IpAddressClass::Public
    );
}
```

## 与标准库的区别

### 为什么不使用std::net::Ipv4Addr::is_private

Rust标准库提供了`is_private()`方法，但：
1. 不包含链路本地地址(169.254.0.0/16)
2. 需要统一处理IPv4和IPv6
3. 需要自定义分类逻辑

### 本模块优势

1. **统一接口**: 同时支持IPv4和IPv6
2. **完整覆盖**: 包含链路本地地址
3. **分类功能**: 提供IpAddrClass枚举
4. **可扩展**: 易于添加新的地址类型

## 性能考虑

- 所有判断都是O(1)时间复杂度
- 无内存分配
- 适合高频调用场景
