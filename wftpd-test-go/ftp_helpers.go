package main

import (
	"bufio"
	"crypto/tls"
	"fmt"
	"net"
	"net/textproto"
	"strconv"
	"strings"
	"time"
)

// FtpConn FTP连接封装
type FtpConn struct {
	*textproto.Conn
	conn net.Conn
}

// Close 关闭FTP连接
func (fc *FtpConn) Close() error {
	if fc.conn != nil {
		return fc.conn.Close()
	}
	return nil
}

// SendAbort 发送ABOR中止命令
func (fc *FtpConn) SendAbort() error {
	_, err := fc.conn.Write([]byte{0xFF, 0xF4, 0xFF, 0xF2})
	if err != nil {
		return fmt.Errorf("发送 Telnet IP/Synch 信号失败: %w", err)
	}
	err = fc.PrintfLine("ABOR")
	if err != nil {
		return fmt.Errorf("发送 ABOR 命令失败: %w", err)
	}
	return nil
}

// connectAndLogin 连接并登录FTP服务器
func connectAndLogin() (*FtpConn, error) {
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	conn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort), timeout)
	if err != nil {
		return nil, fmt.Errorf("连接失败: %w", err)
	}
	
	c := textproto.NewConn(conn)
	
	_, _, err = c.ReadResponse(220)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("欢迎消息错误: %w", err)
	}
	
	err = c.PrintfLine("USER %s", config.Username)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("USER 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(331)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("认证失败: %w", err)
	}
	
	err = c.PrintfLine("PASS %s", config.Password)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("PASS 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(230)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("密码错误: %w", err)
	}
	
	return &FtpConn{Conn: c, conn: conn}, nil
}

// parsePasvResponse 解析PASV响应
func parsePasvResponse(msg string) (string, int, error) {
	start := strings.Index(msg, "(")
	end := strings.Index(msg, ")")
	if start == -1 || end == -1 {
		return "", 0, fmt.Errorf("无效的 PASV 响应格式")
	}
	
	parts := strings.Split(msg[start+1:end], ",")
	if len(parts) != 6 {
		return "", 0, fmt.Errorf("PASV 响应参数数量错误: 期望 6 个，实际 %d 个", len(parts))
	}
	
	h1, err := strconv.Atoi(strings.TrimSpace(parts[0]))
	if err != nil {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 无效的 IP 地址部分 h1: %s", parts[0])
	}
	h2, err := strconv.Atoi(strings.TrimSpace(parts[1]))
	if err != nil {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 无效的 IP 地址部分 h2: %s", parts[1])
	}
	h3, err := strconv.Atoi(strings.TrimSpace(parts[2]))
	if err != nil {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 无效的 IP 地址部分 h3: %s", parts[2])
	}
	h4, err := strconv.Atoi(strings.TrimSpace(parts[3]))
	if err != nil {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 无效的 IP 地址部分 h4: %s", parts[3])
	}
	p1, err := strconv.Atoi(strings.TrimSpace(parts[4]))
	if err != nil {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 无效的端口部分 p1: %s", parts[4])
	}
	p2, err := strconv.Atoi(strings.TrimSpace(parts[5]))
	if err != nil {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 无效的端口部分 p2: %s", parts[5])
	}
	
	if h1 < 0 || h1 > 255 || h2 < 0 || h2 > 255 || h3 < 0 || h3 > 255 || h4 < 0 || h4 > 255 {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: IP 地址超出范围")
	}
	if p1 < 0 || p1 > 255 || p2 < 0 || p2 > 255 {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 端口部分超出范围")
	}
	
	host := fmt.Sprintf("%d.%d.%d.%d", h1, h2, h3, h4)
	port := p1*256 + p2
	
	// 安全检查：验证 IP 地址是否合理
	ip := net.ParseIP(host)
	if ip == nil {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 无效的 IP 地址格式: %s", host)
	}
	
	// 警告：如果服务器返回的 IP 与连接的服务器不同
	if !ip.IsLoopback() && !ip.IsPrivate() {
		logger.Printf("  ⚠ 警告: PASV 返回公网 IP %s，可能存在安全风险\n", host)
	}
	
	// 验证端口范围
	if port < 1024 {
		logger.Printf("  ⚠ 警告: PASV 返回特权端口 %d\n", port)
	}
	if port > 65535 {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 端口超出有效范围: %d", port)
	}
	
	return host, port, nil
}

// testTLSConnection 测试TLS加密连接
func testTLSConnection() error {
	startTime := time.Now()
	logger.Printf("  [TLS] 测试 TLS 加密连接...\n")
	
	addr := net.JoinHostPort(config.FTPServer, strconv.Itoa(config.FTPPort))
	
	var conn net.Conn
	var err error
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	
	if config.ImplicitFTPS {
		// 注意：InsecureSkipVerify 仅用于测试环境，生产环境应验证证书
		tlsConfig := &tls.Config{
			InsecureSkipVerify: true, // 测试环境跳过证书验证
		}
		conn, err = tls.DialWithDialer(&net.Dialer{Timeout: timeout}, "tcp", addr, tlsConfig)
		if err != nil {
			return fmt.Errorf("TLS 连接失败: %w", err)
		}
	} else {
		conn, err = net.DialTimeout("tcp", addr, timeout)
		if err != nil {
			return fmt.Errorf("连接失败: %w", err)
		}
		
		reader := bufio.NewReader(conn)
		_, err = reader.ReadString('\n')
		if err != nil {
			return fmt.Errorf("读取欢迎消息失败: %w", err)
		}
		
		_, err = fmt.Fprintf(conn, "AUTH TLS\r\n")
		if err != nil {
			return fmt.Errorf("发送 AUTH 命令失败: %w", err)
		}
		
		response, err := reader.ReadString('\n')
		if err != nil {
			return fmt.Errorf("读取 AUTH 响应失败: %w", err)
		}
		
		if !strings.HasPrefix(response, "234") {
			return fmt.Errorf("AUTH 命令被拒绝: %s", strings.TrimSpace(response))
		}
		logger.Printf("  ✓ AUTH TLS 响应: %s", response)
		
		// 注意：InsecureSkipVerify 仅用于测试环境，生产环境应验证证书
		tlsConfig := &tls.Config{
			InsecureSkipVerify: true, // 测试环境跳过证书验证
		}
		tlsConn := tls.Client(conn, tlsConfig)
		if err := tlsConn.Handshake(); err != nil {
			return fmt.Errorf("TLS 握手失败: %w", err)
		}
		conn = tlsConn
	}
	defer conn.Close()
	
	logger.Printf("  ✓ TLS 连接建立成功\n")
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}
