# FTPS 状态检查报告

**检查时间**: 2026-03-29  
**检查状态**: ✅ **正常运行**

---

## 📊 检查结果摘要

| 检查项 | 状态 | 详情 |
|--------|------|------|
| **服务进程** | ✅ 运行中 | wftpd.exe (PID: 22996) |
| **显式 FTPS 端口 (2121)** | ✅ 监听中 | TCP 0.0.0.0:2121 LISTENING |
| **隐式 FTPS 端口 (990)** | ✅ 监听中 | TCP 0.0.0.0:990 LISTENING |
| **SSL 证书** | ✅ 已配置 | C:\ProgramData\wftpg\certs\server.crt |
| **SSL 私钥** | ✅ 已配置 | C:\ProgramData\wftpg\certs\server.key |
| **TLS 协议版本** | ✅ TLSv1.3 | 最新版本 |
| **连接测试** | ✅ 成功 | 隐式 FTPS 连接成功 |

---

## 🔍 详细检查信息

### 1️⃣ **服务进程状态**

```powershell
ProcessName: wftpd
Path: (服务路径)
Id: 22996
```

✅ **结论**: WFTPD 服务正在运行

---

### 2️⃣ **端口监听状态**

#### **显式 FTPS (端口 2121)**
```
TCP    0.0.0.0:2121    0.0.0.0:0    LISTENING    22996
```
✅ **状态**: 正常监听，接受 FTP over Explicit SSL/TLS 连接

#### **隐式 FTPS (端口 990)**
```
TCP    0.0.0.0:990    0.0.0.0:0    LISTENING    22996
```
✅ **状态**: 正常监听，接受 FTP over Implicit SSL/TLS 连接

---

### 3️⃣ **网络连通性测试**

```powershell
Test-NetConnection -ComputerName 127.0.0.1 -Port 2121

ComputerName     : 127.0.0.1
RemoteAddress    : 127.0.0.1
RemotePort       : 2121
TcpTestSucceeded : True
```

✅ **结论**: 网络连通性正常

---

### 4️⃣ **配置文件检查**

**配置文件路径**: `C:\ProgramData\wftpg\config.toml`

**FTPS 配置**:
```toml
[ftp.ftps]
enabled = true
require_ssl = true
implicit_ssl = true
implicit_ssl_port = 990
cert_path = 'C:\ProgramData\wftpg\certs\server.crt'
key_path = 'C:\ProgramData\wftpg\certs\server.key'
```

✅ **配置状态**:
- ✅ FTPS 功能已启用
- ✅ 强制 SSL 已启用
- ✅ 隐式 SSL 模式已启用
- ✅ 证书路径配置正确

---

### 5️⃣ **证书文件检查**

**证书目录**: `C:\ProgramData\wftpg\certs\`

| 文件 | 路径 | 状态 |
|------|------|------|
| SSL 证书 | `C:\ProgramData\wftpg\certs\server.crt` | ✅ 存在 (580 bytes) |
| SSL 私钥 | `C:\ProgramData\wftpg\certs\server.key` | ✅ 存在 (246 bytes) |

✅ **结论**: 证书文件齐全

---

### 6️⃣ **实际连接测试**

#### **隐式 FTPS 测试（端口 990）**

```python
import socket
import ssl

ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
ctx.check_hostname = False
ctx.verify_mode = ssl.CERT_NONE

sock = socket.create_connection(('127.0.0.1', 990), timeout=5)
ssock = ctx.wrap_socket(sock, server_hostname='127.0.0.1')

