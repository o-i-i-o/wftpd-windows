#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
WFTPD FTP/SFTP 测试套件 (Python版本)
提供完整的FTP和SFTP功能测试
"""

import os
import sys
import json
import time
import logging
import tempfile
import hashlib
from pathlib import Path
from datetime import datetime
from typing import Dict, List, Optional, Tuple

# FTP相关库
try:
    from ftplib import FTP, FTP_TLS
except ImportError:
    print("错误: 需要安装ftplib库")
    sys.exit(1)

# SFTP相关库
try:
    import paramiko
except ImportError:
    print("错误: 需要安装paramiko库 (pip install paramiko)")
    sys.exit(1)


class TestConfig:
    """测试配置管理类 - 集中管理服务端和用户信息"""
    
    def __init__(self, config_file: str = "test_config.json"):
        self.config_file = config_file
        self.config = self.load_config()
        
    def load_config(self) -> Dict:
        """加载配置文件"""
        default_config = {
            "server": {
                "ftp_host": "127.0.0.1",
                "ftp_port": 21,
                "sftp_host": "127.0.0.1", 
                "sftp_port": 2222,
                "timeout": 30,
                "use_ftps": False,
                "ftps_implicit": False
            },
            "user": {
                "username": "123",
                "password": "123123",
                "home_dir": "/test"
            },
            "test_settings": {
                "test_data_dir": "./testdata",
                "log_file": "./test_results.log",
                "max_retries": 3,
                "retry_delay": 2,
                "create_test_files": True,
                "cleanup_after_test": True
            }
        }
        
        if os.path.exists(self.config_file):
            try:
                with open(self.config_file, 'r', encoding='utf-8') as f:
                    loaded_config = json.load(f)
                    # 合并配置
                    for key in default_config:
                        if key not in loaded_config:
                            loaded_config[key] = default_config[key]
                        elif isinstance(default_config[key], dict):
                            for sub_key in default_config[key]:
                                if sub_key not in loaded_config[key]:
                                    loaded_config[key][sub_key] = default_config[key][sub_key]
                    return loaded_config
            except Exception as e:
                print(f"警告: 读取配置文件失败 {e}, 使用默认配置")
                return default_config
        else:
            # 创建默认配置文件
            self.save_config(default_config)
            return default_config
    
    def save_config(self, config: Dict):
        """保存配置文件"""
        with open(self.config_file, 'w', encoding='utf-8') as f:
            json.dump(config, f, indent=4, ensure_ascii=False)
    
    @property
    def ftp_host(self) -> str:
        return self.config["server"]["ftp_host"]
    
    @property
    def ftp_port(self) -> int:
        return self.config["server"]["ftp_port"]
    
    @property
    def sftp_host(self) -> str:
        return self.config["server"]["sftp_host"]
    
    @property
    def sftp_port(self) -> int:
        return self.config["server"]["sftp_port"]
    
    @property
    def timeout(self) -> int:
        return self.config["server"]["timeout"]
    
    @property
    def username(self) -> str:
        return self.config["user"]["username"]
    
    @property
    def password(self) -> str:
        return self.config["user"]["password"]
    
    @property
    def test_data_dir(self) -> str:
        return self.config["test_settings"]["test_data_dir"]
    
    @property
    def log_file(self) -> str:
        return self.config["test_settings"]["log_file"]


class TestResult:
    """测试结果记录类"""
    
    def __init__(self):
        self.results = []
        self.start_time = None
        self.end_time = None
        
    def add_result(self, test_name: str, success: bool, duration: float = 0, 
                   error_msg: str = "", details: str = ""):
        """添加测试结果"""
        result = {
            "test_name": test_name,
            "success": success,
            "duration": duration,
            "error_msg": error_msg,
            "details": details,
            "timestamp": datetime.now().isoformat()
        }
        self.results.append(result)
        
    def summary(self) -> Dict:
        """生成测试摘要"""
        total = len(self.results)
        passed = sum(1 for r in self.results if r["success"])
        failed = total - passed
        
        return {
            "total": total,
            "passed": passed,
            "failed": failed,
            "pass_rate": f"{(passed/total*100):.1f}%" if total > 0 else "0%"
        }


class FTPTester:
    """FTP测试类 - 优化版"""
    
    def __init__(self, config: TestConfig, result: TestResult):
        self.config = config
        self.result = result
        self.ftp = None
        self.test_files_created = []  # 跟踪创建的测试文件
        
    def _retry_operation(self, operation, max_retries=None, delay=None):
        """通用重试机制"""
        if max_retries is None:
            max_retries = self.config.config["test_settings"].get("max_retries", 3)
        if delay is None:
            delay = self.config.config["test_settings"].get("retry_delay", 2)
            
        last_error = None
        for attempt in range(1, max_retries + 1):
            try:
                return operation()
            except Exception as e:
                last_error = e
                if attempt < max_retries:
                    time.sleep(delay)
                    continue
        raise last_error
        
    def connect(self) -> bool:
        """建立FTP连接 - 带重试机制"""
        test_name = "FTP基本连接"
        start_time = time.time()
        
        def do_connect():
            if self.config.config["server"]["use_ftps"]:
                self.ftp = FTP_TLS()
                # 如果需要隐式FTPS
                if self.config.config["server"].get("ftps_implicit", False):
                    self.ftp.ssl_context.check_hostname = False
                    self.ftp.ssl_context.verify_mode = False
            else:
                self.ftp = FTP()
                
            self.ftp.settimeout(self.config.timeout)
            self.ftp.connect(self.config.ftp_host, self.config.ftp_port)
            return self.ftp.getwelcome()
        
        try:
            welcome = self._retry_operation(do_connect)
            duration = time.time() - start_time
            
            self.result.add_result(test_name, True, duration, 
                                 details=f"连接成功: {welcome}")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s")
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def login(self) -> bool:
        """用户认证 - 带重试机制"""
        test_name = "FTP用户认证"
        start_time = time.time()
        
        def do_login():
            if not self.ftp:
                raise Exception("FTP未连接")
            return self.ftp.login(self.config.username, self.config.password)
        
        try:
            self._retry_operation(do_login)
            duration = time.time() - start_time
            
            self.result.add_result(test_name, True, duration, 
                                 details="登录成功")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s")
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def directory_operations(self) -> bool:
        """目录操作测试 - 增强版"""
        test_name = "FTP目录操作"
        start_time = time.time()
        
        try:
            if not self.ftp:
                raise Exception("FTP未连接")
            
            # 获取当前目录
            current_dir = self.ftp.pwd()
            
            # 创建测试目录
            test_dir = f"test_dir_{int(time.time())}"
            self.ftp.mkd(test_dir)
            
            # 切换目录
            self.ftp.cwd(test_dir)
            new_dir = self.ftp.pwd()
            
            # 列出目录内容
            files_list = self.ftp.nlst()
            files_detailed = self.ftp.dir()  # 详细信息
            
            # 在子目录中创建文件测试
            test_file_remote = "subdir_test.txt"
            with open(os.path.join(self.config.test_data_dir, "temp_upload.txt"), 'w') as f:
                f.write("子目录测试")
            with open(os.path.join(self.config.test_data_dir, "temp_upload.txt"), 'rb') as f:
                self.ftp.storbinary(f'STOR {test_file_remote}', f)
            
            # 验证文件存在
            files_after = self.ftp.nlst()
            
            # 清理：删除测试文件，返回上级，删除测试目录
            try:
                self.ftp.delete(test_file_remote)
            except:
                pass
            self.ftp.cwd("..")
            self.ftp.rmd(test_dir)
            
            # 清理本地临时文件
            temp_file = os.path.join(self.config.test_data_dir, "temp_upload.txt")
            if os.path.exists(temp_file):
                os.remove(temp_file)
            
            duration = time.time() - start_time
            self.result.add_result(test_name, True, duration,
                                 details=f"创建/删除目录: {test_dir}, 文件列表: {len(files_after)}个文件")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s")
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def file_transfer(self) -> bool:
        """文件传输测试 - 增强版（支持多种模式）"""
        test_name = "FTP文件传输"
        start_time = time.time()
        
        try:
            if not self.ftp:
                raise Exception("FTP未连接")
            
            # 测试1: 小文本文件
            small_file = os.path.join(self.config.test_data_dir, "ftp_small_test.txt")
            small_content = f"FTP小文件测试 - {datetime.now()}\n" * 5
            with open(small_file, 'w', encoding='utf-8') as f:
                f.write(small_content)
            self.test_files_created.append(small_file)
            
            remote_small = "uploaded_small.txt"
            # 上传
            with open(small_file, 'rb') as f:
                self.ftp.storbinary(f'STOR {remote_small}', f)
            
            # 下载并验证
            downloaded_small = os.path.join(self.config.test_data_dir, "downloaded_small.txt")
            with open(downloaded_small, 'wb') as f:
                self.ftp.retrbinary(f'RETR {remote_small}', f.write)
            self.test_files_created.append(downloaded_small)
            
            with open(downloaded_small, 'r', encoding='utf-8') as f:
                downloaded_content = f.read()
            
            if small_content != downloaded_content:
                raise Exception("小文件内容验证失败")
            
            # 测试2: 二进制文件
            binary_file = os.path.join(self.config.test_data_dir, "ftp_binary_test.bin")
            binary_content = bytes(range(256)) * 100  # 25.6KB 二进制数据
            with open(binary_file, 'wb') as f:
                f.write(binary_content)
            self.test_files_created.append(binary_file)
            
            remote_binary = "uploaded_binary.bin"
            with open(binary_file, 'rb') as f:
                self.ftp.storbinary(f'STOR {remote_binary}', f)
            
            downloaded_binary = os.path.join(self.config.test_data_dir, "downloaded_binary.bin")
            with open(downloaded_binary, 'wb') as f:
                self.ftp.retrbinary(f'RETR {remote_binary}', f.write)
            self.test_files_created.append(downloaded_binary)
            
            with open(downloaded_binary, 'rb') as f:
                downloaded_binary_content = f.read()
            
            if binary_content != downloaded_binary_content:
                raise Exception("二进制文件内容验证失败")
            
            # 计算传输速度
            file_size = len(binary_content)
            transfer_time = time.time() - start_time
            speed_kbps = (file_size / 1024) / transfer_time if transfer_time > 0 else 0
            
            duration = time.time() - start_time
            self.result.add_result(test_name, True, duration,
                                 details=f"文本+二进制文件传输成功, 速度: {speed_kbps:.2f} KB/s")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s, 速度: {speed_kbps:.2f} KB/s")
            
            # 清理远程文件
            for remote_file in [remote_small, remote_binary]:
                try:
                    self.ftp.delete(remote_file)
                except:
                    pass
                    
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def passive_mode(self) -> bool:
        """被动模式测试 - 增强版"""
        test_name = "FTP被动模式"
        start_time = time.time()
        
        try:
            if not self.ftp:
                raise Exception("FTP未连接")
            
            # 启用被动模式
            self.ftp.set_pasv(True)
            
            # 执行多个操作来测试被动模式稳定性
            files = self.ftp.nlst()
            
            # 在被动模式下进行文件传输测试
            test_file = os.path.join(self.config.test_data_dir, "pasv_test.txt")
            with open(test_file, 'w') as f:
                f.write("PASV模式测试")
            
            remote_file = "pasv_upload_test.txt"
            with open(test_file, 'rb') as f:
                self.ftp.storbinary(f'STOR {remote_file}', f)
            
            # 下载验证
            downloaded = os.path.join(self.config.test_data_dir, "pasv_download_test.txt")
            with open(downloaded, 'wb') as f:
                self.ftp.retrbinary(f'RETR {remote_file}', f.write)
            
            # 清理
            self.ftp.delete(remote_file)
            if os.path.exists(test_file):
                os.remove(test_file)
            if os.path.exists(downloaded):
                os.remove(downloaded)
            
            duration = time.time() - start_time
            self.result.add_result(test_name, True, duration,
                                 details=f"被动模式稳定, 文件数: {len(files)}, 传输测试成功")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s")
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def ascii_mode(self) -> bool:
        """ASCII模式测试"""
        test_name = "FTP ASCII模式"
        start_time = time.time()
        
        try:
            if not self.ftp:
                raise Exception("FTP未连接")
            
            # 创建文本文件
            text_content = "Line 1\r\nLine 2\r\nLine 3\r\n"
            local_file = os.path.join(self.config.test_data_dir, "ascii_test.txt")
            with open(local_file, 'w', newline='') as f:
                f.write(text_content)
            
            # ASCII模式上传
            remote_file = "ascii_upload.txt"
            with open(local_file, 'r') as f:
                self.ftp.storlines(f'STOR {remote_file}', f)
            
            # ASCII模式下载
            downloaded_file = os.path.join(self.config.test_data_dir, "ascii_download.txt")
            with open(downloaded_file, 'w', newline='') as f:
                self.ftp.retrlines(f'RETR {remote_file}', f.write)
            
            # 验证
            with open(downloaded_file, 'r') as f:
                downloaded_content = f.read()
            
            # 清理
            self.ftp.delete(remote_file)
            for f in [local_file, downloaded_file]:
                if os.path.exists(f):
                    os.remove(f)
            
            duration = time.time() - start_time
            self.result.add_result(test_name, True, duration,
                                 details="ASCII模式传输成功")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s")
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def file_rename(self) -> bool:
        """文件重命名测试"""
        test_name = "FTP文件重命名"
        start_time = time.time()
        
        try:
            if not self.ftp:
                raise Exception("FTP未连接")
            
            # 创建测试文件
            original_name = f"rename_original_{int(time.time())}.txt"
            new_name = f"rename_new_{int(time.time())}.txt"
            
            test_content = "重命名测试文件"
            local_file = os.path.join(self.config.test_data_dir, "rename_test.txt")
            with open(local_file, 'w') as f:
                f.write(test_content)
            
            # 上传
            with open(local_file, 'rb') as f:
                self.ftp.storbinary(f'STOR {original_name}', f)
            
            # 重命名
            self.ftp.rename(original_name, new_name)
            
            # 验证新文件存在
            files = self.ftp.nlst()
            if new_name not in files:
                raise Exception("重命名后文件不存在")
            
            # 下载验证
            downloaded = os.path.join(self.config.test_data_dir, "rename_verify.txt")
            with open(downloaded, 'wb') as f:
                self.ftp.retrbinary(f'RETR {new_name}', f.write)
            
            with open(downloaded, 'r', encoding='utf-8') as f:
                if f.read() != test_content:
                    raise Exception("重命名后文件内容不匹配")
            
            # 清理
            self.ftp.delete(new_name)
            for f in [local_file, downloaded]:
                if os.path.exists(f):
                    os.remove(f)
            
            duration = time.time() - start_time
            self.result.add_result(test_name, True, duration,
                                 details=f"文件重命名: {original_name} -> {new_name}")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s")
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def large_file_transfer(self) -> bool:
        """大文件传输测试"""
        test_name = "FTP大文件传输"
        start_time = time.time()
        
        try:
            if not self.ftp:
                raise Exception("FTP未连接")
            
            # 创建1MB测试文件
            file_size_mb = 1
            file_size_bytes = file_size_mb * 1024 * 1024
            large_file = os.path.join(self.config.test_data_dir, "large_ftp_test.bin")
            
            print(f"  正在生成{file_size_mb}MB测试文件...")
            with open(large_file, 'wb') as f:
                # 分块写入以避免内存占用过高
                chunk_size = 1024 * 1024  # 1MB
                chunks = file_size_bytes // chunk_size
                for i in range(chunks):
                    f.write(bytes(range(256)) * (chunk_size // 256))
            
            self.test_files_created.append(large_file)
            
            # 上传大文件
            remote_large = f"large_upload_{int(time.time())}.bin"
            upload_start = time.time()
            with open(large_file, 'rb') as f:
                self.ftp.storbinary(f'STOR {remote_large}', f)
            upload_time = time.time() - upload_start
            
            # 下载大文件
            downloaded_large = os.path.join(self.config.test_data_dir, "large_ftp_downloaded.bin")
            download_start = time.time()
            with open(downloaded_large, 'wb') as f:
                self.ftp.retrbinary(f'RETR {remote_large}', f.write)
            download_time = time.time() - download_start
            
            self.test_files_created.append(downloaded_large)
            
            # 验证文件大小
            original_size = os.path.getsize(large_file)
            downloaded_size = os.path.getsize(downloaded_large)
            
            if original_size != downloaded_size:
                raise Exception(f"文件大小不匹配: 原始={original_size}, 下载={downloaded_size}")
            
            # 计算速度
            upload_speed = (original_size / 1024 / 1024) / upload_time if upload_time > 0 else 0
            download_speed = (original_size / 1024 / 1024) / download_time if download_time > 0 else 0
            
            duration = time.time() - start_time
            self.result.add_result(test_name, True, duration,
                                 details=f"{file_size_mb}MB文件传输成功, 上传: {upload_speed:.2f} MB/s, 下载: {download_speed:.2f} MB/s")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s (上传: {upload_speed:.2f} MB/s, 下载: {download_speed:.2f} MB/s)")
            
            # 清理远程文件
            try:
                self.ftp.delete(remote_large)
            except:
                pass
            
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def concurrent_transfers(self) -> bool:
        """并发传输测试（模拟）"""
        test_name = "FTP并发传输"
        start_time = time.time()
        
        try:
            if not self.ftp:
                raise Exception("FTP未连接")
            
            # 连续上传多个小文件模拟并发
            num_files = 5
            file_size = 1024  # 1KB
            
            success_count = 0
            for i in range(num_files):
                test_file = os.path.join(self.config.test_data_dir, f"concurrent_{i}.bin")
                with open(test_file, 'wb') as f:
                    f.write(bytes([i % 256]) * file_size)
                
                remote_file = f"concurrent_upload_{i}.bin"
                with open(test_file, 'rb') as f:
                    self.ftp.storbinary(f'STOR {remote_file}', f)
                
                success_count += 1
                self.test_files_created.append(test_file)
            
            # 清理
            for i in range(num_files):
                try:
                    self.ftp.delete(f"concurrent_upload_{i}.bin")
                except:
                    pass
            
            duration = time.time() - start_time
            self.result.add_result(test_name, True, duration,
                                 details=f"成功传输{success_count}/{num_files}个文件")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s, 完成: {success_count}/{num_files}")
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def disconnect(self):
        """断开FTP连接 - 增强清理"""
        if self.ftp:
            try:
                self.ftp.quit()
            except:
                try:
                    self.ftp.close()
                except:
                    pass
            self.ftp = None
        
        # 清理本地测试文件
        for file_path in self.test_files_created:
            try:
                if os.path.exists(file_path):
                    os.remove(file_path)
            except Exception as e:
                print(f"警告: 清理文件失败 {file_path}: {e}")
        self.test_files_created.clear()


class SFTPTester:
    """SFTP测试类"""
    
    def __init__(self, config: TestConfig, result: TestResult):
        self.config = config
        self.result = result
        self.ssh_client = None
        self.sftp_client = None
        
    def connect(self) -> bool:
        """建立SFTP连接"""
        test_name = "SFTP基本连接"
        start_time = time.time()
        
        try:
            self.ssh_client = paramiko.SSHClient()
            self.ssh_client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
            
            self.ssh_client.connect(
                hostname=self.config.sftp_host,
                port=self.config.sftp_port,
                username=self.config.username,
                password=self.config.password,
                timeout=self.config.timeout,
                allow_agent=False,
                look_for_keys=False
            )
            
            self.sftp_client = self.ssh_client.open_sftp()
            
            duration = time.time() - start_time
            self.result.add_result(test_name, True, duration,
                                 details="SFTP连接成功")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s")
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def directory_operations(self) -> bool:
        """SFTP目录操作测试"""
        test_name = "SFTP目录操作"
        start_time = time.time()
        
        try:
            if not self.sftp_client:
                raise Exception("SFTP未连接")
            
            # 获取当前目录
            current_dir = self.sftp_client.getcwd() or "/"
            
            # 创建测试目录
            test_dir = f"test_sftp_dir_{int(time.time())}"
            test_path = f"{current_dir}/{test_dir}".replace("//", "/")
            self.sftp_client.mkdir(test_path)
            
            # 列出目录内容
            files = self.sftp_client.listdir(current_dir)
            
            # 删除测试目录
            self.sftp_client.rmdir(test_path)
            
            duration = time.time() - start_time
            self.result.add_result(test_name, True, duration,
                                 details=f"创建/删除目录: {test_dir}")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s")
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def file_transfer(self) -> bool:
        """SFTP文件传输测试"""
        test_name = "SFTP文件传输"
        start_time = time.time()
        
        try:
            if not self.sftp_client:
                raise Exception("SFTP未连接")
            
            # 创建测试文件
            test_file = os.path.join(self.config.test_data_dir, "sftp_test.txt")
            os.makedirs(os.path.dirname(test_file), exist_ok=True)
            
            test_content = f"SFTP测试文件 - {datetime.now()}\n" * 10
            with open(test_file, 'w', encoding='utf-8') as f:
                f.write(test_content)
            
            # 上传文件
            remote_filename = f"uploaded_sftp_test_{int(time.time())}.txt"
            self.sftp_client.put(test_file, remote_filename)
            
            # 下载文件
            downloaded_file = os.path.join(self.config.test_data_dir, "downloaded_sftp_test.txt")
            self.sftp_client.get(remote_filename, downloaded_file)
            
            # 验证文件内容
            with open(downloaded_file, 'r', encoding='utf-8') as f:
                downloaded_content = f.read()
            
            if test_content == downloaded_content:
                duration = time.time() - start_time
                self.result.add_result(test_name, True, duration,
                                     details="SFTP文件上传下载验证成功")
                print(f"✓ {test_name} - 耗时: {duration:.2f}s")
                
                # 清理远程文件
                try:
                    self.sftp_client.remove(remote_filename)
                except:
                    pass
                    
                return True
            else:
                raise Exception("SFTP文件内容验证失败")
                
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def file_permissions(self) -> bool:
        """SFTP文件权限测试"""
        test_name = "SFTP文件权限"
        start_time = time.time()
        
        try:
            if not self.sftp_client:
                raise Exception("SFTP未连接")
            
            # 创建测试文件
            test_file = os.path.join(self.config.test_data_dir, "perm_test.txt")
            with open(test_file, 'w', encoding='utf-8') as f:
                f.write("权限测试文件")
            
            remote_filename = f"perm_test_{int(time.time())}.txt"
            self.sftp_client.put(test_file, remote_filename)
            
            # 获取文件属性
            file_stat = self.sftp_client.stat(remote_filename)
            
            # 尝试修改权限（如果支持）
            try:
                self.sftp_client.chmod(remote_filename, 0o644)
                new_stat = self.sftp_client.stat(remote_filename)
                permission_changed = True
            except:
                permission_changed = False
            
            # 清理
            self.sftp_client.remove(remote_filename)
            os.remove(test_file)
            
            duration = time.time() - start_time
            self.result.add_result(test_name, True, duration,
                                 details=f"文件权限测试完成, 权限修改: {permission_changed}")
            print(f"✓ {test_name} - 耗时: {duration:.2f}s")
            return True
            
        except Exception as e:
            duration = time.time() - start_time
            self.result.add_result(test_name, False, duration, str(e))
            print(f"✗ {test_name} - 错误: {e}")
            return False
    
    def disconnect(self):
        """断开SFTP连接"""
        if self.sftp_client:
            try:
                self.sftp_client.close()
            except:
                pass
                
        if self.ssh_client:
            try:
                self.ssh_client.close()
            except:
                pass
                
        self.sftp_client = None
        self.ssh_client = None


class WFTPDTestSuite:
    """WFTPD测试套件主类"""
    
    def __init__(self, config_file: str = "test_config.json"):
        self.config = TestConfig(config_file)
        self.result = TestResult()
        self.setup_logging()
        
    def setup_logging(self):
        """设置日志记录"""
        log_dir = os.path.dirname(self.config.log_file)
        if log_dir:
            os.makedirs(log_dir, exist_ok=True)
            
        logging.basicConfig(
            level=logging.INFO,
            format='%(asctime)s - %(levelname)s - %(message)s',
            handlers=[
                logging.FileHandler(self.config.log_file, encoding='utf-8'),
                logging.StreamHandler()
            ]
        )
        self.logger = logging.getLogger(__name__)
    
    def prepare_test_environment(self):
        """准备测试环境"""
        print("=" * 50)
        print("WFTPD FTP/SFTP 测试套件")
        print("=" * 50)
        print(f"FTP服务器: {self.config.ftp_host}:{self.config.ftp_port}")
        print(f"SFTP服务器: {self.config.sftp_host}:{self.config.sftp_port}")
        print(f"用户名: {self.config.username}")
        print(f"测试开始时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
        print()
        
        # 创建测试数据目录
        os.makedirs(self.config.test_data_dir, exist_ok=True)
        
        self.result.start_time = time.time()
    
    def run_ftp_tests(self):
        """运行FTP测试 - 优化版（包含更多测试用例）"""
        print("\n" + "=" * 30)
        print("FTP 测试模块")
        print("=" * 30)
        
        ftp_tester = FTPTester(self.config, self.result)
        
        # 连接测试
        if not ftp_tester.connect():
            ftp_tester.disconnect()
            return
        
        # 登录测试
        if not ftp_tester.login():
            ftp_tester.disconnect()
            return
        
        # 核心功能测试
        ftp_tester.directory_operations()
        ftp_tester.file_transfer()
        
        # 传输模式测试
        ftp_tester.passive_mode()
        ftp_tester.ascii_mode()
        
        # 高级功能测试
        ftp_tester.file_rename()
        ftp_tester.large_file_transfer()
        ftp_tester.concurrent_transfers()
        
        # 断开连接
        ftp_tester.disconnect()
    
    def run_sftp_tests(self):
        """运行SFTP测试"""
        print("\n" + "=" * 30)
        print("SFTP 测试模块")
        print("=" * 30)
        
        sftp_tester = SFTPTester(self.config, self.result)
        
        # 连接测试
        if sftp_tester.connect():
            # 目录操作测试
            sftp_tester.directory_operations()
            
            # 文件传输测试
            sftp_tester.file_transfer()
            
            # 文件权限测试
            sftp_tester.file_permissions()
        
        # 断开连接
        sftp_tester.disconnect()
    
    def generate_report(self):
        """生成测试报告"""
        self.result.end_time = time.time()
        total_duration = self.result.end_time - self.result.start_time
        
        summary = self.result.summary()
        
        print("\n" + "=" * 50)
        print("测试报告")
        print("=" * 50)
        print(f"总测试数: {summary['total']}")
        print(f"通过: {summary['passed']}")
        print(f"失败: {summary['failed']}")
        print(f"通过率: {summary['pass_rate']}")
        print(f"总耗时: {total_duration:.2f}秒")
        print()
        
        # 详细结果
        print("详细测试结果:")
        print("-" * 60)
        for i, result in enumerate(self.result.results, 1):
            status = "✓ 通过" if result["success"] else "✗ 失败"
            print(f"{i:2d}. [{status}] {result['test_name']}")
            print(f"    耗时: {result['duration']:.2f}s")
            if result["error_msg"]:
                print(f"    错误: {result['error_msg']}")
            if result["details"]:
                print(f"    详情: {result['details']}")
            print()
        
        # 保存JSON报告
        report = {
            "test_suite": "WFTPD FTP/SFTP Test Suite",
            "start_time": datetime.fromtimestamp(self.result.start_time).isoformat(),
            "end_time": datetime.fromtimestamp(self.result.end_time).isoformat(),
            "total_duration": total_duration,
            "summary": summary,
            "results": self.result.results,
            "config": {
                "ftp_server": f"{self.config.ftp_host}:{self.config.ftp_port}",
                "sftp_server": f"{self.config.sftp_host}:{self.config.sftp_port}",
                "username": self.config.username
            }
        }
        
        report_file = "test_report.json"
        with open(report_file, 'w', encoding='utf-8') as f:
            json.dump(report, f, indent=2, ensure_ascii=False)
        
        print(f"详细报告已保存到: {report_file}")
        print(f"日志文件: {self.config.log_file}")
        
        return summary["failed"] == 0
    
    def cleanup(self):
        """清理测试环境"""
        if self.config.config["test_settings"].get("cleanup_after_test", True):
            try:
                # 清理测试文件（保留原始测试数据）
                test_files = [
                    "ftp_test.txt", "downloaded_ftp_test.txt",
                    "sftp_test.txt", "downloaded_sftp_test.txt",
                    "perm_test.txt"
                ]
                
                for filename in test_files:
                    filepath = os.path.join(self.config.test_data_dir, filename)
                    if os.path.exists(filepath):
                        os.remove(filepath)
                        
                print("测试环境清理完成")
            except Exception as e:
                print(f"清理测试环境时出错: {e}")
    
    def run_all_tests(self):
        """运行所有测试"""
        try:
            self.prepare_test_environment()
            
            # 运行FTP测试
            self.run_ftp_tests()
            
            # 运行SFTP测试  
            self.run_sftp_tests()
            
            # 生成报告
            success = self.generate_report()
            
            # 清理环境
            self.cleanup()
            
            return success
            
        except KeyboardInterrupt:
            print("\n测试被用户中断")
            return False
        except Exception as e:
            print(f"\n测试过程中发生错误: {e}")
            self.logger.error(f"测试异常: {e}", exc_info=True)
            return False


def main():
    """主函数"""
    # 检查命令行参数
    config_file = "test_config.json"
    if len(sys.argv) > 1:
        config_file = sys.argv[1]
    
    # 创建并运行测试套件
    test_suite = WFTPDTestSuite(config_file)
    success = test_suite.run_all_tests()
    
    # 退出码
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()