# WFTPG FTP/SFTP 测试脚本

## 快速开始

### 1. 安装依赖
```bash
pip install -r requirements-test.txt
```

### 2. 确保 wftpd 已编译
```bash
cargo build --release
```

### 3. 运行测试
```bash
python test_ftp_sftp.py
```

## 默认配置

- **FTP**: 127.0.0.1:21
- **SFTP**: 127.0.0.1:2222
- **用户**: 123
- **密码**: 123456

## 测试内容

### FTP (18 项测试)
- ✓ 登录验证
- ✓ 目录操作（创建、切换、删除）
- ✓ 文件操作（上传、下载、列表、删除、重命名）
- ✓ 文件属性（大小、修改时间）
- ✓ FTP 命令（PWD, SYST, FEAT, TYPE, NOOP 等）

### SFTP (16 项测试)
- ✓ 登录验证
- ✓ 目录操作（创建、切换、删除）
- ✓ 文件操作（上传、下载、列表、删除、重命名）
- ✓ 文件属性（STAT, LSTAT, CHMOD）
- ✓ 符号链接（SYMLINK, READLINK）

## 测试结果

测试完成后会生成 `test_result.json` 文件，包含详细的测试结果统计。

## 文档

详细使用说明请查看 [TEST_GUIDE.md](TEST_GUIDE.md)
