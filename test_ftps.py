# -*- coding: utf-8 -*-
"""
WFTPG FTPS (FTP over SSL/TLS) 功能测试脚本
测试用户：123 / 密码：123456
"""

import os
import sys
import time
import tempfile
import hashlib
import socket
import random
import string
from pathlib import Path
from datetime import datetime
from ftplib import FTP, error_perm, error_temp
import ssl
import json


# ==================== 集中配置区域 ====================
# 服务器配置
SERVER_HOST = '127.0.0.1'      # 服务器 IP 地址
FTPS_PORT = 2121               # FTPS 端口（与 FTP 相同）
EXPLICIT_FTPS_PORT = 2121      # 显式 FTPS 端口
IMPLICIT_FTPS_PORT = 990       # 隐式 FTPS 端口

# 用户认证配置
TEST_USERNAME = '123'          # 测试用户名
TEST_PASSWORD = '123456'       # 测试密码

# SSL/TLS 配置
SSL_VERIFY_MODE = ssl.CERT_NONE  # 测试环境使用自签名证书，禁用验证
CONNECTION_TIMEOUT = 10         # 连接超时时间（秒）
PORT_WAIT_TIMEOUT = 30          # 等待端口就绪超时时间（秒）

# 日志与输出
ENABLE_VERBOSE_LOG = False      # 是否启用详细日志
OUTPUT_JSON_RESULT = True       # 是否输出 JSON 结果文件
# =================================================


def generate_unique_filename(prefix='', suffix='.txt'):
    """生成唯一的文件名（时间戳 + 随机字符串）"""
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    random_str = ''.join(random.choices(string.ascii_lowercase + string.digits, k=8))
    return f"{prefix}{timestamp}_{random_str}{suffix}"


def calculate_md5(file_path):
    """计算文件的 MD5 值"""
    hash_md5 = hashlib.md5()
    with open(file_path, 'rb') as f:
        for chunk in iter(lambda: f.read(4096), b''):
            hash_md5.update(chunk)
    return hash_md5.hexdigest()


def is_port_in_use(host, port):
    """检测端口是否被占用"""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(2)
        result = sock.connect_ex((host, port))
        return result == 0


def wait_for_port(host, port, timeout=30):
    """等待端口可用，返回 True 表示端口已开放，False 表示超时"""
    start_time = time.time()
    while time.time() - start_time < timeout:
        if is_port_in_use(host, port):
            return True
        time.sleep(0.5)
    return False


class TestResult:
    """测试结果统计"""
    def __init__(self):
        self.total = 0
        self.passed = 0
        self.failed = 0
        self.errors = []
        self.start_time = None
        self.end_time = None
    
    def add_pass(self, test_name):
        self.total += 1
        self.passed += 1
        print(f"  [PASS] {test_name}")
    
    def add_fail(self, test_name, reason):
        self.total += 1
        self.failed += 1
        self.errors.append({"test": test_name, "reason": reason})
        print(f"  [FAIL] {test_name}: {reason}")
    
    def get_summary(self):
        duration = (self.end_time - self.start_time) if self.end_time and self.start_time else None
        duration_seconds = duration.total_seconds() if duration else 0
        return {
            "total": self.total,
            "passed": self.passed,
            "failed": self.failed,
            "success_rate": f"{(self.passed / self.total * 100):.2f}%" if self.total > 0 else "N/A",
            "duration_seconds": round(duration_seconds, 2),
            "errors": self.errors
        }


