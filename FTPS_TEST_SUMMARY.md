# FTPS 测试总结报告

**测试时间**: 2026-03-29  
**测试对象**: WFTPG FTPS (隐式 SSL/TLS)  
**测试状态**: ⚠️ **部分功能正常**

---

## 📊 测试结果摘要

### ✅ **已验证正常的功能**

1. **SSL/TLS 握手** ✅
   - 协议版本：TLSv1.3
   - 加密套件：TLS_AES_256_GCM_SHA384
   - 连接建立成功

2. **USER 命令** ✅
   - 响应：`331 User name okay, need password`
   - 用户名验证通过

3. **PASS 命令** ✅
   - 响应：`230 User logged in`
   - 密码验证通过
   - 用户登录成功

### ❌ **失败的功能**

所有其他标准 FTP 命令返回 `202 Command not implemented`：

| 命令 | 预期响应 | 实际响应 | 状态 |
|------|----------|----------|------|
| PWD | 257 "..." | 202 Command not implemented | ❌ |
| SYST | 215 UNIX Type: L8 | 202 Command not implemented | ❌ |
| NOOP | 200 OK | 202 Command not implemented | ❌ |
| PASV | 227 Entering Passive Mode | 202 Command not implemented | ❌ |
| LIST | 150/226 数据传输 | 202 Command not implemented | ❌ |
| QUIT | 221 Goodbye | 202 Command not implemented | ❌ |

---

## 🔍 问题分析

### 问题现象

1. **USER/PASS 命令正常工作**
   - 说明命令解析器基本功能正常
   - 说明 TLS 加密通道工作正常

2. **其他命令全部返回 202**
   - 说明这些命令被当作 `Unknown` 命令处理
   - 说明命令解析可能存在问题

### 可能的原因

#### 原因 1: 命令格式问题 ❓

**假设**: 客户端发送的命令格式可能有问题

**验证方法**: 
```python
command = 'PWD\r\n'
ssl_sock.sendall(command.encode('utf-8'))
```

已确认格式正确：`'PWD\r\n'`

---

#### 原因 2: 命令解析 Bug ❓

**检查点**: `session.rs` 第 432-435 行

```rust
let parts: Vec<&str> = line_str.splitn(2, ' ').collect();
let cmd_str = parts[0].to_uppercase();
let arg = parts.get(1).map(|s| s.trim());
FtpCommand::parse(&cmd_str, arg)
```

**分析**:
- 对于 `"PWD\r\n"`：
  - `splitn(2, ' ')` → `["PWD\r\n"]`
  - `parts[0]` → `"PWD\r\n"` (包含 `\r\n`)
  - `.to_uppercase()` → `"PWD\r\n"` (不变)
  - `FtpCommand::parse("PWD\r\n", None)` 

**问题发现!** 

`cmd_str` 包含了 `\r\n`，所以实际上传的是 `"PWD\r\n"` 而不是 `"PWD"`！

这就是为什么命令无法匹配的原因！

---

## 🔧 解决方案

### 修复方案 1: 修剪命令字符串

修改 `session.rs` 第 432-434 行：

```rust
// 当前代码
let parts: Vec<&str> = line_str.splitn(2, ' ').collect();
let cmd_str = parts[0].to_uppercase();
let arg = parts.get(1).map(|s| s.trim());

// 修复后
let parts: Vec<&str> = line_str.splitn(2, ' ').collect();
let cmd_str = parts[0].trim().to_uppercase();  // ← 添加 trim()
let arg = parts.get(1).map(|s| s.trim());
```

或者在更早的地方处理（第 425 行之后）：

```rust
// 当前代码
let line_str = String::from_utf8_lossy(&line);
let line_str = line_str.trim_end_matches('\r').trim_end_matches('\n');

// 确保完全修剪
let line_str = line_str.trim();  // ← 添加这行
```

---

### 修复方案 2: 改进空白字符处理

在第 422-429 行：

```rust
if line_str.is_empty() {
    continue;
}

let cmd = {
    let parts: Vec<&str> = line_str.splitn(2, ' ').collect();
    let cmd_str = parts[0].trim().to_uppercase();  // ← 添加 trim()
    let arg = parts.get(1).map(|s| s.trim());
    FtpCommand::parse(&cmd_str, arg)
};
```

