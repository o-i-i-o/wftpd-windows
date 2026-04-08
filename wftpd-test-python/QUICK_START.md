# WFTPD Python 测试套件 - 快速开始指南

## 📋 目录
- [环境要求](#环境要求)
- [安装步骤](#安装步骤)
- [配置说明](#配置说明)
- [运行测试](#运行测试)
- [测试结果](#测试结果)
- [故障排除](#故障排除)

## 🔧 环境要求

### 必需软件
- **Python 3.6+** (推荐 3.8+)
- **pip** (Python包管理器)

### 支持的操作系统
- Windows 10/11
- Linux (Ubuntu/CentOS等)
- macOS

## 📦 安装步骤

### 1. 克隆或下载项目
```bash
# 如果从git仓库获取
git clone <repository-url>
cd wftpd-test-python

# 或者直接解压下载的文件包
```

### 2. 验证Python环境
```bash
python --version
# 应该显示 Python 3.x.x
```

### 3. 安装依赖
```bash
# 方法1: 使用requirements.txt
pip install -r requirements.txt

# 方法2: 手动安装
pip install paramiko
```

### 4. 验证安装
```bash
python test_environment.py
```

如果看到 "✓ 所有环境测试通过！"，说明安装成功。

## ⚙️ 配置说明

### 配置文件位置
`test_config.json` - 主配置文件

### 基本配置示例
```json
{
    "server": {
        "ftp_host": "127.0.0.1",     // FTP服务器IP
        "ftp_port": 21,               // FTP端口
        "sftp_host": "127.0.0.1",     // SFTP服务器IP  
        "sftp_port": 2222,            // SFTP端口
        "timeout": 30,                // 超时时间(秒)
        "use_ftps": false,            // 是否使用FTPS
        "ftps_implicit": false        // 是否使用隐式FTPS
    },
    "user": {
        "username": "testuser",       // 用户名
        "password": "password123",    // 密码
        "home_dir": "/home/testuser"  // 用户主目录
    },
    "test_settings": {
        "test_data_dir": "./testdata",    // 测试数据目录
        "log_file": "./test_results.log", // 日志文件路径
        "max_retries": 3,                 // 最大重试次数
        "retry_delay": 2,                 // 重试延迟(秒)
        "create_test_files": true,        // 自动创建测试文件
        "cleanup_after_test": true        // 测试后清理
    }
}
```

### 常见配置场景

#### 本地测试
```json
{
    "server": {
        "ftp_host": "127.0.0.1",
        "ftp_port": 21,
        "sftp_host": "127.0.0.1", 
        "sftp_port": 2222
    },
    "user": {
        "username": "admin",
        "password": "admin123"
    }
}
```

#### 远程服务器测试
```json
{
    "server": {
        "ftp_host": "192.168.1.100",
        "ftp_port": 21,
        "sftp_host": "192.168.1.100",
        "sftp_port": 22,
        "timeout": 60
    },
    "user": {
        "username": "ftpuser",
        "password": "securepass"
    }
}
```

#### FTPS加密测试
```json
{
    "server": {
        "ftp_host": "127.0.0.1",
        "ftp_port": 21,
        "use_ftps": true,
        "ftps_implicit": false
    }
}
```

## 🚀 运行测试

### 方法1: 直接运行Python脚本
```bash
python wftpd_test.py
```

### 方法2: 使用批处理文件 (Windows)
```bash
run_tests.bat
```

### 方法3: 使用PowerShell脚本 (Windows)
```bash
.\run_tests.ps1
```

### 方法4: 指定自定义配置
```bash
python wftpd_test.py my_custom_config.json
```

## 📊 测试结果

### 控制台输出示例
```
==================================================
WFTPD FTP/SFTP 测试套件
==================================================
FTP服务器: 127.0.0.1:21
SFTP服务器: 127.0.0.1:2222
用户名: testuser
测试开始时间: 2026-04-08 10:30:00

==============================
FTP 测试模块
==============================
✓ FTP基本连接 - 耗时: 0.05s
✓ FTP用户认证 - 耗时: 0.12s
✓ FTP目录操作 - 耗时: 0.08s
✓ FTP文件传输 - 耗时: 0.15s
✓ FTP被动模式 - 耗时: 0.03s

==============================
SFTP 测试模块
==============================
✓ SFTP基本连接 - 耗时: 0.25s
✓ SFTP目录操作 - 耗时: 0.18s
✓ SFTP文件传输 - 耗时: 0.22s
✓ SFTP文件权限 - 耗时: 0.12s

==================================================
测试报告
==================================================
总测试数: 9
通过: 9
失败: 0
通过率: 100.0%
总耗时: 1.20秒
```

### 生成的文件
- `test_report.json` - 详细的JSON格式测试报告
- `test_results.log` - 文本格式日志文件
- `testdata/` - 测试过程中使用的临时文件

### 测试报告内容
JSON报告包含：
- 测试摘要统计
- 每个测试的详细结果
- 执行时间信息
- 错误详情（如果有）
- 配置信息

## 🔍 测试项目详解

### FTP测试项目
1. **基本连接测试** - 验证网络连通性和服务可用性
2. **用户认证测试** - 验证凭据正确性
3. **目录操作测试** - 创建、删除、切换目录功能
4. **文件传输测试** - 上传、下载、完整性验证
5. **被动模式测试** - PASV模式数据传输

### SFTP测试项目
1. **基本连接测试** - SSH/SFTP连接建立
2. **目录操作测试** - 远程目录管理
3. **文件传输测试** - 安全文件传输
4. **文件权限测试** - 权限读取和修改

## 🛠️ 故障排除

### 常见问题及解决方案

#### 1. 连接失败
**症状**: "Connection refused" 或 "Timeout"
**解决方案**:
- 确认服务器正在运行
- 检查防火墙设置
- 验证IP地址和端口是否正确
- 测试网络连通性: `ping server_ip`

#### 2. 认证失败
**症状**: "Authentication failed"
**解决方案**:
- 检查用户名和密码
- 确认账户状态正常
- 验证用户权限设置

#### 3. 依赖安装失败
**症状**: pip安装错误
**解决方案**:
```bash
# 升级pip
python -m pip install --upgrade pip

# 使用国内镜像
pip install -r requirements.txt -i https://pypi.tuna.tsinghua.edu.cn/simple
```

#### 4. 权限错误
**症状**: 文件或目录访问被拒绝
**解决方案**:
- 以管理员身份运行
- 检查文件和目录权限
- 确认用户对测试目录有读写权限

#### 5. SFTP连接问题
**症状**: SFTP连接超时或失败
**解决方案**:
- 确认SSH服务运行状态
- 检查SSH密钥配置
- 验证SFTP子系统配置

### 调试技巧

#### 启用详细日志
修改代码中的日志级别：
```python
logging.basicConfig(level=logging.DEBUG)
```

#### 单独测试FTP或SFTP
注释掉不需要的测试模块：
```python
# 在 run_all_tests() 方法中
# self.run_ftp_tests()  # 只运行SFTP测试
self.run_sftp_tests()
```

#### 网络连接测试
```bash
# 测试FTP端口
telnet server_ip 21

# 测试SFTP端口  
telnet server_ip 2222
```

## 📝 最佳实践

### 1. 配置管理
- 为不同环境创建单独的配置文件
- 不要在版本控制中提交敏感信息
- 定期备份重要配置

### 2. 测试执行
- 在生产环境变更前运行完整测试
- 定期检查测试结果趋势
- 保存历史测试报告用于对比

### 3. 安全考虑
- 使用强密码
- 定期更新依赖包
- 限制测试账户权限
- 清理测试数据

### 4. 性能优化
- 调整超时参数适应网络环境
- 合理设置并发连接数
- 监控测试资源使用情况

## 🆘 获取帮助

### 文档资源
- 查看 `README.md` 获取详细说明
- 检查 `test_report.json` 了解测试结果
- 阅读 `test_results.log` 获取调试信息

### 技术支持
如遇到无法解决的问题：
1. 检查上述故障排除章节
2. 查看日志文件获取详细错误信息
3. 确认服务器端配置正确
4. 验证网络连接状态

## 📄 许可证
本测试套件遵循与原WFTPD项目相同的许可证条款。

---

**祝您使用愉快！** 🎉