class FTPSTester:
    """FTPS (Explicit SSL/TLS) 功能测试"""
    
    def __init__(self, host=SERVER_HOST, port=EXPLICIT_FTPS_PORT, 
                 username=TEST_USERNAME, password=TEST_PASSWORD,
                 mode='explicit'):
        self.host = host
        self.port = port
        self.username = username
        self.password = password
        self.ftp = None
        self.result = TestResult()
        self.test_dir = '/test_ftps_' + datetime.now().strftime('%Y%m%d_%H%M%S')
        self.temp_files = []
        self.unique_prefix = generate_unique_filename(prefix='ftps_', suffix='') + '_'
        self.ssl_context = None
        self.mode = mode  # 'explicit' or 'implicit'
    
    def setup_ssl_context(self):
        """配置 SSL 上下文"""
        self.ssl_context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        self.ssl_context.check_hostname = False
        self.ssl_context.verify_mode = SSL_VERIFY_MODE
        # 允许 TLS 1.2+
        self.ssl_context.minimum_version = ssl.TLSVersion.TLSv1_2
    
    def connect(self):
        """建立 FTPS 连接（显式或隐式 SSL）"""
        try:
            self.setup_ssl_context()
            
            if self.mode == 'implicit':
                return self.connect_implicit_mode()
            else:
                return self.connect_explicit_mode()
                
        except ssl.SSLError as e:
            print(f"FTPS SSL 错误：{e}")
            return False
        except Exception as e:
            print(f"FTPS 连接失败：{e}")
            return False
    
    def connect_explicit_mode(self):
        """显式 FTPS 连接"""
        try:
            # 使用 FTP_TLS 类进行显式 SSL 连接
            from ftplib import FTP_TLS
            
            # 创建 FTP_TLS 对象
            self.ftp = FTP_TLS(
                host=self.host,
                user=self.username,
                passwd=self.password,
                timeout=CONNECTION_TIMEOUT,
                context=self.ssl_context
            )
            
            # 设置被动模式
            self.ftp.set_pasv(True)
            
            print(f"FTPS (显式 SSL) 连接成功 - 协议版本：{self.ftp.sock.version()}")
            return True
            
        except Exception as e:
            print(f"显式 FTPS 连接失败：{e}")
            return False
    
    def connect_implicit_mode(self):
        """隐式 FTPS 连接 - 使用正确的 ftplib 方法"""
        try:
            from ftplib import FTP_TLS
            
            # 创建 FTP_TLS 对象（专门用于 TLS 的 FTP 类）
            ftp_tls = FTP_TLS()
            
            # 配置 SSL 上下文
            ftp_tls.ssl_context = self.ssl_context
            
            # 连接到服务器（隐式模式下，连接时立即进行 SSL 握手）
            ftp_tls.connect(self.host, self.port, timeout=CONNECTION_TIMEOUT)
            
            # 登录
            ftp_tls.login(self.username, self.password)
            
            # 设置被动模式
            ftp_tls.set_pasv(True)
            
            # 保存到 self.ftp
            self.ftp = ftp_tls
            
            # 获取 SSL 版本信息
            ssl_version = ftp_tls.sock.version()
            print(f"FTPS (隐式 SSL) 连接成功 - 协议版本：{ssl_version}")
            return True
            
        except Exception as e:
            print(f"隐式 FTPS 连接失败：{e}")
            import traceback
            traceback.print_exc()
            return False
    
    def connect_implicit(self):
        """建立 FTPS 连接（隐式 SSL）"""
        try:
            self.setup_ssl_context()
            
            # 隐式 SSL 模式需要直接通过 SSL 连接
            self.ftp = FTP()
            
            # 先建立普通连接
            self.ftp.connect(self.host, IMPLICIT_FTPS_PORT, timeout=CONNECTION_TIMEOUT)
            
            # 立即包装 SSL（隐式模式）
            self.ftp.sock = self.ssl_context.wrap_socket(
                self.ftp.sock,
                server_hostname=self.host
            )
            
            # 登录
            self.ftp.login(self.username, self.password)
            self.ftp.set_pasv(True)
            
            print(f"FTPS (隐式 SSL) 连接成功 - 协议版本：{self.ftp.sock.version()}")
            return True
            
        except ssl.SSLError as e:
            print(f"FTPS 隐式 SSL 错误：{e}")
            return False
        except Exception as e:
            print(f"FTPS 隐式连接失败：{e}")
            return False
    
    def disconnect(self):
        """断开 FTPS 连接"""
        if self.ftp:
            try:
                self.ftp.quit()
            except:
                pass
    
    def cleanup(self):
        """清理测试文件"""
        if not self.ftp:
            return
        
        try:
            # 删除测试目录
            try:
                self.ftp.rmd(self.test_dir)
            except:
                pass
            
            # 删除本地临时文件
            for f in self.temp_files:
                try:
                    if os.path.exists(f):
                        os.remove(f)
                except:
                    pass
        except:
            pass
    
    def test_ssl_handshake(self):
        """测试 SSL 握手"""
        test_name = "SSL 握手验证"
        try:
            if self.ftp and self.ftp.sock:
                cipher = self.ftp.sock.cipher()
                if cipher:
                    cipher_name, cipher_version, _ = cipher
                    print(f"    加密套件：{cipher_name} ({cipher_version})")
                    self.result.add_pass(test_name)
                else:
                    self.result.add_fail(test_name, "无法获取加密套件信息")
            else:
                self.result.add_fail(test_name, "SSL 连接未建立")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def test_login(self):
        """测试登录"""
        test_name = "登录验证"
        try:
            # 隐式模式已经在 connect 时登录了
            if self.mode == 'implicit':
                # 使用 PWD 命令验证登录状态
                pwd = self.ftp.pwd()
                if pwd or pwd == '/':
                    self.result.add_pass(test_name)
                else:
                    self.result.add_fail(test_name, f"PWD 返回异常：{pwd}")
            else:
                # 显式模式发送 NOOP 测试
                response = self.ftp.voidcmd('NOOP')
                if '200' in response:
                    self.result.add_pass(test_name)
                else:
                    self.result.add_fail(test_name, f"NOOP 响应异常：{response}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def test_pwd(self):
        """测试获取当前目录"""
        test_name = "PWD 命令"
        try:
            pwd = self.ftp.pwd()
            if pwd:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, "返回空路径")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def test_mkd(self):
        """测试创建目录"""
        test_name = "MKD 创建目录"
        try:
            self.ftp.mkd(self.test_dir)
            self.result.add_pass(test_name)
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def test_cwd(self):
        """测试切换目录"""
        test_name = "CWD 切换目录"
        try:
            self.ftp.cwd(self.test_dir)
            current = self.ftp.pwd()
            if self.test_dir in current:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, f"切换后路径不正确：{current}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def test_put_file(self):
        """测试上传文件（带 MD5 校验）"""
        test_name = "STOR 上传文件（MD5 校验）"
        try:
            unique_filename = self.unique_prefix + 'upload.txt'
            test_content = f"FTPS 测试文件内容 - {datetime.now().isoformat()}\n随机数据：{''.join(random.choices(string.ascii_letters + string.digits, k=100))}"
            local_file = os.path.join(tempfile.gettempdir(), unique_filename)
            with open(local_file, 'w', encoding='utf-8') as f:
                f.write(test_content)
            
            self.temp_files.append(local_file)
            local_md5 = calculate_md5(local_file)
            
            remote_file = unique_filename
            with open(local_file, 'rb') as f:
                self.ftp.storbinary(f'STOR {remote_file}', f)
            
            files = self.ftp.nlst()
            if remote_file not in files:
                self.result.add_fail(test_name, "文件上传后不存在")
                return
            
            download_file = os.path.join(tempfile.gettempdir(), unique_filename + '.download')
            self.temp_files.append(download_file)
            with open(download_file, 'wb') as f:
                self.ftp.retrbinary(f'RETR {remote_file}', f.write)
            
            download_md5 = calculate_md5(download_file)
            
            if local_md5 == download_md5:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, f"MD5 校验失败：本地={local_md5}, 远程={download_md5}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def test_get_file(self):
        """测试下载文件（带 MD5 校验）"""
        test_name = "RETR 下载文件（MD5 校验）"
        try:
            remote_file = self.unique_prefix + 'upload.txt'
            unique_filename = remote_file + '.download'
            local_file = os.path.join(tempfile.gettempdir(), unique_filename)
            self.temp_files.append(local_file)
            
            temp_file = os.path.join(tempfile.gettempdir(), unique_filename + '.tmp')
            self.temp_files.append(temp_file)
            with open(temp_file, 'wb') as f:
                self.ftp.retrbinary(f'RETR {remote_file}', f.write)
            
            remote_md5 = calculate_md5(temp_file)
            
            with open(local_file, 'wb') as f:
                self.ftp.retrbinary(f'RETR {remote_file}', f.write)
            
            if os.path.exists(local_file) and os.path.getsize(local_file) > 0:
                local_md5 = calculate_md5(local_file)
                if local_md5 == remote_md5:
                    self.result.add_pass(test_name)
                else:
                    self.result.add_fail(test_name, f"MD5 校验失败：本地={local_md5}, 远程={remote_md5}")
            else:
                self.result.add_fail(test_name, "下载的文件为空或不存在")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def test_data_encryption(self):
        """测试数据传输加密"""
        test_name = "数据传输加密"
        try:
            # 启用数据保护（PBSZ 和 PROT）
            pbzd_response = self.ftp.sendcmd('PBSZ 0')
            if '200' not in pbzd_response:
                self.result.add_fail(test_name, f"PBSZ 命令失败：{pbzd_response}")
                return
            
            prot_response = self.ftp.sendcmd('PROT P')
            if '200' not in prot_response:
                self.result.add_fail(test_name, f"PROT 命令失败：{prot_response}")
                return
            
            self.result.add_pass(test_name)
        except Exception as e:
            self.result.add_fail(test_name, f"数据加密设置失败：{e}")
    
    def test_list(self):
        """测试列出目录"""
        test_name = "LIST 列出目录"
        try:
            result = []
            self.ftp.retrlines('LIST', result.append)
            if len(result) > 0:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, "目录列表为空")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def test_delete(self):
        """测试删除文件"""
        test_name = "DELE 删除文件"
        try:
            remote_file = self.unique_prefix + 'upload.txt'
            files = self.ftp.nlst()
            
            if remote_file not in files:
                test_content = f'Delete test content - {datetime.now().isoformat()}'
                local_file = os.path.join(tempfile.gettempdir(), remote_file)
                with open(local_file, 'w', encoding='utf-8') as f:
                    f.write(test_content)
                self.temp_files.append(local_file)
                
                with open(local_file, 'rb') as f:
                    self.ftp.storbinary(f'STOR {remote_file}', f)
            
            self.ftp.delete(remote_file)
            
            files = self.ftp.nlst()
            if remote_file not in files:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, "文件删除后仍然存在")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def test_rmd(self):
        """测试删除目录"""
        test_name = "RMD 删除目录"
        try:
            self.ftp.cwd('/')
            self.ftp.rmd(self.test_dir)
            self.result.add_pass(test_name)
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def test_auth_tls(self):
        """测试 AUTH TLS 命令（仅显式模式）"""
        if self.mode == 'implicit':
            # 隐式模式跳过此测试
            print(f"    [SKIP] 隐式模式无需 AUTH TLS")
            return
        
        test_name = "AUTH TLS 协商"
        try:
            response = self.ftp.sendcmd('AUTH TLS')
            if '234' in response or 'already using TLS' in response:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, f"AUTH TLS 响应异常：{response}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def run_all_tests(self):
        """运行所有 FTPS 测试"""
        print("\n" + "="*60)
        print("开始 FTPS (显式 SSL/TLS) 功能测试")
        print("="*60)
        
        self.result.start_time = datetime.now()
        
        if not self.connect():
            self.result.add_fail("FTPS 连接", "无法连接到 FTPS 服务器")
            self.result.end_time = datetime.now()
            return self.result
        
        try:
            # SSL 相关测试
            self.test_ssl_handshake()
            self.test_auth_tls()
            
            # 基础命令测试
            self.test_login()
            self.test_pwd()
            
            # 目录操作测试
            self.test_mkd()
            self.test_cwd()
            
            # 文件操作测试
            self.test_put_file()
            self.test_list()
            self.test_data_encryption()
            self.test_get_file()
            self.test_delete()
            
            # 清理测试
            self.test_rmd()
            
        finally:
            self.disconnect()
            self.cleanup()
        
        self.result.end_time = datetime.now()
        return self.result


