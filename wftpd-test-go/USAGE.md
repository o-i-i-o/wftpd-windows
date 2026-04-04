# WFTPD FTP/SFTP 测试工具使用说明

## 安装 Go 环境

### 1. 下载并安装 Go
访问 [https://golang.org/dl/](https://golang.org/dl/) 下载并安装 Go 1.21 或更高版本。

### 2. 验证安装
```cmd
go version
```

### 3. 设置 GOPATH（可选）
```cmd
set GOPATH=%USERPROFILE%\go
```

## 项目结构

```
wftpd-test-go/
├── go.mod          # Go 模块定义
├── go.sum          # 依赖校验和
├── main.go         # 主程序入口
├── sftp_test.go    # SFTP 测试实现
├── build.bat       # Windows 构建脚本
├── README.md       # 项目说明
└── testdata/       # 测试数据目录（自动生成）
    ├── small.txt   # 小文件 (1KB)
    ├── medium.bin  # 中文件 (1MB)
    └── large.bin   # 大文件 (10MB)
```

## 构建步骤

### 方法一：使用构建脚本（推荐）
```cmd
cd c:\Users\oi-io\Documents\wftpg-egui-20260328\wftpd-test-go
build.bat
```

### 方法二：手动构建
```cmd
# 1. 初始化模块（如果尚未初始化）
go mod init wftpd-test-go

# 2. 下载依赖
go mod tidy

# 3. 构建可执行文件
go build -o wftpd_test.exe -ldflags="-s -w"
```

## 配置测试参数

编辑 `main.go` 文件中的 `TestConfig` 结构体：

```go
config = TestConfig{
    FTPServer:     "127.0.0.1",      // FTP 服务器地址
    FTPPort:       21,               // FTP 端口
    SFTPServer:    "127.0.0.1",      // SFTP 服务器地址
    SFTPPort:      22,               // SFTP 端口
    Username:      "testuser",       // 测试用户名
    Password:      "testpass123",    // 测试密码
    TestDataDir:   "./testdata",     // 测试数据目录
    UseTLS:        true,             // 是否启用 TLS 测试
    ImplicitFTPS:  false,            // 是否使用隐式 FTPS
}
```

## 运行测试

### 1. 确保 WFTPD 服务正在运行
启动 WFTPD 服务并确保 FTP 和 SFTP 服务都已启用。

### 2. 运行测试程序
```cmd
wftpd_test.exe
```

## 测试内容

### FTP 测试项目
1. **基本连接测试** - 验证 TCP 连接和服务器响应
2. **用户认证测试** - 测试 USER/PASS 命令
3. **目录操作测试** - PWD, CWD, MKD, RMD, CDUP
4. **文件上传测试** - STOR 命令（小/中/大文件）
5. **文件下载测试** - RETR 命令
6. **断点续传测试** - REST 命令
7. **被动模式测试** - PASV/EPSV 命令
8. **TLS 加密测试** - AUTH TLS (FTPS)

### SFTP 测试项目
1. **SSH 连接测试** - SSH 协议握手
2. **SFTP 连接测试** - SFTP 子协议
3. **目录操作测试** - Getwd, Mkdir, ReadDir, RemoveDirectory
4. **文件操作测试** - Create, Open, Read, Write
5. **文件重命名测试** - Rename 操作
6. **文件信息查询** - Stat 操作

## 日志输出

### 控制台输出格式
```
[操作] 描述信息...
✓ 成功信息
✗ 失败信息: 错误详情
[耗时] XX.XX ms
```

### 响应码说明
- `2xx` - 成功响应
- `3xx` - 需要更多输入
- `4xx` - 临时错误
- `5xx` - 永久错误

## 故障排除

### 常见问题

#### 1. 连接被拒绝
- **症状**: dial tcp :21: connectex: No connection could be made
- **原因**: WFTPD 服务未启动或端口被占用
- **解决方案**: 确保 WFTPD 服务运行并检查防火墙设置

#### 2. 认证失败
- **症状**: 530 Not logged in
- **原因**: 用户名或密码错误
- **解决方案**: 检查 WFTPD 用户配置

#### 3. TLS 连接失败
- **症状**: TLS handshake failure
- **原因**: 证书配置问题
- **解决方案**: 检查 WFTPD TLS 证书配置

#### 4. 被动模式失败
- **症状**: 无法建立数据连接
- **原因**: 防火墙阻止被动端口范围
- **解决方案**: 配置防火墙规则或 NAT 映射

### 调试技巧

#### 启用详细日志
修改代码中的日志级别或添加更多调试输出。

#### 测试单个功能
注释掉不需要的测试函数，只运行特定测试。

## 性能基准

### 本地测试预期结果
- 连接时间: < 50ms
- 认证时间: < 100ms
- 1KB 文件传输: < 100ms
- 1MB 文件传输: < 500ms
- 10MB 文件传输: < 2000ms

### 网络延迟影响
实际时间取决于网络状况和服务器性能。

## 协议标准支持

### FTP 标准
- RFC 959 - File Transfer Protocol
- RFC 2228 - FTP Security Extensions
- RFC 2389 - Feature Negotiation
- RFC 3659 - Extensions to FTP

### SFTP 标准
- SSH File Transfer Protocol (draft-ietf-secsh-filexfer)
- RFC 4253 - SSH Transport Layer

## 自定义测试

### 添加新的测试案例
在 `main.go` 中添加新的 `testResult` 调用：

```go
testResult("自定义测试名称", func() error {
    // 测试逻辑
    return nil
})
```

### 修改测试数据
编辑 `generateTestFiles()` 函数以创建不同的测试文件。

## 安全注意事项

### 生产环境使用
- 不要在生产环境中使用默认的测试凭证
- 验证服务器证书以防止中间人攻击
- 限制测试账户的权限

### 数据安全
- 测试文件包含随机数据，不会泄露敏感信息
- 测试完成后自动清理临时文件

## 更新和维护

### 代码结构
- `main.go` - 核心 FTP 测试逻辑
- `sftp_test.go` - SFTP 测试逻辑
- 保持代码模块化便于维护

### 依赖管理
- 使用 Go Modules 管理依赖
- 定期更新依赖包以获得安全补丁

## 开发者参考

### 代码风格
遵循 Go 语言标准代码风格，使用 `gofmt` 格式化代码。

### 错误处理
统一的错误处理模式，确保所有错误都被适当捕获和报告。

### 测试覆盖率
目标是覆盖所有主要功能路径和错误情况。

---
**注意**: 此工具仅供测试 WFTPD 服务器功能使用，请勿用于未经授权的系统测试。