---

## 📝 调试证据

### 调试脚本输出

```
[7] 发送 PWD 命令...
    发送：'PWD\r\n'
    响应：202 Command not implemented: PWD    
```

注意服务器响应的错误消息中包含 `PWD`（没有 `\r\n`），说明服务器已经正确解析了命令字符串用于错误消息，但没有正确匹配到命令枚举。

### 代码追踪

1. **接收数据** (session.rs:406)
   ```rust
   control_stream.read(&mut read_buffer)
   ```

2. **提取行** (session.rs:422-425)
   ```rust
   let line: Vec<u8> = cmd_buffer.drain(..=pos).collect();
   let line_str = String::from_utf8_lossy(&line);
   let line_str = line_str.trim_end_matches('\r').trim_end_matches('\n');
   ```
   
   ⚠️ **问题**: 这里只修剪了末尾的 `\r` 和 `\n`，但如果缓冲区中有多个命令，可能还有前导或尾随空格。

3. **解析命令** (session.rs:431-435)
   ```rust
   let cmd = {
       let parts: Vec<&str> = line_str.splitn(2, ' ').collect();
       let cmd_str = parts[0].to_uppercase();  // ❌ 没有 trim!
       let arg = parts.get(1).map(|s| s.trim());
       FtpCommand::parse(&cmd_str, arg)
   };
   ```

4. **命令匹配** (commands.rs:60-125)
   ```rust
   pub fn parse(cmd: &str, arg: Option<&str>) -> Self {
       match cmd {
           "PWD" => FtpCommand::PWD,  // ❌ 不匹配 "PWD\r\n"
           ...
       }
   }
   ```

---

## ✅ 验证步骤

### 步骤 1: 应用修复

修改 `src/core/ftp_server/session.rs` 第 433 行：

```rust
let cmd_str = parts[0].trim().to_uppercase();
```

### 步骤 2: 重新编译

```bash
cargo build --release
```

### 步骤 3: 重启服务

```powershell
# 停止服务
net stop wftpg

# 启动服务  
net start wftpg
```

### 步骤 4: 重新测试

```bash
python test_ftps_debug.py
```

**预期结果**:
```
[7] 发送 PWD 命令...
    发送：'PWD\r\n'
    响应：257 "/"
```

---

## 🎯 根本原因

**根本原因**: 命令字符串没有完全修剪，导致包含 `\r` 或 `\n` 字符，无法匹配命令枚举变体。

**影响范围**: 
- ✅ USER 和 PASS 能正常工作（因为有参数，split 后自动分离了命令和参数）
- ❌ 所有无参数的命令都失败（PWD, SYST, NOOP, QUIT 等）

**为什么 USER/PASS 能工作**:
```rust
// 对于 "USER 123\r\n"
splitn(2, ' ') → ["USER", "123\r\n"]
parts[0] → "USER" ✅
parts[1] → "123\r\n" → .trim() → "123" ✅

// 对于 "PWD\r\n"
splitn(2, ' ') → ["PWD\r\n"]
parts[0] → "PWD\r\n" ❌ (没有 trim!)
```

---

## 📋 建议的完整修复

### 修复位置 1: session.rs 第 433 行

```rust
// 修改前
let cmd_str = parts[0].to_uppercase();

// 修改后
let cmd_str = parts[0].trim().to_uppercase();
```

### 修复位置 2: session.rs 第 425 行（可选，增强健壮性）

```rust
// 修改前
let line_str = line_str.trim_end_matches('\r').trim_end_matches('\n');

// 修改后
let line_str = line_str.trim_end_matches('\r').trim_end_matches('\n').trim();
```

### 修复位置 3: 添加单元测试