def check_prerequisites():
    """检查前置条件"""
    print("检查 FTPS 测试环境...")
    
    # 检查 Python 依赖
    try:
        import ssl
        from ftplib import FTP
    except ImportError as e:
        print(f"错误：缺少必要的 Python 模块：{e}")
        return False
    
    print("  [PASS] Python 依赖检查通过")
    
    # 检查端口
    if not wait_for_port(SERVER_HOST, FTPS_PORT, timeout=PORT_WAIT_TIMEOUT):
        print(f"[错误] FTPS 端口 {FTPS_PORT} 未开放")
        print("\n请确保:")
        print("1. WFTPD 服务已启动")
        print("2. FTPS 功能已启用（在 GUI 中配置 SSL 证书）")
        print("3. 防火墙允许该端口访问")
        return False
    
    print(f"  [PASS] FTPS 端口 {FTPS_PORT} 已就绪")
    return True


def main():
    """主函数"""
    print("="*60)
    print("WFTPG FTPS (FTP over SSL/TLS) 功能测试")
    print("测试用户：123 / 密码：123456")
    print("="*60)
    
    # 检查前置条件
    if not check_prerequisites():
        print("\n❌ 测试环境检查失败，无法继续")
        sys.exit(1)
    
    print("\n✅ 测试环境检查通过\n")
    
    # 确定测试模式
    test_mode = 'implicit' if IMPLICIT_FTPS_PORT == 990 else 'explicit'
    test_port = IMPLICIT_FTPS_PORT if test_mode == 'implicit' else EXPLICIT_FTPS_PORT
    
    print(f"📝 当前测试模式：{test_mode.upper()} (端口 {test_port})\n")
    
    # 运行测试
    ftps_tester = FTPSTester(
        host=SERVER_HOST, 
        port=test_port,
        username=TEST_USERNAME, 
        password=TEST_PASSWORD,
        mode=test_mode
    )
    
    result = ftps_tester.run_all_tests()
    
    # 汇总结果
    print("\n" + "="*60)
    print("FTPS 测试结果汇总")
    print("="*60)
    
    print(f"\n总测试数：{result.total}")
    print(f"通过：{result.passed}")
    print(f"失败：{result.failed}")
    print(f"成功率：{result.get_summary()['success_rate']}")
    print(f"耗时：{result.get_summary()['duration_seconds']:.2f} 秒")
    
    if result.errors:
        print("\n--- 错误详情 ---")
        for error in result.errors:
            print(f"  [FAIL] {error['test']}: {error['reason']}")
    
    # 保存测试结果到 JSON 文件
    if OUTPUT_JSON_RESULT:
        test_result = {
            "start_time": result.start_time.isoformat() if result.start_time else None,
            "end_time": result.end_time.isoformat() if result.end_time else None,
            "ftps": result.get_summary()
        }
        
        result_file = os.path.join(os.path.dirname(__file__), 'test_ftps_result.json')
        with open(result_file, 'w', encoding='utf-8') as f:
            json.dump(test_result, f, ensure_ascii=False, indent=2)
        
        print(f"\n测试结果已保存到：{result_file}")
    
    # 返回退出码
    if result.failed > 0:
        print(f"\n❌ 有 {result.failed} 项测试失败")
        sys.exit(1)
    else:
        print(f"\n✅ 所有 {result.total} 项测试全部通过！")
        sys.exit(0)


if __name__ == '__main__':
    main()
