# 🎯 WFTPD Python 测试套件 - 立即开始

## ⚡ 3分钟快速上手

### 第1步：检查环境 (30秒)
```bash
python --version
```
确保显示 Python 3.6 或更高版本。

### 第2步：安装依赖 (1分钟)
```bash
cd wftpd-test-python
python -m pip install paramiko
```

### 第3步：验证安装 (30秒)
```bash
python test_environment.py
```
看到 "✓ 所有环境测试通过！" 即成功。

### 第4步：运行测试 (1分钟)
```bash
python wftpd_test.py
```

## 📋 一键运行脚本

### Windows 用户
双击运行：
- `run_tests.bat` (命令提示符)
- `run_tests.ps1` (PowerShell，右键"使用PowerShell运行")

### 命令行用户
```bash
# 基本运行
python wftpd_test.py

# 使用自定义配置
python wftpd_test.py my_config.json
```

## 🔧 快速配置

编辑 `test_config.json` 修改服务器信息：

```json
{
    "server": {
        "ftp_host": "你的FTP服务器IP",
        "ftp_port": 21,
        "sftp_host": "你的SFTP服务器IP", 
        "sftp_port": 2222
    },
    "user": {
        "username": "你的用户名",
        "password": "你的密码"
    }
}
```

## 📊 查看结果

测试完成后会生成：
- **控制台输出** - 实时测试结果
- **test_report.json** - 详细JSON报告
- **test_results.log** - 文本日志文件

## ❓ 常见问题

### Q: 提示找不到Python？
A: 请先安装Python 3.6+，从 https://python.org 下载

### Q: 依赖安装失败？
A: 尝试使用国内镜像：
```bash
pip install paramiko -i https://pypi.tuna.tsinghua.edu.cn/simple
```

### Q: 连接服务器失败？
A: 检查：
1. 服务器是否运行
2. IP地址和端口是否正确  
3. 防火墙设置
4. 用户名密码是否正确

### Q: 如何只测试FTP或SFTP？
A: 编辑 `wftpd_test.py`，在 `run_all_tests()` 方法中注释掉不需要的测试：
```python
# self.run_ftp_tests()  # 只测试SFTP
self.run_sftp_tests()
```

## 📚 更多文档

- `README.md` - 完整使用说明
- `QUICK_START.md` - 详细快速开始指南  
- `PROJECT_SUMMARY.md` - 项目技术总结

## 🆘 需要帮助？

1. 查看 `test_results.log` 日志文件
2. 运行 `python test_environment.py` 检查环境
3. 参考 `QUICK_START.md` 的故障排除章节

---

**就这么简单！** 🚀

开始测试吧 → `python wftpd_test.py`