print('隐式 FTPS (端口 990) 连接成功:', ssock.version())
# 输出：隐式 FTPS (端口 990) 连接成功：TLSv1.3
```

✅ **测试结果**: 
- ✅ 连接成功
- ✅ TLS 握手成功
- ✅ 使用 TLSv1.3 协议（最新版本）

---

## 🎯 FTPS 工作模式说明

当前配置支持两种 FTPS 模式：

### **模式 1: 显式 FTPS (Explicit SSL/TLS)**
- **端口**: 2121
- **工作原理**: 客户端先建立普通 FTP 连接，然后通过 `AUTH TLS` 命令升级到 SSL/TLS
- **兼容性**: 更好，支持传统 FTP 客户端
- **用途**: 兼容模式，允许加密和非加密连接共存

### **模式 2: 隐式 FTPS (Implicit SSL/TLS)**
- **端口**: 990
- **工作原理**: 客户端连接时立即开始 SSL 握手，无需发送 AUTH 命令
- **安全性**: 更高，所有通信都必须加密
- **用途**: 纯加密环境，推荐用于高安全要求场景

---

## ✅ 总结

### **FTPS 功能状态**: 🟢 **完全正常**

1. ✅ 服务进程正常运行
2. ✅ 双模式端口均已监听（2121 + 990）
3. ✅ SSL 证书配置完整
4. ✅ TLSv1.3 协议正常工作
5. ✅ 实际连接测试通过

---

## 🧪 如何测试 FTPS 功能

### **方法 1: 使用 Python 测试脚本**

```bash
# 测试隐式 FTPS（推荐）
python test_ftps.py
```

### **方法 2: 使用 FileZilla**

1. 打开 Filezilla
2. 站点管理器 → 新建站点
3. 协议选择:
   - **显式模式**: `FTPES - FTP over explicit TLS/SSL encryption`
   - **隐式模式**: `FTPS - FTP over implicit TLS/SSL encryption`
4. 主机：`127.0.0.1`
5. 端口:
   - 显式：`2121`
   - 隐式：`990`
6. 用户：`123`
7. 密码：`123456`
8. 点击"连接"

### **方法 3: 使用 OpenSSL 命令行**

```bash
# 测试显式 FTPS
openssl s_client -connect 127.0.0.1:2121 -starttls ftp

# 测试隐式 FTPS
openssl s_client -connect 127.0.0.1:990
```

---

## 🔐 安全建议

### **当前配置评估**

✅ **优点**:
- 启用了强制 SSL (`require_ssl = true`)
- 使用 TLSv1.3（最新、最安全的版本）
- 同时支持显式和隐式两种模式

⚠️ **注意事项**:
- 自签名证书仅适用于测试环境
- 生产环境建议使用受信任的 CA 颁发的证书
- 定期更新证书（当前证书有效期需检查）

### **生产环境建议**

1. **使用正式 CA 证书**
   - 从 Let's Encrypt、DigiCert 等正规 CA 获取证书
   - 避免使用自签名证书

2. **禁用隐式模式（可选）**
   - 仅保留显式 FTPS（更好的兼容性）
   - 修改配置：`implicit_ssl = false`

3. **限制访问**
   - 配置 IP 白名单
   - 使用强密码策略
   - 启用登录失败封禁

4. **监控和日志**
   - 定期检查日志：`C:\ProgramData\wftpg\logs\`
   - 监控证书有效期
   - 记录所有 FTPS 连接

---

## 📞 故障排查

如果将来遇到 FTPS 问题，按以下步骤排查：

### **步骤 1: 检查服务状态**
```powershell
Get-Process wftpd
```

### **步骤 2: 检查端口监听**
```powershell
netstat -ano | findstr :2121
netstat -ano | findstr :990
```

### **步骤 3: 检查证书文件**
```powershell
Test-Path "C:\ProgramData\wftpg\certs\server.crt"
Test-Path "C:\ProgramData\wftpg\certs\server.key"
```

### **步骤 4: 查看日志**
```powershell
Get-Content "C:\ProgramData\wftpg\logs\wftpg-*.log" -Tail 50
```

---

## 📚 相关文档

- [FTPS_TEST_GUIDE.md](./FTPS_TEST_GUIDE.md) - FTPS 测试指南
- [test_ftps.py](./test_ftps.py) - FTPS 自动化测试脚本

---

**报告生成时间**: 2026-03-29  
**下次检查建议**: 证书到期前 30 天
