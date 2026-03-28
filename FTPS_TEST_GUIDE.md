# FTPS 测试指南

## 📋 概述

本指南用于测试 WFTPG 的 FTPS (FTP over SSL/TLS) 功能。

---

## ⚠️ 当前测试失败原因

```
[WinError 10061] 由于目标计算机积极拒绝，无法连接。
```

**原因分析：**
- ✅ Python 环境正确
- ✅ 测试脚本正确
- ❌ **FTPS 服务未启用或未正确配置**

---

## 🔧 启用 FTPS 的步骤

### 步骤 1: 生成或准备 SSL 证书

FTPS 需要 SSL 证书和私钥。你可以选择：

#### 选项 A：使用自签名证书（推荐用于测试）

项目已包含证书生成工具：

```bash
# 运行证书生成器
target\release\wftpd.exe --gen-cert
# 或者
cargo run --bin wftpd -- --gen-cert
```

证书将生成到：
- `C:\ProgramData\wftpg\cert.pem` (证书)
- `C:\ProgramData\wftpg\key.pem` (私钥)

#### 选项 B：使用现有的 SSL 证书

如果你有有效的 SSL 证书，将其放置在：
- 证书文件：`C:\ProgramData\wftpg\cert.pem`
- 私钥文件：`C:\ProgramData\wftpg\key.pem`

---

### 步骤 2: 在 GUI 中配置 FTPS

1. **以管理员身份运行 WFTPG GUI**
   ```
   target\release\wftpg.exe
   ```

2. **进入"服务器配置"页面**

3. **启用 FTPS:**
   - 找到 "🔒 FTPS 设置 (FTP over SSL/TLS)" 部分
   - ✅ 勾选 "启用 FTPS"

4. **配置证书路径:**
   - 证书文件：`C:\ProgramData\wftpg\cert.pem`
   - 私钥文件：`C:\ProgramData\wftpg\key.pem`

5. **选择 SSL 模式:**
   - ☑️ **显式 SSL (推荐)**: 客户端通过 `AUTH TLS` 命令升级连接
     - 端口：2121（与 FTP 相同）
   - ☐ **隐式 SSL**: 连接立即开始 SSL 握手
     - 端口：990（默认）

6. **强制 SSL（可选）:**
   - ☐ 勾选后将拒绝非加密连接

7. **保存配置:**
   - 点击 "💾 保存配置" 按钮
   - 等待后端重新加载配置

---

### 步骤 3: 重启服务

配置更改后，需要重启服务：

1. **打开"系统服务管理"页面**

2. **重启服务:**
   - 点击 "🔄 重启服务" 按钮
   - 等待服务状态更新

3. **验证服务运行:**
   - 确保"运行状态"显示为 "● 运行中"

---

### 步骤 4: 验证 FTPS 端口

```bash
# 使用 PowerShell 检查端口
netstat -ano | findstr :2121
netstat -ano | findstr :990
```

应该能看到类似：
```
TCP    0.0.0.0:2121           0.0.0.0:0              LISTENING       12345
```

---

## 🧪 运行 FTPS 测试

### 方法 1: 运行完整测试脚本

```bash
python test_ftps.py
```

### 方法 2: 手动测试连接

#### 使用 Windows 命令行

```cmd
# 显式 FTPS 测试
openssl s_client -connect 127.0.0.1:2121 -starttls ftp

# 隐式 FTPS 测试
openssl s_client -connect 127.0.0.1:990
```

#### 使用 FileZilla

1. 打开 Filezilla
2. 站点管理器 → 新建站点
3. 协议：**FTPES** (FTP over explicit TLS/SSL)
4. 主机：`127.0.0.1`
5. 端口：`2121`
6. 用户：`123`
7. 密码：`123456`
8. 连接

---

## 📊 测试项目说明

FTPS 测试包括以下项目：

| 测试项 | 说明 | 重要性 |
|--------|------|--------|
| SSL 握手验证 | 验证 SSL/TLS 协议版本和加密套件 | ⭐⭐⭐ |
| AUTH TLS 协商 | 测试显式 SSL 升级命令 | ⭐⭐⭐ |
| 登录验证 | 测试加密通道下的认证 | ⭐⭐⭐ |
| PWD 命令 | 基本 FTP 命令测试 | ⭐⭐ |
| MKD 创建目录 | 目录操作测试 | ⭐⭐ |
| CWD 切换目录 | 目录操作测试 | ⭐⭐ |
| STOR 上传文件 | 文件上传（加密传输） | ⭐⭐⭐ |
| RETR 下载文件 | 文件下载（加密传输） | ⭐⭐⭐ |
| LIST 列出目录 | 目录列表（加密传输） | ⭐⭐ |
| 数据传输加密 | PBSZ/PROT 命令测试 | ⭐⭐⭐ |
| DELE 删除文件 | 文件删除测试 | ⭐ |
| RMD 删除目录 | 目录删除测试 | ⭐ |

