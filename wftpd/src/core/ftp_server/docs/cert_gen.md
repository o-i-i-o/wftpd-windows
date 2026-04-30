# cert_gen.rs - TLS证书生成

## 功能概述

本文件实现FTP服务器所需的自签名TLS证书生成功能，用于FTPS（FTP over TLS）加密通信。

## 设计原理

### FTPS概述

FTPS是FTP的安全版本，使用TLS/SSL加密：
- **显式FTPS (FTPES)**: 客户端请求升级到TLS
- **隐式FTPS**: 强制TLS连接

### 证书用途

```
客户端连接
    ↓
服务器发送证书
    ↓
客户端验证证书
    ↓
建立加密通道
    ↓
安全数据传输
```

## 核心函数

### generate_self_signed_cert - 生成自签名证书

```rust
pub fn generate_self_signed_cert(
    cert_path: &str,
    key_path: &str
) -> Result<()>
```

**功能**: 生成自签名X.509证书和私钥

**参数**:
- `cert_path`: 证书文件保存路径
- `key_path`: 私钥文件保存路径

**证书属性**:

| 属性 | 值 |
|------|-----|
| 通用名称(CN) | WFTPG FTP Server |
| 组织(O) | WFTPG |
| 有效期 | 10年 (3650天) |
| 密钥类型 | RSA |
| 格式 | PEM |

**主题备用名称(SAN)**:
- DNS: localhost
- IP: 127.0.0.1
- IP: ::1

**执行流程**:

#### 1. 创建证书目录
```rust
let cert_dir = Path::new(cert_path)
    .parent()
    .ok_or_else(|| anyhow::anyhow!("Invalid certificate path"))?;

fs::create_dir_all(cert_dir).context("Failed to create certificate directory")?;
```

#### 2. 配置证书参数
```rust
let mut params = rcgen::CertificateParams::default();

// 设置主题名称
params.distinguished_name = rcgen::DistinguishedName::new();
params.distinguished_name.push(rcgen::DnType::CommonName, "WFTPG FTP Server");
params.distinguished_name.push(rcgen::DnType::OrganizationName, "WFTPG");

// 设置主题备用名称
params.subject_alt_names = vec![
    rcgen::SanType::DnsName("localhost".try_into()?),
    rcgen::SanType::IpAddress(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
    rcgen::SanType::IpAddress(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1))),
];

// 设置有效期
params.not_before = time::OffsetDateTime::now_utc();
params.not_after = params.not_before + time::Duration::days(3650);
```

#### 3. 生成密钥对和证书
```rust
let key_pair = rcgen::KeyPair::generate()?;
let cert = params.self_signed(&key_pair)?;
```

#### 4. 保存私钥
```rust
let key_pem = key_pair.serialize_pem();
fs::write(key_path, key_pem).context("Failed to save private key file")?;

// Unix系统设置权限为600（仅所有者可读写）
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(key_path)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(key_path, permissions)?;
}
```

#### 5. 保存证书
```rust
let cert_pem = cert.pem();
fs::write(cert_path, cert_pem).context("Failed to save certificate file")?;
```

### ensure_cert_exists - 确保证书存在

```rust
pub fn ensure_cert_exists(
    cert_path: &str,
    key_path: &str
) -> Result<bool>
```

**功能**: 检查证书是否存在，不存在则生成

**返回**: 
- `Ok(true)`: 生成了新证书
- `Ok(false)`: 证书已存在

**流程**:

```
检查证书和密钥文件是否存在
    ↓
都存在 → 返回 Ok(false)
    ↓
部分存在 → 删除现有文件，重新生成
    ↓
都不存在 → 生成新证书
```

**实现**:
```rust
pub fn ensure_cert_exists(cert_path: &str, key_path: &str) -> Result<bool> {
    let cert_file = Path::new(cert_path);
    let key_file = Path::new(key_path);

    // 证书和密钥都存在
    if cert_file.exists() && key_file.exists() {
        info!("FTPS certificate file already exists");
        return Ok(false);
    }

    // 只存在其中一个，删除后重新生成
    if cert_file.exists() != key_file.exists() {
        warn!("Only certificate or key file exists, will regenerate");
        
        if cert_file.exists() {
            if let Err(e) = fs::remove_file(cert_file) {
                warn!("Failed to remove cert file: {}", e);
            }
        }
        if key_file.exists() {
            if let Err(e) = fs::remove_file(key_file) {
                warn!("Failed to remove key file: {}", e);
            }
        }
    }

    // 生成新证书
    generate_self_signed_cert(cert_path, key_path)?;
    Ok(true)
}
```

