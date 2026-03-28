# WFTPG FTP/SFTP 测试脚本使用说明

## 概述

这个 Python 测试脚本用于完整测试 WFTPG 的 FTP 和 SFTP 功能，包括：
- 自动启动 wftpd 服务（如果找到）
- 完整的 FTP 协议命令测试
- 完整的 SFTP 协议操作测试
- 详细的测试结果统计和报告

## 前置要求

### 1. Python 环境
- Python 3.8 或更高版本

### 2. 安装依赖
```bash
pip install -r requirements-test.txt
```

或者直接安装：
```bash
pip install paramiko
```

### 3. WFTPD 服务
确保已编译 wftpd.exe：
```bash
cargo build --release
```

## 测试配置

默认测试配置：
- **FTP 服务器**: 127.0.0.1:21
- **SFTP 服务器**: 127.0.0.1:2222
- **测试用户**: 123
- **测试密码**: 123456

如果需要修改配置，请编辑 `test_ftp_sftp.py` 文件中的以下部分：

```python
# FTP 测试配置
ftp_tester = FTPTester(
    host='127.0.0.1', 
    port=21, 
    username='123', 
    password='123456'
)

# SFTP 测试配置
sftp_tester = SFTPTester(
    host='127.0.0.1', 
    port=2222, 
    username='123', 
    password='123456'
)
```

## 运行测试

### 方法 1：直接运行
```bash
python test_ftp_sftp.py
```

### 方法 2：指定配置运行
编辑脚本后运行：
```bash
python test_ftp_sftp.py
```

## 测试流程

脚本会自动执行以下步骤：

1. **环境检查**
   - 检查 Python 依赖是否安装
   - 查找 wftpd.exe 路径

2. **启动服务**
   - 自动启动 wftpd 服务（非服务模式，作为控制台应用）
   - 等待服务完全启动

3. **FTP 功能测试**
   - 连接和登录验证
   - 基础命令：PWD, SYST, FEAT, TYPE, NOOP
   - 目录操作：MKD, CWD, RMD
   - 文件操作：STOR, RETR, LIST, NLST, SIZE, MDTM, RNFR/RNTO, DELE
   - 传输模式测试

4. **SFTP 功能测试**
   - 连接和登录验证
   - 目录操作：MKDIR, CHDIR, RMDIR
   - 文件操作：PUT, GET, LIST, STAT, LSTAT, REMOVE, RENAME
   - 权限操作：CHMOD
   - 符号链接：SYMLINK, READLINK

5. **清理和汇总**
   - 清理所有测试文件
   - 停止 wftpd 服务
   - 生成测试结果报告

## 测试结果

### 控制台输出

测试过程中会实时显示每个测试项的结果：
- ✓ 表示测试通过
- ✗ 表示测试失败，并显示失败原因

### JSON 报告

测试结果会保存到 `test_result.json` 文件，包含：
- 总体统计信息
- FTP 详细结果
- SFTP 详细结果
- 错误详情

示例结构：
```json
{
  "start_time": "2026-03-28T10:00:00.000000",
  "end_time": "2026-03-28T10:01:30.000000",
  "ftp": {
    "total": 18,
    "passed": 18,
    "failed": 0,
    "success_rate": "100.00%",
    "duration_seconds": 45.23,
    "errors": []
  },
  "sftp": {
    "total": 16,
    "passed": 15,
    "failed": 1,
    "success_rate": "93.75%",
    "duration_seconds": 42.15,
    "errors": [...]
  },
  "summary": {
    "total_tests": 34,
    "total_passed": 33,
    "total_failed": 1,
    "success_rate": "97.06%"
  }
}
```

## 测试项目详细说明

### FTP 测试项目 (18 项)