---

## ✅ 成功的测试结果示例

```
==============================================================
开始 FTPS (显式 SSL/TLS) 功能测试
==============================================================
FTPS (显式 SSL) 连接成功 - 协议版本：TLSv1.3
    加密套件：TLS_AES_256_GCM_SHA384 (TLSv1.3)
  [PASS] SSL 握手验证
  [PASS] AUTH TLS 协商
  [PASS] 登录验证
  [PASS] PWD 命令
  [PASS] MKD 创建目录
  [PASS] CWD 切换目录
  [PASS] STOR 上传文件（MD5 校验）
  [PASS] LIST 列出目录
  [PASS] 数据传输加密
  [PASS] RETR 下载文件（MD5 校验）
  [PASS] DELE 删除文件
  [PASS] RMD 删除目录

==============================================================
FTPS 测试结果汇总
==============================================================

总测试数：12
通过：12
失败：0
成功率：100.00%
耗时：2.35 秒

✅ 所有 12 项测试全部通过！
```

---

## 🔍 常见问题排查

### 问题 1: 连接被拒绝

**症状:**
```
[WinError 10061] 由于目标计算机积极拒绝，无法连接。
```

**解决方案:**
1. 确认服务已启动
2. 检查防火墙规则
3. 验证端口是否监听：`netstat -ano | findstr :2121`

---

### 问题 2: SSL 握手失败

**症状:**
```
[SSL: WRONG_VERSION_NUMBER] wrong version number
```

**解决方案:**
1. 确认证书和私钥路径正确
2. 检查证书格式是否为 PEM
3. 尝试重启服务

---

### 问题 3: 证书验证失败

**症状:**
```
[SSL: CERTIFICATE_VERIFY_FAILED] certificate verify failed
```

**解决方案:**
1. 测试环境使用自签名证书是正常的
2. 测试脚本已设置 `ssl.CERT_NONE` 跳过验证
3. 生产环境应使用受信任的 CA 证书

---

### 问题 4: 数据连接建立失败

**症状:**
```
TimeoutError: [WinError 10060] 连接超时
```

**解决方案:**
1. 确保使用被动模式（PASV）
2. 检查被动端口范围配置
3. 防火墙开放被动端口范围

---

## 🔐 安全建议

### 生产环境配置

1. **使用受信任的 CA 证书**
   - 不要使用自签名证书
   - 从正规 CA 购买 SSL 证书

2. **强制 SSL**
   - 勾选"强制 SSL"选项
   - 拒绝所有非加密连接

3. **使用强加密套件**
   - TLS 1.2 或更高版本
   - AES-256-GCM 或 ChaCha20-Poly1305

4. **限制访问**
   - 配置 IP 白名单
   - 使用强密码策略

5. **定期更新证书**
   - 监控证书有效期
   - 提前续期避免中断

---

## 📝 配置文件位置

- **主配置文件**: `C:\ProgramData\wftpg\config.json`
- **用户配置文件**: `C:\ProgramData\wftpg\users.json`
- **SSL 证书**: `C:\ProgramData\wftpg\cert.pem`
- **SSL 私钥**: `C:\ProgramData\wftpg\key.pem`
- **日志文件**: `C:\ProgramData\wftpg\logs\wftpg-*.log`

---

## 📞 获取帮助

如果遇到问题：

1. **查看日志文件**
   ```
   C:\ProgramData\wftpg\logs\wftpg-*.log
   ```

2. **检查服务状态**
   - 打开 GUI 的"系统服务管理"页面
   - 查看服务是否运行

3. **查看测试结果**
   - JSON 结果：`test_ftps_result.json`
   - 详细错误信息在控制台输出

---

## 📚 相关文档

- [FTPS 协议规范](https://en.wikipedia.org/wiki/FTPS)
- [RFC 4217 - Securing FTP with TLS](https://tools.ietf.org/html/rfc4217)
- [OpenSSL 文档](https://www.openssl.org/docs/)

---

## ✅ 检查清单

在运行测试前，请确保：

- [ ] 已生成或准备好 SSL 证书
- [ ] 已在 GUI 中启用 FTPS
- [ ] 已配置证书和私钥路径
- [ ] 已保存配置并重启服务
- [ ] 服务状态为"运行中"
- [ ] 端口 2121（或 990）正在监听
- [ ] 防火墙已放行相应端口
- [ ] 已安装 Python 依赖：`pip install paramiko`

完成以上所有步骤后，再次运行测试：

```bash
python test_ftps.py
```

祝测试顺利！🎉
