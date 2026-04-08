package main

import (
	"bufio"
	"crypto/tls"
	"fmt"
	"io"
	"net"
	"net/textproto"
	"strconv"
	"strings"
	"time"
)

type FtpConn struct {
	*textproto.Conn
	conn net.Conn
}

func (fc *FtpConn) Close() error {
	if fc.conn != nil {
		return fc.conn.Close()
	}
	return nil
}

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

	nums := make([]int, 6)
	for i, p := range parts {
		n, err := strconv.Atoi(strings.TrimSpace(p))
		if err != nil {
			return "", 0, fmt.Errorf("解析 PASV 响应失败: 无效的部分 %d: %s", i, p)
		}
		if n < 0 || n > 255 {
			return "", 0, fmt.Errorf("解析 PASV 响应失败: 值超出范围: %d", n)
		}
		nums[i] = n
	}

	host := fmt.Sprintf("%d.%d.%d.%d", nums[0], nums[1], nums[2], nums[3])
	port := nums[4]*256 + nums[5]

	ip := net.ParseIP(host)
	if ip == nil {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 无效的 IP 地址格式: %s", host)
	}
	if !ip.IsLoopback() && !ip.IsPrivate() {
		logger.Printf("  ⚠ 警告: PASV 返回公网 IP %s，可能存在安全风险\n", host)
	}
	if port < 1024 {
		logger.Printf("  ⚠ 警告: PASV 返回特权端口 %d\n", port)
	}
	if port > 65535 {
		return "", 0, fmt.Errorf("解析 PASV 响应失败: 端口超出有效范围: %d", port)
	}

	return host, port, nil
}

func parseEpsvResponse(msg string) (int, error) {
	start := strings.Index(msg, "|||")
	end := strings.LastIndex(msg, "|")
	if start == -1 || end == -1 || end <= start+3 {
		return 0, fmt.Errorf("无效的 EPSV 响应格式: %s", msg)
	}
	portStr := msg[start+3 : end]
	port, err := strconv.Atoi(portStr)
	if err != nil {
		return 0, fmt.Errorf("解析 EPSV 端口失败: %w", err)
	}
	return port, nil
}

type DataTransfer struct {
	Conn net.Conn
	Fc   *FtpConn
}

func (dt *DataTransfer) Close() {
	if dt.Conn != nil {
		dt.Conn.Close()
	}
}

func pasvDataConnect(fc *FtpConn) (*DataTransfer, error) {
	err := fc.PrintfLine("PASV")
	if err != nil {
		return nil, fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err := fc.ReadResponse(227)
	if err != nil {
		return nil, fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}

	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return nil, err
	}

	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return nil, fmt.Errorf("连接数据端口失败: %w", err)
	}

	return &DataTransfer{Conn: dataConn, Fc: fc}, nil
}

func epsvDataConnect(fc *FtpConn) (*DataTransfer, error) {
	err := fc.PrintfLine("EPSV")
	if err != nil {
		return nil, fmt.Errorf("发送 EPSV 命令失败: %w", err)
	}
	code, msg, err := fc.ReadResponse(229)
	if err != nil {
		return nil, fmt.Errorf("EPSV 命令错误: %d %s", code, msg)
	}

	port, err := parseEpsvResponse(msg)
	if err != nil {
		return nil, err
	}

	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, port), timeout)
	if err != nil {
		return nil, fmt.Errorf("连接数据端口失败: %w", err)
	}

	return &DataTransfer{Conn: dataConn, Fc: fc}, nil
}

func (dt *DataTransfer) Upload(src io.Reader, filename string) error {
	err := dt.Fc.PrintfLine("STOR %s", filename)
	if err != nil {
		return fmt.Errorf("发送 STOR 命令失败: %w", err)
	}

	code, msg, err := dt.Fc.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = dt.Fc.ReadResponse(125)
		}
		if err != nil {
			return fmt.Errorf("STOR 准备响应错误: %d %s", code, msg)
		}
	}

	timeout := time.Duration(config.TimeoutSeconds)*time.Second + 5*time.Second
	done := make(chan error, 1)
	go func() {
		_, err := io.Copy(dt.Conn, src)
		done <- err
	}()

	select {
	case err := <-done:
		if err != nil {
			return fmt.Errorf("传输文件失败: %w", err)
		}
	case <-time.After(timeout):
		return fmt.Errorf("上传超时")
	}
	dt.Conn.Close()

	_, _, err = dt.Fc.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	return nil
}

func (dt *DataTransfer) Download(dst io.Writer, filename string) error {
	err := dt.Fc.PrintfLine("RETR %s", filename)
	if err != nil {
		return fmt.Errorf("发送 RETR 命令失败: %w", err)
	}

	code, msg, err := dt.Fc.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = dt.Fc.ReadResponse(125)
		}
		if err != nil {
			return fmt.Errorf("RETR 准备响应错误: %d %s", code, msg)
		}
	}

	timeout := time.Duration(config.TimeoutSeconds)*time.Second + 5*time.Second
	done := make(chan error, 1)
	go func() {
		_, copyErr := io.Copy(dst, dt.Conn)
		done <- copyErr
	}()

	select {
	case err := <-done:
		if err != nil {
			return fmt.Errorf("接收文件失败: %w", err)
		}
	case <-time.After(timeout):
		return fmt.Errorf("下载超时")
	}
	dt.Conn.Close()

	_, _, err = dt.Fc.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	return nil
}

func (dt *DataTransfer) ReadListing() (string, error) {
	var buf strings.Builder
	timeout := time.Duration(config.TimeoutSeconds)*time.Second + 5*time.Second
	done := make(chan error, 1)
	go func() {
		_, copyErr := io.Copy(&buf, dt.Conn)
		done <- copyErr
	}()

	select {
	case err := <-done:
		if err != nil {
			return "", fmt.Errorf("读取列表失败: %w", err)
		}
	case <-time.After(timeout):
		return "", fmt.Errorf("读取列表超时")
	}
	dt.Conn.Close()

	_, _, err := dt.Fc.ReadResponse(226)
	if err != nil {
		return "", fmt.Errorf("传输确认错误: %w", err)
	}
	return buf.String(), nil
}

func testTLSConnection() error {
	startTime := time.Now()
	logger.Printf("  [TLS] 测试 TLS 加密连接...\n")

	addr := net.JoinHostPort(config.FTPServer, strconv.Itoa(config.FTPPort))

	var conn net.Conn
	var err error

	timeout := time.Duration(config.TimeoutSeconds) * time.Second

	if config.ImplicitFTPS {
		tlsConfig := &tls.Config{
			InsecureSkipVerify: true,
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

		tlsConfig := &tls.Config{
			InsecureSkipVerify: true,
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
