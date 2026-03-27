# WFTPG - SFTP/FTP 服务器管理工具

[![Rust](https://img.shields.io/badge/Rust-2024 Edition-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%2010%2F11-blue.svg)](https://www.microsoft.com/windows)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

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

## 使用说明

### 快速开始

1. **启动程序** - 以管理员身份运行 `wftpg.exe`
2. **配置服务器** - 在"服务器"标签页启用 SFTP/FTP
3. **添加用户** - 在"用户管理"标签页添加用户
4. **启动服务** - 点击"启动"按钮或安装为系统服务

### 默认配置

- **SFTP 端口**: 2222
- **FTP 端口**: 21
- **被动端口范围**: 50000-50100
- **配置目录**: `C:\ProgramData\wftpg\`
- **日志目录**: `C:\ProgramData\wftpg\logs\`

### 测试账户

- **用户名**: `123`
- **密码**: `123123`

## 项目结构

```
wftpg-egui/
├── src/
│   ├── core/           # 核心功能模块
│   │   ├── config.rs       # 配置管理
│   │   ├── users.rs        # 用户管理
│   │   ├── sftp_server.rs  # SFTP 服务器
│   │   ├── ftp_server/     # FTP 服务器
│   │   ├── server_manager.rs # 服务管理
│   │   ├── quota.rs        # 配额管理
│   │   ├── logger.rs       # 日志系统
│   │   └── path_utils.rs   # 路径处理
│   ├── gui_egui/       # GUI 界面模块
│   │   ├── server_tab.rs   # 服务器配置界面
│   │   ├── user_tab.rs     # 用户管理界面
│   │   ├── security_tab.rs # 安全设置界面
│   │   └── ...
│   ├── gui_main.rs     # GUI 程序入口
│   ├── service_main.rs # 服务程序入口
│   └── lib.rs          # 库入口
├── ui/                 # 界面资源
└── Cargo.toml          # 项目配置
```

## 技术栈

- **Rust** - 系统编程语言 (Edition 2024)
- **egui/eframe** - 即时模式 GUI 框架
- **tokio** - 异步运行时
- **russh** - SSH/SFTP 协议实现
- **native-tls** - TLS/SSL 支持
- **serde** - 序列化/反序列化
- **argon2** - 密码哈希

## 安全特性

- **密码加密** - 使用 Argon2 算法存储密码
- **路径隔离** - 用户只能访问自己的主目录 (chroot)
- **IP 过滤** - 支持允许/拒绝 IP 列表
- **防路径逃逸** - 防止 `..` 等路径遍历攻击
- **符号链接检查** - 验证符号链接目标

## 开发规范

- 所有文件使用 UTF-8 编码
- 代码遵循 Rust 2024 Edition 规范
- 每次修改后运行 `cargo clippy` 检查
- 禁止隐藏告警，禁止使用 `#[allow(dead_code)]`
- 版本号格式: `0.xx.yy`，每次修改更新 `yy`

## 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](LICENSE) 文件

## 作者

- **作者**: boss@oi-io.cc

## 致谢

感谢以下开源项目的支持：
- [egui](https://github.com/emilk/egui) - 优秀的 Rust GUI 框架
- [tokio](https://tokio.rs/) - 强大的异步运行时
- [russh](https://github.com/warp-tech/russh) - 纯 Rust SSH 实现
