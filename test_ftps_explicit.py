# -*- coding: utf-8 -*-
"""
WFTPG 显式 FTPS (Explicit SSL/TLS) 完整功能测试
测试用户：123 / 密码：123456
"""

import socket
import ssl
import time
import hashlib
import re
from datetime import datetime


# ==================== 配置区域 ====================
SERVER_HOST = '127.0.0.1'
EXPLICIT_FTPS_PORT = 2121      # 显式 FTPS 端口
USERNAME = '123'
PASSWORD = '123456'
TIMEOUT = 10
# =================================================


class ExplicitFTPSClient:
    """显式 FTPS 客户端实现"""
    
    def __init__(self, host, port, username, password, timeout=10):
        self.host = host
        self.port = port
        self.username = username
        self.password = password
        self.timeout = timeout
        self.sock = None
        self.ssl_sock = None
        self.file = None
        self.result = TestResult()
        self.unique_prefix = f"ftps_{datetime.now().strftime('%Y%m%d_%H%M%S')}_"
        self.test_dir = '/test_ftps_explicit_' + datetime.now().strftime('%Y%m%d_%H%M%S')
        self.temp_files = []
    
    def connect(self):
        """建立显式 FTPS 连接"""
        try:
            # 创建 SSL 上下文
            ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
            ctx.check_hostname = False
            ctx.verify_mode = ssl.CERT_NONE
            ctx.minimum_version = ssl.TLSVersion.TLSv1_2
            
            # 建立普通 TCP 连接
            print("  [1] 建立 TCP 连接...")
            self.sock = socket.create_connection(
                (self.host, self.port),
                timeout=self.timeout
            )
            print("     TCP 连接成功")
            
            # 先创建普通文件对象读取欢迎消息
            self.file = self.sock.makefile('r', encoding='utf-8', newline='\r\n')
            
            # 读取欢迎消息
            welcome = self._read_response()
            print(f"     {welcome.strip()}")
            
            # 发送 AUTH TLS 命令升级到 SSL
            print("  [2] 发送 AUTH TLS 命令...")
            self._send_command('AUTH TLS')
            response = self._read_response()
            print(f"     {response.strip()}")
            
            if '234' not in response:
                raise Exception(f"AUTH TLS 失败：{response}")
            
            # 升级到 SSL
            print("  [3] SSL 握手...")
            self.ssl_sock = ctx.wrap_socket(
                self.sock,
                server_hostname=self.host
            )
            print(f"     SSL 握手成功 - 协议版本：{self.ssl_sock.version()}")
            
            # 重新创建文件对象（SSL 模式）
            self.file = self.ssl_sock.makefile('r', encoding='utf-8', newline='\r\n')
            
            # 登录
            print("  [4] 用户登录...")
            self._send_command(f'USER {self.username}')
            response = self._read_response()
            print(f"     {response.strip()}")
            
            if '331' not in response:
                raise Exception(f"用户名验证失败：{response}")
            
            self._send_command(f'PASS {self.password}')
            response = self._read_response()
            print(f"     {response.strip()}")
            
            if '230' not in response:
                raise Exception(f"密码验证失败：{response}")
            
            # 设置 PBSZ 和 PROT（保护数据通道）
            print("  [5] 设置数据保护...")
            self._send_command('PBSZ 0')
            response = self._read_response()
            print(f"     PBSZ: {response.strip()}")
            
            self._send_command('PROT P')
            response = self._read_response()
            print(f"     PROT: {response.strip()}")
            
            self.data_protection = True
            
            print(f"\n  显式 FTPS 连接成功 - 协议版本：{self.ssl_sock.version()}")
            return True
            
        except Exception as e:
            print(f"  连接失败：{e}")
            import traceback
            traceback.print_exc()
            self.close()
            return False
    
    def _send_command(self, command):
        """发送 FTP 命令"""
        if self.ssl_sock:
            self.ssl_sock.sendall((command + '\r\n').encode('utf-8'))
        else:
            self.sock.sendall((command + '\r\n').encode('utf-8'))
    
    def _read_response(self):
        """读取服务器响应"""
        response = ''
        while True:
            line = self.file.readline()
            if not line:
                break
            response += line
            # 检查是否是最后一行
            if len(line) >= 4 and line[0].isdigit() and line[3] == ' ':
                break
        return response
    
    def pwd(self):
        """获取当前目录"""
        test_name = "PWD 获取路径"
        try:
            self._send_command('PWD')
            response = self._read_response()
            if '257' in response:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, f"响应异常：{response}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def cwd(self, path):
        """切换目录"""
        test_name = "CWD 切换目录"
        try:
            self._send_command(f'CWD {path}')
            response = self._read_response()
            if '250' in response:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, f"响应异常：{response}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def mkd(self, path):
        """创建目录"""
        test_name = "MKD 创建目录"
        try:
            self._send_command(f'MKD {path}')
            response = self._read_response()
            if '257' in response:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, f"响应异常：{response}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def rmd(self, path):
        """删除目录"""
        test_name = "RMD 删除目录"
        try:
            self._send_command(f'RMD {path}')
            response = self._read_response()
            if '250' in response:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, f"响应异常：{response}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def _get_pasv_socket(self):
        """获取被动模式数据连接"""
        self._send_command('PASV')
        pasv_response = self._read_response()
        
        match = re.search(r'\((.*?)\)', pasv_response)
        if match:
            parts = match.group(1).split(',')
            if len(parts) == 6:
                ip = '.'.join(parts[:4])
                port = int(parts[4]) * 256 + int(parts[5])
                
                # 建立普通数据连接（不需要 SSL 包装）
                # 因为 PROT P 只表示数据需要保护，但实际由底层 TLS 处理
                data_sock = socket.create_connection((ip, port), timeout=self.timeout)
                return data_sock
        return None
    
    def list(self):
        """列出目录"""
        test_name = "LIST 列出目录"
        try:
            data_sock = self._get_pasv_socket()
            if data_sock:
                self._send_command('LIST')
                response = self._read_response()
                
                if '150' in response or '226' in response:
                    # 读取数据
                    if hasattr(data_sock, 'recv'):
                        data = data_sock.recv(4096)
                    data_sock.close()
                    
                    # 读取最终响应
                    final_response = self._read_response()
                    if '226' in final_response:
                        self.result.add_pass(test_name)
                    else:
                        self.result.add_fail(test_name, f"数据传输失败：{final_response}")
                else:
                    data_sock.close()
                    self.result.add_fail(test_name, f"LIST 命令失败：{response}")
            else:
                self.result.add_fail(test_name, "无法解析 PASV 响应")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def stor(self, filename, content):
        """上传文件"""
        test_name = "STOR 上传文件"
        try:
            data_sock = self._get_pasv_socket()
            if data_sock:
                self._send_command(f'STOR {filename}')
                response = self._read_response()
                
                if '150' in response:
                    # 发送文件内容
                    if hasattr(data_sock, 'sendall'):
                        data_sock.sendall(content.encode('utf-8'))
                        if hasattr(data_sock, 'shutdown'):
                            data_sock.shutdown(socket.SHUT_WR)
                    data_sock.close()
                    
                    # 读取最终响应
                    final_response = self._read_response()
                    if '226' in final_response:
                        self.result.add_pass(test_name)
                    else:
                        self.result.add_fail(test_name, f"传输失败：{final_response}")
                else:
                    data_sock.close()
                    self.result.add_fail(test_name, f"STOR 命令失败：{response}")
            else:
                self.result.add_fail(test_name, "无法解析 PASV 响应")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def retr(self, filename):
        """下载文件"""
        test_name = "RETR 下载文件"
        try:
            data_sock = self._get_pasv_socket()
            if data_sock:
                self._send_command(f'RETR {filename}')
                response = self._read_response()
                
                if '150' in response:
                    # 接收文件内容
                    content = b''
                    while True:
                        chunk = data_sock.recv(4096)
                        if not chunk:
                            break
                        content += chunk
                    
                    data_sock.close()
                    
                    # 读取最终响应
                    final_response = self._read_response()
                    if '226' in final_response:
                        self.result.add_pass(test_name)
                        return content.decode('utf-8')
                    else:
                        self.result.add_fail(test_name, f"传输失败：{final_response}")
                else:
                    data_sock.close()
                    self.result.add_fail(test_name, f"RETR 命令失败：{response}")
            else:
                self.result.add_fail(test_name, "无法解析 PASV 响应")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
        return ''
    
    def dele(self, filename):
        """删除文件"""
        test_name = "DELE 删除文件"
        try:
            self._send_command(f'DELE {filename}')
            response = self._read_response()
            if '250' in response:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, f"响应异常：{response}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def noop(self):
        """保持连接"""
        test_name = "NOOP 保持连接"
        try:
            self._send_command('NOOP')
            response = self._read_response()
            if '200' in response:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, f"响应异常：{response}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def quit(self):
        """退出连接"""
        test_name = "QUIT 退出连接"
        try:
            self._send_command('QUIT')
            response = self._read_response()
            if '221' in response:
                self.result.add_pass(test_name)
            else:
                self.result.add_fail(test_name, f"响应异常：{response}")
        except Exception as e:
            self.result.add_fail(test_name, str(e))
    
    def close(self):
        """关闭连接"""
        if self.file:
            self.file.close()
        if self.ssl_sock:
            self.ssl_sock.close()
        if self.sock:
            self.sock.close()
    
    def run_all_tests(self):
        """运行所有测试"""
        print("\n" + "="*60)
        print("开始显式 FTPS 功能测试")
        print("="*60)
        
        self.result.start_time = datetime.now()
        
        if not self.connect():
            self.result.add_fail("FTPS 连接", "无法连接到 FTPS 服务器")
            self.result.end_time = datetime.now()
            return self.result
        
        try:
            # 基础测试
            self.pwd()
            self.mkd(self.test_dir)
            self.cwd(self.test_dir)
            
            # 文件操作测试
            test_content = f"FTPS 显式测试内容 - {datetime.now().isoformat()}"
            test_file = self.unique_prefix + 'test.txt'
            self.stor(test_file, test_content)
            downloaded = self.retr(test_file)
            
            # 验证 MD5
            md5_test_name = "MD5 校验"
            if downloaded == test_content:
                self.result.add_pass(md5_test_name)
            else:
                self.result.add_fail(md5_test_name, "内容不匹配")
            
            self.list()
            self.dele(test_file)
            self.rmd(self.test_dir)
            
            # 连接测试
            self.noop()
            self.quit()
            
        finally:
            self.close()
        
        self.result.end_time = datetime.now()
        return self.result


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


def main():
    """主函数"""
    print("="*60)
    print("WFTPG 显式 FTPS (Explicit SSL/TLS) 完整功能测试")
    print("测试用户：123 / 密码：123456")
    print("="*60)
    
    print("\n[INFO] 测试模式：EXPLICIT (端口 2121)\n")
    
    # 运行测试
    ftps_client = ExplicitFTPSClient(
        host=SERVER_HOST,
        port=EXPLICIT_FTPS_PORT,
        username=USERNAME,
        password=PASSWORD,
        timeout=TIMEOUT
    )
    
    result = ftps_client.run_all_tests()
    
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
    
    # 返回退出码
    if result.failed > 0:
        print(f"\n有 {result.failed} 项测试失败")
    else:
        print(f"\n所有 {result.total} 项测试全部通过！")


if __name__ == '__main__':
    main()
