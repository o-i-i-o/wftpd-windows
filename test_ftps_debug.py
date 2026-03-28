# -*- coding: utf-8 -*-
"""
WFTPG FTPS 调试脚本 - 用于诊断命令处理问题
"""

import socket
import ssl


def debug_ftps():
    """调试 FTPS 连接和命令"""
    
    HOST = '127.0.0.1'
    PORT = 990
    USERNAME = '123'
    PASSWORD = '123456'
    
    # 创建 SSL 上下文
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    
    print("="*60)
    print("FTPS 调试模式")
    print("="*60)
    
    try:
        # 建立连接
        print("\n[1] 建立 TCP 连接到 {}:{}".format(HOST, PORT))
        sock = socket.create_connection((HOST, PORT), timeout=10)
        print("    ✅ TCP 连接成功")
        
        # SSL 握手
        print("\n[2] SSL 握手...")
        ssl_sock = ctx.wrap_socket(sock, server_hostname=HOST)
        print("    ✅ SSL 握手成功 - 协议版本：{}".format(ssl_sock.version()))
        
        # 创建文件对象
        print("\n[3] 创建文件对象...")
        file_obj = ssl_sock.makefile('rb')
        print("    ✅ 文件对象创建成功")
        
        # 读取欢迎消息
        print("\n[4] 读取欢迎消息...")
        welcome = file_obj.readline().decode('utf-8').strip()
        print("    服务器：{}".format(welcome))
        
        # 发送 USER 命令
        print("\n[5] 发送 USER 命令...")
        command = 'USER {}\r\n'.format(USERNAME)
        print("    发送：{}".format(repr(command)))
        ssl_sock.sendall(command.encode('utf-8'))
        response = file_obj.readline().decode('utf-8').strip()
        print("    响应：{}".format(response))
        
        # 发送 PASS 命令
        print("\n[6] 发送 PASS 命令...")
        command = 'PASS {}\r\n'.format(PASSWORD)
        print("    发送：{}".format(repr(command)))
        ssl_sock.sendall(command.encode('utf-8'))
        response = file_obj.readline().decode('utf-8').strip()
        print("    响应：{}".format(response))
        
        # 发送 PWD 命令
        print("\n[7] 发送 PWD 命令...")
        command = 'PWD\r\n'
        print("    发送：{}".format(repr(command)))
        ssl_sock.sendall(command.encode('utf-8'))
        response = file_obj.readline().decode('utf-8').strip()
        print("    响应：{}".format(response))
        
        # 发送 SYST 命令
        print("\n[8] 发送 SYST 命令...")
        command = 'SYST\r\n'
        print("    发送：{}".format(repr(command)))
        ssl_sock.sendall(command.encode('utf-8'))
        response = file_obj.readline().decode('utf-8').strip()
        print("    响应：{}".format(response))
        
        # 发送 NOOP 命令
        print("\n[9] 发送 NOOP 命令...")
        command = 'NOOP\r\n'
        print("    发送：{}".format(repr(command)))
        ssl_sock.sendall(command.encode('utf-8'))
        response = file_obj.readline().decode('utf-8').strip()
        print("    响应：{}".format(response))
        
        # 发送 PASV 命令
        print("\n[10] 发送 PASV 命令...")
        command = 'PASV\r\n'
        print("    发送：{}".format(repr(command)))
        ssl_sock.sendall(command.encode('utf-8'))
        response = file_obj.readline().decode('utf-8').strip()
        print("    响应：{}".format(response))
        
        # 发送 LIST 命令
        print("\n[11] 发送 LIST 命令...")
        command = 'LIST\r\n'
        print("    发送：{}".format(repr(command)))
        ssl_sock.sendall(command.encode('utf-8'))
        response = file_obj.readline().decode('utf-8').strip()
        print("    响应：{}".format(response))
        
        # 发送 QUIT 命令
        print("\n[12] 发送 QUIT 命令...")
        command = 'QUIT\r\n'
        print("    发送：{}".format(repr(command)))
        ssl_sock.sendall(command.encode('utf-8'))
        response = file_obj.readline().decode('utf-8').strip()
        print("    响应：{}".format(response))
        
        # 关闭连接
        file_obj.close()
        ssl_sock.close()
        
        print("\n" + "="*60)
        print("调试完成")
        print("="*60)
        
    except Exception as e:
        print("\n❌ 错误：{}".format(e))
        import traceback
        traceback.print_exc()


if __name__ == '__main__':
    debug_ftps()
