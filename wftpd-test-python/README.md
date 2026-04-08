# WFTPD Python 测试套件

## 简介
这是一个完整的Python版本的WFTPD FTP/SFTP测试套件，提供标准化的测试流程和集中化的配置管理。

## 特性
- ✅ 完整的FTP功能测试（连接、认证、目录操作、文件传输、被动模式）
- ✅ 完整的SFTP功能测试（连接、目录操作、文件传输、权限管理）
- ✅ 集中化的服务端和用户信息管理
- ✅ 详细的测试报告和日志记录
- ✅ 自动化的测试环境准备和清理
- ✅ 支持FTPS加密连接
- ✅ 可配置的测试参数

## 安装依赖

```bash
pip install -r requirements.txt
```

或者手动安装：
```bash
pip install paramiko
```

## 配置说明

### 配置文件 (test_config.json)

```json
{
    "server": {
        "ftp_host": "127.0.0.1",      // FTP服务器地址
        "ftp_port": 21,                // FTP端口
        "sftp_host": "127.0.0.1",      // SFTP服务器地址  
        "sftp_port": 2222,             // SFTP端口
        "timeout": 30,                 // 连接超时时间(秒)
        "use_ftps": false,             // 是否使用FTPS
        "ftps_implicit": false         // 是否使用隐式FTPS
    },
    "user": {
        "username": "123",             // 用户名
        "password": "123123",          // 密码
        "home_dir": "/test"            // 用户主目录
    },
    "test_settings": {
        "test_data_dir": "./testdata", // 测试数据目录
        "log_file": "./test_results.log", // 日志文件
        "max_retries": 3,              // 最大重试次数
        "retry_delay": 2,              // 重试延迟(秒)
        "create_test_files": true,     // 是否创建测试文件
        "cleanup_after_test": true     // 测试后是否清理
    }
}
```

## 使用方法

### 基本用法
```bash
python wftpd_test.py
```

### 指定配置文件
```bash
python wftpd_test.py my_config.json
```

### 自定义配置示例
创建 `custom_config.json`:
```json
{
    "server": {
        "ftp_host": "192.168.1.100",
        "ftp_port": 21,
        "sftp_host": "192.168.1.100", 
        "sftp_port": 2222,
        "timeout": 60
    },
    "user": {
        "username": "admin",
        "password": "secure_password"
    }
}
```

运行测试：
```bash
python wftpd_test.py custom_config.json
```

## 测试项目

### FTP测试
1. **基本连接测试** - 验证FTP服务器可达性
2. **用户认证测试** - 验证用户名密码正确性
3. **目录操作测试** - 创建、删除、切换目录
4. **文件传输测试** - 上传、下载、内容验证
5. **被动模式测试** - PASV模式连接测试

### SFTP测试
1. **基本连接测试** - 验证SFTP服务器可达性
2. **目录操作测试** - 创建、删除、列出目录
3. **文件传输测试** - 上传、下载、内容验证
4. **文件权限测试** - 文件属性读取和修改

## 输出说明

### 控制台输出
```
==================================================
WFTPD FTP/SFTP 测试套件
==================================================
FTP服务器: 127.0.0.1:21
SFTP服务器: 127.0.0.1:2222
用户名: 123
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
- `test_results.log` - 详细日志文件
- `test_report.json` - JSON格式测试报告
- `testdata/` - 测试数据目录

## 故障排除

### 常见问题

1. **连接失败**
   - 检查服务器是否运行
   - 验证IP地址和端口是否正确
   - 检查防火墙设置

2. **认证失败**
   - 确认用户名密码正确
   - 检查用户账户状态

3. **SFTP连接问题**
   - 确认SFTP服务已启用
   - 检查SSH密钥配置

4. **权限错误**
   - 验证用户对测试目录有读写权限
   - 检查SELinux/AppArmor设置

### 调试模式
修改配置文件中的日志级别或直接在代码中添加调试信息。

## 扩展开发

### 添加新的测试用例
在相应的测试类中添加方法：

```python
def new_test_method(self) -> bool:
    test_name = "新测试名称"
    start_time = time.time()
    
    try:
        # 测试逻辑
        duration = time.time() - start_time
        self.result.add_result(test_name, True, duration, details="测试详情")
        return True
    except Exception as e:
        duration = time.time() - start_time
        self.result.add_result(test_name, False, duration, str(e))
        return False
```

### 自定义报告格式
修改 `generate_report()` 方法来调整报告输出格式。

## 版本历史

### v1.0 (2026-04-08)
- ✅ 初始版本发布
- ✅ 完整的FTP/SFTP测试功能
- ✅ 集中化配置管理
- ✅ 详细测试报告生成

## 技术支持

如遇到问题，请检查：
1. 服务器运行状态
2. 网络连接
3. 配置文件参数
4. 日志文件详细信息

## 许可证
本项目遵循与原WFTPD项目相同的许可证条款。