## 证书文件格式

### 证书文件 (PEM格式)

```
-----BEGIN CERTIFICATE-----
MIIDXTCCAkWgAwIBAgIJAJC1HiIAZAiUMA0GCSqGSIb3DQEBCwUAMEUxCzAJBgNV
BAYTAkFVMRMwEQYDVQQIDApTb21lLVN0YXRlMSEwHwYDVQQKDBhJbnRlcm5ldCBX
...
-----END CERTIFICATE-----
```

### 私钥文件 (PEM格式)

```
-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7VJTUt9Us8cKj
...
-----END PRIVATE KEY-----
```

## 使用示例

### 在FTP服务器中使用

```rust
// 启动FTPS时检查证书
if config.ftps_enabled {
    let cert_path = config.ftps_cert_path.as_deref().unwrap();
    let key_path = config.ftps_key_path.as_deref().unwrap();
    
    match cert_gen::ensure_cert_exists(cert_path, key_path) {
        Ok(true) => tracing::info!("Generated self-signed certificate for FTPS"),
        Ok(false) => tracing::info!("Using existing FTPS certificate"),
        Err(e) => {
            tracing::error!("Certificate check/generation failed: {}", e);
            return Err(e);
        }
    }
    
    // 配置FTPS
    server_builder = server_builder
        .ftps(cert_path, key_path)
        .ftps_required(config.ftps_require_ssl, config.ftps_require_ssl);
}
```

### 单独使用

```rust
use crate::core::ftp_server::cert_gen;

// 生成新证书
cert_gen::generate_self_signed_cert(
    "/etc/wftpg/certs/server.crt",
    "/etc/wftpg/certs/server.key"
)?;

// 或确保证书存在
let generated = cert_gen::ensure_cert_exists(
    "/etc/wftpg/certs/server.crt",
    "/etc/wftpg/certs/server.key"
)?;
if generated {
    println!("已生成新证书");
}
```

## 安全考虑

### 自签名证书的限制

1. **不受信任**: 客户端会显示证书警告
2. **仅用于测试**: 生产环境应使用CA签名证书
3. **中间人攻击**: 无法防止MITM攻击

### 私钥保护

**Unix系统**:
```rust
permissions.set_mode(0o600);  // 仅所有者可读写
```

**Windows系统**: 依赖文件系统权限

### 生产环境建议

1. **使用CA签名证书**: 如Let's Encrypt
2. **证书轮换**: 定期更新证书
3. **私钥保护**: 严格限制访问权限
4. **证书验证**: 启用客户端证书验证

## 证书验证

### 客户端验证自签名证书

**FileZilla配置**:
1. 首次连接会显示证书警告
2. 选择"信任此证书"
3. 后续连接不再提示

**命令行工具**:
```bash
# lftp
set ssl:verify-certificate no

# curl
curl --insecure ftps://server/
```

### 证书链验证

```
CA证书
  └── 服务器证书
```

自签名证书没有证书链，客户端需要手动信任。

## 错误处理

| 错误场景 | 处理方式 |
|----------|----------|
| 路径无效 | 返回错误 |
| 目录创建失败 | 返回错误（带上下文） |
| 密钥生成失败 | 返回错误 |
| 文件写入失败 | 返回错误（带上下文） |
| 权限设置失败 | 忽略（仅Unix） |

## 日志记录

| 事件 | 级别 | 内容 |
|------|------|------|
| 开始生成 | INFO | "Generating self-signed certificate for FTPS..." |
| 私钥保存 | INFO | 路径 |
| 证书保存 | INFO | 路径 |
| 生成完成 | INFO | "FTPS self-signed certificate generated successfully" |
| 证书已存在 | INFO | "FTPS certificate file already exists" |
| 部分存在 | WARN | "Only certificate or key file exists, will regenerate" |
| 删除失败 | WARN | 错误信息 |

## 依赖库

### rcgen

用于生成X.509证书的Rust库：
- 纯Rust实现
- 支持多种密钥类型
- 支持SAN扩展
- PEM输出格式

### time

用于处理证书有效期：
- 跨平台时间处理
- 时区支持
- 时间计算

## 扩展功能（未来）

### 可能的增强

1. **ECDSA密钥**: 更高效的加密算法
2. **证书请求(CSR)**: 支持CA签名
3. **多域名支持**: 配置多个SAN
4. **证书续期**: 自动更新即将过期的证书