1. **登录验证** - 测试用户认证
2. **PWD 命令** - 获取当前目录
3. **SYST 系统类型** - 查询服务器类型
4. **FEAT 功能列表** - 查询支持的功能
5. **TYPE 设置类型** - 设置传输类型（ASCII/Binary）
6. **NOOP 保持连接** - 保持连接活跃
7. **MKD 创建目录** - 创建新目录
8. **CWD 切换目录** - 切换工作目录
9. **STOR 上传文件** - 上传文件到服务器
10. **LIST 列出目录** - 详细目录列表
11. **NLST 简单列表** - 简单文件名列表
12. **SIZE 获取文件大小** - 查询文件大小
13. **MDTM 获取修改时间** - 查询文件修改时间
14. **RETR 下载文件** - 从服务器下载文件
15. **RNFR/RNTO 重命名** - 重命名文件
16. **DELE 删除文件** - 删除文件
17. **RMD 删除目录** - 删除空目录
18. **传输模式** - 测试主动/被动模式

### SFTP 测试项目 (16 项)

1. **登录验证** - 测试 SSH 认证
2. **PWD 获取路径** - 获取当前工作目录
3. **MKDIR 创建目录** - 创建新目录
4. **CHDIR 切换目录** - 切换工作目录
5. **PUT 上传文件** - 上传文件到服务器
6. **GET 下载文件** - 从服务器下载文件
7. **LIST 列出目录** - 列出目录内容
8. **STAT 获取属性** - 获取文件详细信息
9. **CHMOD 修改权限** - 修改文件权限
10. **LSTAT 链接属性** - 获取符号链接属性
11. **RENAME 重命名** - 重命名文件/目录
12. **REMOVE 删除文件** - 删除文件
13. **RMDIR 删除目录** - 删除目录
14. **SYMLINK 符号链接** - 创建符号链接
15. **READLINK 读取链接** - 读取符号链接目标
16. **权限验证** - 验证权限设置

## 手动运行测试

如果 wftpd 服务已经在运行，可以手动运行测试：

```python
from test_ftp_sftp import FTPTester, SFTPTester

# FTP 测试
ftp = FTPTester(host='127.0.0.1', port=21, username='123', password='123456')
result = ftp.run_all_tests()
print(f"FTP 测试通过率：{result.passed}/{result.total}")

# SFTP 测试
sftp = SFTPTester(host='127.0.0.1', port=2222, username='123', password='123456')
result = sftp.run_all_tests()
print(f"SFTP 测试通过率：{result.passed}/{result.total}")
```

## 常见问题

### 1. 连接失败
**问题**: 无法连接到 FTP/SFTP 服务器

**解决方法**:
- 确认 wftpd 服务正在运行
- 检查防火墙设置，确保端口 21 和 2222 未被阻止
- 检查配置文件 `C:\ProgramData\wftpg\config.toml` 中的端口设置

### 2. 认证失败
**问题**: 登录失败

**解决方法**:
- 确认测试用户存在且密码正确
- 检查用户配置文件 `C:\ProgramData\wftpg\users.json`
- 确保用户状态为 enabled

### 3. 权限错误
**问题**: 文件操作失败

**解决方法**:
- 检查用户的权限设置
- 确保用户主目录存在且可访问
- 以管理员身份运行脚本

### 4. wftpd 启动失败
**问题**: 无法自动启动 wftpd

**解决方法**:
- 手动启动 wftpd 服务
- 检查是否有其他服务占用了端口
- 查看日志文件 `C:\ProgramData\wftpg\logs\`

## 自定义测试

### 添加新的测试用例

在相应的测试类中添加新方法：

```python
def test_custom_feature(self):
    """测试自定义功能"""
    test_name = "自定义测试"
    try:
        # 测试代码
        self.result.add_pass(test_name)
    except Exception as e:
        self.result.add_fail(test_name, str(e))
```

然后在 `run_all_tests()` 方法中调用它。

## 注意事项

1. **管理员权限**: 某些测试可能需要管理员权限
2. **端口占用**: 确保端口 21 和 2222 未被其他程序占用
3. **防火墙**: 临时关闭防火墙或添加例外规则
4. **测试清理**: 测试完成后会自动清理所有测试文件
5. **并发测试**: 不要同时运行多个测试实例

## 技术支持

如遇到问题，请查看：
- 日志文件：`C:\ProgramData\wftpg\logs\`
- 测试结果：`test_result.json`
- 项目文档：README.md
