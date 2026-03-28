# -*- coding: utf-8 -*-
"""
WFTPG FTPS (Implicit SSL/TLS) 快速测试脚本
测试用户：123 / 密码：123456
"""

import socket
import ssl
import time


SERVER_HOST = '127.0.0.1'
IMPLICIT_FTPS_PORT = 990
USERNAME = '123'
PASSWORD = '123456'
TIMEOUT = 10


def send_command(sock, command):
    """发送 FTP 命令并读取响应"""
    sock.sendall((command + '\r\n').encode('utf-8'))
    response = b''
    
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            break
        response += chunk
        
        # 检查是否是单行响应（以数字开头）
        lines = response.decode('utf-8', errors='ignore').split('\r\n')
        if len(lines) > 1:
            last_line = lines[-2]  # 倒数第二行是完整的
            if last_line[0].isdigit() and (len(last_line) < 4 or last_line[3] == ' '):
                break
    
    return response.decode('utf-8', errors='ignore')


def test_ftps_implicit():
    """测试隐式 FTPS 连接"""
    print("="*60)
    print("WFTPG FTPS (隐式 SSL/TLS) 快速测试")
    print("="*60)
    
    tests_passed = 0
    tests_failed = 0
    
    try:
        # 创建 SSL 上下文
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
        ctx.minimum_version = ssl.TLSVersion.TLSv1_2
        
        print("\n[测试 1] SSL 握手...")
        sock = socket.create_connection((SERVER_HOST, IMPLICIT_FTPS_PORT), timeout=TIMEOUT)
        ssl_sock = ctx.wrap_socket(sock, server_hostname=SERVER_HOST)
        print(f"  ✅ SSL 连接成功 - 协议版本：{ssl_sock.version()}")
        print(f"  ✅ 加密套件：{ssl_sock.cipher()[0]}")
        tests_passed += 1
        
        print("\n[测试 2] 接收欢迎消息...")
        welcome = ssl_sock.recv(1024).decode('utf-8')
        print(f"  ✅ {welcome.strip()}")
        tests_passed += 1
        
        print("\n[测试 3] 用户登录...")
        response = send_command(ssl_sock, f'USER {USERNAME}')
        print(f"  响应：{response.strip()}")
        if '331' in response:
            print(f"  ✅ 用户名接受")
            tests_passed += 1
        else:
            print(f"  ❌ 用户名拒绝")
            tests_failed += 1
            return tests_passed, tests_failed
        
        print("\n[测试 4] 密码验证...")
        response = send_command(ssl_sock, f'PASS {PASSWORD}')
        print(f"  响应：{response.strip()}")
        if '230' in response:
            print(f"  ✅ 登录成功")
            tests_passed += 1
        else:
            print(f"  ❌ 登录失败")
            tests_failed += 1
            return tests_passed, tests_failed
        
        print("\n[测试 5] 获取当前路径 (PWD)...")
        response = send_command(ssl_sock, 'PWD')
        print(f"  响应：{response.strip()}")
        if '257' in response:
            print(f"  ✅ PWD 命令成功")
            tests_passed += 1
        else:
            print(f"  ❌ PWD 命令失败")
            tests_failed += 1
        
        print("\n[测试 6] 系统类型 (SYST)...")
        response = send_command(ssl_sock, 'SYST')
        print(f"  响应：{response.strip()}")
        if '215' in response:
            print(f"  ✅ SYST 命令成功")
            tests_passed += 1
        else:
            print(f"  ❌ SYST 命令失败")
            tests_failed += 1
        
        print("\n[测试 7] 被动模式 (PASV)...")
        response = send_command(ssl_sock, 'PASV')
        print(f"  响应：{response.strip()}")
        if '227' in response:
            print(f"  ✅ PASV 命令成功")
            tests_passed += 1
        else:
            print(f"  ❌ PASV 命令失败")
            tests_failed += 1
        
        print("\n[测试 8] 列出目录 (LIST)...")
        response = send_command(ssl_sock, 'LIST')
        print(f"  响应：{response.strip()}")
        if '150' in response or '226' in response:
            print(f"  ✅ LIST 命令成功")
            tests_passed += 1
        else:
            print(f"  ❌ LIST 命令失败")
            tests_failed += 1
        
        print("\n[测试 9] 保持连接 (NOOP)...")
        response = send_command(ssl_sock, 'NOOP')
        print(f"  响应：{response.strip()}")
        if '200' in response:
            print(f"  ✅ NOOP 命令成功")
            tests_passed += 1
        else:
            print(f"  ❌ NOOP 命令失败")
            tests_failed += 1
        
        print("\n[测试 10] 退出 (QUIT)...")
        response = send_command(ssl_sock, 'QUIT')
        print(f"  响应：{response.strip()}")
        if '221' in response:
            print(f"  ✅ QUIT 命令成功")
            tests_passed += 1
        else:
            print(f"  ❌ QUIT 命令失败")
            tests_failed += 1
        
        ssl_sock.close()
        
    except Exception as e:
        print(f"\n❌ 测试失败：{e}")
        import traceback
        traceback.print_exc()
        tests_failed += 1
    
    return tests_passed, tests_failed


if __name__ == '__main__':
    start_time = time.time()
    
    passed, failed = test_ftps_implicit()
    
    elapsed = time.time() - start_time
    
    print("\n" + "="*60)
    print("测试结果汇总")
    print("="*60)
    print(f"总测试数：{passed + failed}")
    print(f"通过：{passed}")
    print(f"失败：{failed}")
    print(f"成功率：{(passed / (passed + failed) * 100):.2f}%" if (passed + failed) > 0 else "N/A")
    print(f"耗时：{elapsed:.2f} 秒")
    print("="*60)
    
    if failed == 0:
        print("\n✅ 所有测试通过！")
        exit(0)
    else:
        print(f"\n❌ {failed} 项测试失败")
        exit(1)
