# WFTPD-Test-Go 使用说明

## 快速开始

### 基本用法

```bash
# 使用默认配置运行测试
.\wftpd_test.exe

# 指定 SFTP 服务器
.\wftpd_test.exe -sftp 127.0.0.1 -sftp-port 2222 -user 123 -pass 123123

# 指定 FTP 服务器
.\wftpd_test.exe -ftp 192.168.1.100 -ftp-port 21 -user admin -pass password

# 自定义日志文件
.\wftpd_test.exe -log ./my_test.log
```

### 命令行参数

| 参数 | 说明 | 示例 |
|------|------|------|
| `-config` | 配置文件路径（默认: config.json） | `-config ./my_config.json` |
| `-ftp` | FTP 服务器地址 | `-ftp 127.0.0.1` |
| `-ftp-port` | FTP 端口 | `-ftp-port 21` |
| `-sftp` | SFTP 服务器地址 | `-sftp 127.0.0.1` |
| `-sftp-port` | SFTP 端口 | `-sftp-port 2222` |
| `-user` | 用户名 | `-user admin` |
| `-pass` | 密码 | `-pass password123` |
| `-log` | 日志文件路径 | `-log ./test.log` |

## 配置文件

编辑 `config.json` 来配置测试参数：

```json
{
    "ftp_server": "127.0.0.1",
    "ftp_port": 21,
    "sftp_server": "127.0.0.1",
    "sftp_port": 2222,
    "username": "test_user",
    "password": "test_password",
    "test_data_dir": "./testdata",
    "log_file": "./test_result.log",
    "use_tls": false,
    "implicit_ftps": false,
    "timeout_seconds": 10,
    "max_concurrent": 3
}
```

## 测试模块

### FTP 测试
- 基本连接
- 用户认证
- 目录操作（创建、删除、切换）
- 文件上传/下载
- 文件列表（LIST/NLST）
- 文件删除
- 断点续传
- 被动模式（PASV/EPSV）
- 主动模式（PORT）
- FTPS TLS 加密（可选）
- 功能查询（FEAT/SYST）
- 文件重命名
- UTF-8 文件名支持
- 并发传输
- 性能基准测试
- 等等...

### SFTP 测试
- 基本连接
- 目录操作
- 文件上传/下载/删除
- 大文件传输
- 重命名操作
- 错误处理
- 符号链接操作
- 文件权限管理
- 并发传输
- 断点续传

## 输出说明

测试结果会同时输出到控制台和日志文件：

```
========================================
WFTPD FTP/SFTP 测试套件
========================================

FTP 服务器: 127.0.0.1:21
SFTP 服务器: 127.0.0.1:2222
用户名: test_user

[准备] 生成测试文件...
  ✓ 创建小文件: testdata\small.txt (1KB)
  ✓ 创建中文件: testdata\medium.bin (1MB)
  ✓ 创建大文件: testdata\large.bin (10MB)

========================================
FTP 测试模块
========================================

  [连接] 正在连接到 127.0.0.1:21...
  ✓ 连接成功，响应: 220 Welcome to WFTPG FTP Server
  [耗时] 2.60 ms

========================================
测试报告
========================================
 1. [✓ 通过] FTP 基本连接
    耗时: 2.60 ms
 2. [✗ 失败] FTP 文件上传
    错误: PASV 命令错误: 0
    
总计: 30 项测试，25 通过，5 失败
========================================
```

## 常见问题

### Q: SFTP 测试卡住怎么办？
A: 程序已添加超时机制，如果 SFTP 客户端关闭超时（5秒），会自动继续执行后续测试。

### Q: 如何只测试 SFTP？
A: 目前程序会同时运行 FTP 和 SFTP 测试。如需单独测试，可以注释掉 `main.go` 中的 `runFTPTests()` 调用。

### Q: 测试文件在哪里？
A: 测试文件会自动生成在 `testdata` 目录中（可配置）。

### Q: 为什么有些测试失败？
A: 可能原因：
- 服务器不支持某些 FTP/SFTP 特性
- 网络连接问题
- 权限不足
- 服务器实现与标准不完全兼容

查看日志文件获取详细错误信息。

## 已知问题

1. **SFTP 文件操作可能断开连接**
   - 原因：SFTP 服务器实现与 Go sftp 库的期望行为不完全兼容
   - 状态：已添加超时机制避免卡住，但部分操作仍可能失败
   - 建议：优化 SFTP 服务器端实现

2. **FTP PASV 模式问题**
   - 某些情况下 PASV 连接可能被服务器关闭
   - 建议使用 EPSV（扩展被动模式）

## 技术支持

如遇到问题，请检查：
1. 服务器是否正常运行
2. 网络连接是否正常
3. 用户名密码是否正确
4. 防火墙设置
5. 日志文件中的详细错误信息

## 版本历史

### v1.1 (2026-04-06)
- ✅ 修复 main.go 编译错误
- ✅ 添加 SFTP 客户端关闭超时机制
- ✅ 改进错误处理和日志记录

### v1.0 (初始版本)
- 基础 FTP/SFTP 测试功能
