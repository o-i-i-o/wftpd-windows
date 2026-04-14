# WFTPG - SFTP/FTP 服务器管理工具

[![Rust](https://img.shields.io/badge/Rust-2024 Edition-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%2F11-blue.svg)](https://www.microsoft.com/windows)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![CI](https://github.com/oi-io/wftpg-egui/workflows/CI/badge.svg)](https://github.com/oi-io/wftpg-egui/actions)

WFTPG 是一个专为 Windows 平台设计的 SFTP/FTP 服务器管理工具，采用 Rust 语言开发，结合 egui 框架提供现代化的图形用户界面。

## 功能特性

### 服务器功能
- **SFTP 服务器** - 基于 SSH 的安全文件传输
- **FTP 服务器** - 标准文件传输协议支持
- **FTPS 支持** - FTP over SSL/TLS 加密传输
- **被动模式** - 支持被动模式传输，适应各种网络环境
- **多用户管理** - 支持多用户配置，独立主目录

### 管理功能
- **图形界面** - 基于 egui 的现代化用户界面
- **用户管理** - 添加、删除、配置用户权限
- **权限控制** - 细粒度的文件操作权限（读、写、删除、列表等）
- **配额管理** - 用户存储配额限制
- **速率限制** - 上传/下载速度控制
- **IP 过滤** - 基于 IP 地址的访问控制
- **日志记录** - 详细的操作日志和系统日志

### 系统服务
- **Windows 服务** - 可作为系统服务后台运行
- **服务管理** - 安装、启动、停止、卸载服务
- **IPC 通信** - 命名管道方式与 GUI 通信
- **配置热加载** - 运行时重新加载配置

## 系统要求

- **操作系统**: Windows 10/11 (64位)
- **运行环境**: 需要管理员权限
- **网络**: 需要开放相应端口（默认 SFTP: 2222, FTP: 21）

## 安装说明

### 从源码构建

```bash
# 克隆仓库
git clone <repository-url>
cd wftpg-egui

# 构建发布版本
cargo build --release

# 构建产物位于 target/release/
# - wftpg.exe (GUI 管理工具)
# - wftpd.exe (后台服务程序)
```

### 安装服务

1. 将 `wftpg.exe` 和 `wftpd.exe` 放在同一目录
2. 以管理员身份运行 `wftpg.exe`
3. 在"系统服务"标签页点击"安装服务"
4. 提供了安装脚本 `install.cmd`，用于安装程序。

## GUI 界面功能

### 1. ⚙ 服务器配置
- SFTP/FTP 服务启用/禁用
- 端口配置（SFTP 默认 2222，FTP 默认 21）
- 被动模式端口范围设置
- 最大连接数配置
- 实时连接监控（当前连接数、单 IP 连接数）

### 2. 👤 用户管理
- 添加/删除/编辑用户
- 用户名和密码配置
- 主目录设置
- 权限配置：
  - 读取文件
  - 写入文件
  - 删除文件
  - 列出目录
  - 创建目录
  - 重命名/移动
- 配额设置（字节为单位）

### 3. 🔒 安全设置
- IP 白名单/黑名单管理
- 允许/拒绝规则配置
- 全局安全策略

### 4. 🖥 系统服务
- Windows 服务安装/卸载
- 服务启动/停止
- 服务状态查看
- 自动启动配置

### 5. 📋 运行日志
- 实时日志查看
- 日志级别过滤（DEBUG、INFO、WARN、ERROR）
- 日志搜索
- 日志清理

### 6. 📁 文件日志
- 文件操作日志记录
- 上传/下载历史
- 按用户筛选
- 按时间范围筛选

### 7. ℹ 关于
- 版本信息
- 技术栈说明
- 许可证信息

## 使用说明

### 快速开始

1. **构建项目**
   ```bash
   # 构建 GUI 管理工具
   cd wftpg
   cargo build --release
   
   # 构建后台服务
   cd ../wftpd
   cargo build --release
   ```

2. **启动程序** - 以管理员身份运行 `wftpg.exe`

3. **配置服务器** - 在"⚙ 服务器"标签页启用 SFTP/FTP

4. **添加用户** - 在"👤 用户管理"标签页添加用户

5. **启动服务** - 点击"🖥 系统服务"标签页安装并启动服务

### 默认配置

- **SFTP 端口**: 2222
- **FTP 端口**: 21
- **被动端口范围**: 50000-50100
- **配置目录**: `C:\ProgramData\wftpg\`
- **日志目录**: `C:\ProgramData\wftpg\logs\`
- **配置文件**: `C:\ProgramData\wftpg\config.toml`
- **用户配置**: `C:\ProgramData\wftpg\users.toml`

### FTP/SFTP 客户端连接示例

#### Windows (FileZilla)
1. 主机：`localhost`
2. 协议：选择 `SFTP` 或 `FTP`
3. 端口：`2222` (SFTP) 或 `21` (FTP)
4. 用户名：`123`
5. 密码：`123123`

#### Linux (命令行 SFTP)
```bash
sftp -P 2222 123@localhost
# 输入密码：123123
```

#### Windows (PowerShell FTP)
```powershell
$ftp = [System.Net.FtpWebRequest]::Create("ftp://localhost:21")
$ftp.Credentials = [System.Net.NetworkCredential]::new("123", "123123")
$response = $ftp.GetResponse()
```

### 配置热加载

GUI修改配置文件后无需重启服务：
1. 保存配置，GUI通过命名管道发送 reload 命令热重载配置
2. 如果手动修改了配置文件，需要重启服务才能生效 
3. 涉及绑定地址、绑定端口等配置项，需要重启服务才能生效。

### 日志查看

- **GUI 查看**: 在"📋 运行日志"和"📁 文件日志"标签页查看
- **文件位置**: `C:\ProgramData\wftpg\logs\`
  - `wftpg_gui.log` - GUI 日志
  - `wftpd_service.log` - 服务日志

## 技术栈

### 核心框架
- **Rust** - 系统编程语言 (Edition 2024)
- **egui/eframe** - 即时模式 GUI 框架
- **tokio** - 异步运行时

### 协议实现
- **russh** - SSH/SFTP 协议实现
- **native-tls/rustls** - TLS/SSL 支持
- **rcgen** - 自签名证书生成

### 数据处理
- **serde** - 序列化/反序列化
- **toml** - 配置文件格式
- **chrono/time** - 时间日期处理

### 安全加密
- **argon2** - 密码哈希
- **rsa** - RSA 非对称加密
- **sha2/md-5** - 哈希算法

### 系统交互
- **windows-rs** - Windows API
- **windows-service** - Windows 服务管理
- **ctrlc** - 信号处理

### 日志与追踪
- **tracing** - 结构化日志
- **tracing-subscriber** - 日志订阅者
- **tracing-appender** - 日志输出

### 其他工具
- **parking_lot** - 高性能锁
- **notify** - 文件系统监听
- **ipnet** - IP 网络处理

## 架构设计

### 双进程架构

WFTPG 采用前后端分离的双进程架构：

```
┌─────────────────┐         ┌─────────────────┐
│   wftpg.exe     │         │   wftpd.exe     │
│  (GUI 管理工具)  │◄───────►│  (后台服务)      │
│                 │  命名管道 │                 │
│ - 配置管理界面   │  通信    │ - FTP 服务器     │
│ - 用户管理界面   │         │ - SFTP 服务器    │
│ - 日志查看界面   │         │ - IPC 监听       │
│ - 服务管理界面   │         │ - 配置热加载     │
└─────────────────┘         └─────────────────┘
```

### 主要特点

- **前后端分离**: GUI 和后台服务独立运行，通过命名管道通信
- **配置热加载**: 修改配置文件后自动生效，无需重启服务
- **多路复用**: 支持多个客户端同时连接
- **优雅关闭**: 支持信号处理和资源清理
- **模块化设计**: FTP、SFTP、配额、限流等模块独立可插拔

## 安全特性

### 认证与加密
- **密码加密** - 使用 Argon2 算法存储密码
- **RSA 密钥** - SSH 连接使用 RSA 非对称加密
- **TLS/SSL** - FTPS 支持显式和隐式 SSL/TLS 加密
- **自签名证书** - 自动生成 TLS 证书，无需手动配置

### 访问控制
- **路径隔离** - 用户只能访问自己的主目录 (chroot)
- **IP 过滤** - 支持允许/拒绝 IP 列表
- **防路径逃逸** - 防止 `..` 等路径遍历攻击
- **符号链接检查** - 验证符号链接目标
- **权限粒度** - 细粒度的文件操作权限（读、写、删除、列表、创建目录等）

### 资源限制
- **连接数限制** - 全局和单 IP 连接数控制
- **配额管理** - 用户存储配额限制
- **速率限制** - 上传/下载速度控制

## 开发规范

### 代码风格
- 所有文件使用 UTF-8 编码
- 代码遵循 Rust 2024 Edition 规范
- 使用 `rustfmt` 保持代码格式统一

### 质量检查
- 每次修改后运行 `cargo clippy` 检查
- 禁止隐藏告警，禁止使用 `#[allow(dead_code)]`
- 错误处理使用 `anyhow::Result` 或自定义错误类型

### 提交规范
- 使用清晰的提交信息
- 功能变更添加 `[Feature]` 前缀
- Bug 修复添加 `[Fix]` 前缀
- 性能优化添加 `[Perf]` 前缀

### 测试要求
- 关键功能需要编写单元测试
- 集成测试使用 Python 脚本验证
- CI 自动运行所有测试

## 持续集成

本项目使用 GitHub Actions 进行持续集成，自动执行以下任务：

- ✅ Windows 平台构建和测试
- ✅ 代码格式检查 (rustfmt)
- ✅ Clippy 代码质量检查
- ✅ 自动化测试运行
- ✅ 版本发布和打包

## 常见问题 (FAQ)

### Q: 为什么需要管理员权限？
A: WFTPG 需要注册 Windows 服务、绑定特权端口（如 21、22），这些操作需要管理员权限。

### Q: 服务安装失败怎么办？
A: 
1. 确认以管理员身份运行
2. 检查 wftpd.exe 是否与 wftpg.exe 在同一目录
3. 查看日志文件 `C:\ProgramData\wftpg\logs\wftpd_service.log`
4. 手动安装：`sc create wftpd binPath= "<路径>\wftpd.exe" start= auto`

### Q: 如何修改默认端口？
A: 在 GUI 的"⚙ 服务器"标签页修改，或直接编辑配置文件 `C:\ProgramData\wftpg\config.toml`

### Q: 被动模式连接失败？
A: 
1. 确保防火墙开放了被动端口范围（默认 50000-50100）
2. 在路由器上配置端口转发（如果在内网）
3. 在"⚙ 服务器"标签页调整被动端口范围
4. nat IP配置正确

### Q: 如何备份配置？
A: 备份 `C:\ProgramData\wftpg\` 目录下的所有文件：
- `config.toml` - 服务器配置
- `users.toml` - 用户配置
- `logs/` - 日志目录

### Q: 支持哪些客户端？
A: 支持所有标准 FTP/SFTP 客户端：
- FileZilla
- WinSCP
- Cyberduck
- 命令行 sftp/ftp
- 操作系统文件管理器
- UOS客户端文件管理过于老旧，还在考虑是否实现兼容**

## 故障排查

### 日志位置
- GUI 日志：`C:\ProgramData\wftpg\logs\wftpg_gui.log`
- 服务日志：`C:\ProgramData\wftpg\logs\wftpd_service.log`

### 调试模式
在 GUI 启动时使用命令行参数可查看详细日志：
```bash
wftpg.exe --verbose
```

### 服务状态检查
```powershell
# 查看服务状态
Get-Service wftpd

# 启动服务
Start-Service wftpd

# 停止服务
Stop-Service wftpd

# 重启服务
Restart-Service wftpd
```

### 卸载服务
```powershell
# 通过 GUI 卸载（推荐）
# 或在"🖥 系统服务"标签页点击"卸载服务"

# 命令行卸载
sc stop wftpd
sc delete wftpd
```

## 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](LICENSE) 文件

## 作者

- **作者**: boss@oi-io.cc

## 致谢

感谢以下开源项目的支持：
- [egui](https://github.com/emilk/egui) - 优秀的 Rust GUI 框架
- [tokio](https://tokio.rs/) - 强大的异步运行时
- [russh](https://github.com/warp-tech/russh) - 纯 Rust SSH 实现