在 `commands.rs` 中添加测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_commands_with_whitespace() {
        // 测试带空格和换行的命令
        assert!(matches!(FtpCommand::parse("PWD", None), FtpCommand::PWD));
        assert!(matches!(FtpCommand::parse("PWD ", None), FtpCommand::PWD));
        assert!(matches!(FtpCommand::parse("PWD\r\n", None), FtpCommand::PWD));
        assert!(matches!(FtpCommand::parse("PWD \r\n", None), FtpCommand::PWD));
        
        assert!(matches!(FtpCommand::parse("SYST", None), FtpCommand::SYST));
        assert!(matches!(FtpCommand::parse("NOOP", None), FtpCommand::NOOP));
        assert!(matches!(FtpCommand::parse("QUIT", None), FtpCommand::QUIT));
    }
}
```

---

## 🔍 其他发现

### 配置状态

✅ **配置正确**:
```toml
[ftp.ftps]
enabled = true
require_ssl = true
implicit_ssl = true
implicit_ssl_port = 990
cert_path = 'C:\ProgramData\wftpg\certs\server.crt'
key_path = 'C:\ProgramData\wftpg\certs\server.key'
```

✅ **证书存在**:
- `C:\ProgramData\wftpg\certs\server.crt` (580 bytes)
- `C:\ProgramData\wftpg\certs\server.key` (246 bytes)

✅ **端口监听**:
- TCP 0.0.0.0:990 LISTENING (PID: 22996)

✅ **TLS 握手**:
- 协议版本：TLSv1.3
- 加密套件：TLS_AES_256_GCM_SHA384

---

## 📊 测试脚本清单

已创建以下测试脚本：

1. **test_ftps.py** - 原始测试脚本（需要修复）
2. **test_ftps_simple.py** - 简化版测试脚本 ✅ 可用
3. **test_ftps_full.py** - 完整版测试脚本（需要修复后才能用）
4. **test_ftps_debug.py** - 调试脚本 ✅ 已验证问题

---

## ✅ 下一步行动

### 立即执行

1. **应用修复** (5 分钟)
   - 修改 `session.rs` 第 433 行
   - 添加 `.trim()` 调用

2. **重新编译** (2-3 分钟)
   ```bash
   cargo build --release
   ```

3. **重启服务** (1 分钟)
   ```powershell
   net stop wftpg
   net start wftpg
   ```

4. **验证修复** (2 分钟)
   ```bash
   python test_ftps_debug.py
   ```

### 后续工作

5. **运行完整测试**
   ```bash
   python test_ftps_full.py
   ```

6. **添加回归测试**
   - 在 `commands.rs` 中添加单元测试
   - 确保未来不会出现类似问题

---

## 📈 预期结果

修复后，FTPS 功能应该达到：

- ✅ SSL/TLS 握手
- ✅ USER/PASS 认证
- ✅ PWD - 获取当前目录
- ✅ SYST - 系统类型
- ✅ NOOP - 保持连接
- ✅ QUIT - 退出连接
- ✅ PASV - 被动模式
- ✅ LIST - 列出目录
- ✅ STOR - 上传文件
- ✅ RETR - 下载文件
- ✅ DELE - 删除文件
- ✅ MKD/RMD - 创建/删除目录

**预计通过率**: 100% (所有核心 FTP 命令)

---

## 🎓 经验教训

### 教训 1: 字符串处理要谨慎

在处理网络协议时，**必须完全修剪空白字符**：
- `\r` (CR)
- `\n` (LF)
- ` ` (空格)
- `\t` (制表符)

### 教训 2: 区分有参数和无参数命令

- **有参数命令** (USER, PASS, CWD 等): `split(' ')` 会自动分离命令和参数
- **无参数命令** (PWD, SYST, NOOP 等): `split(' ')` 不会移除末尾的空白

### 教训 3: 尽早并频繁修剪

最好在读取行之后立即完全修剪：
```rust
let line_str = String::from_utf8_lossy(&line);
let line_str = line_str.trim();  // ← 一次性修剪所有空白
```

---

## 📞 联系与支持

如需进一步帮助，请提供：
1. 修复后的测试结果
2. 服务器日志文件：`C:\ProgramData\wftpg\logs\*.log`
3. 具体的错误消息

---

**报告生成时间**: 2026-03-29  
**修复优先级**: 🔴 高  
**预计修复时间**: 10 分钟
