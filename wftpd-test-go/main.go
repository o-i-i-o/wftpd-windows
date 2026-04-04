package main

import (
	"bufio"
	"crypto/tls"
	"fmt"
	"io"
	"log"
	"net"
	"net/textproto"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// 测试配置
type TestConfig struct {
	FTPServer     string
	FTPPort       int
	SFTPServer    string
	SFTPPort      int
	Username      string
	Password      string
	TestDataDir   string
	UseTLS        bool
	ImplicitFTPS  bool
}

// 测试结果
type TestResult struct {
	Name      string
	Passed    bool
	Duration  time.Duration
	Error     error
	Responses []string
}

var config TestConfig
var testResults []TestResult

func main() {
	// 初始化配置
	config = TestConfig{
		FTPServer:     "127.0.0.1",
		FTPPort:       21,
		SFTPServer:    "127.0.0.1",
		SFTPPort:      22,
		Username:      "testuser",
		Password:      "testpass123",
		TestDataDir:   "./testdata",
		UseTLS:        true,
		ImplicitFTPS:  false,
	}

	fmt.Println("========================================")
	fmt.Println("WFTPD FTP/SFTP 测试套件")
	fmt.Println("========================================")
	fmt.Println()

	// 创建测试数据目录
	if err := os.MkdirAll(config.TestDataDir, 0755); err != nil {
		log.Fatalf("创建测试目录失败：%v", err)
	}

	// 生成测试文件
	generateTestFiles()

	// 运行测试
	runFTPTests()
	runSFTPTests()

	// 输出测试报告
	printReport()
}

func generateTestFiles() {
	fmt.Println("[准备] 生成测试文件...")
	
	// 小文件 (1KB)
	smallFile := filepath.Join(config.TestDataDir, "small.txt")
	if err := os.WriteFile(smallFile, []byte(strings.Repeat("A", 1024)), 0644); err != nil {
		log.Fatalf("创建小文件失败：%v", err)
	}
	fmt.Printf("  ✓ 创建小文件：%s (1KB)\n", smallFile)

	// 中文件 (1MB)
	mediumFile := filepath.Join(config.TestDataDir, "medium.bin")
	f, err := os.Create(mediumFile)
	if err != nil {
		log.Fatalf="创建中文件失败：%v", err)
	}
	bufWriter := bufio.NewWriter(f)
	for i := 0; i < 1024; i++ {
		bufWriter.Write(make([]byte, 1024))
	}
	bufWriter.Flush()
	f.Close()
	fmt.Printf("  ✓ 创建中文件：%s (1MB)\n", mediumFile)

	// 大文件 (10MB)
	largeFile := filepath.Join(config.TestDataDir, "large.bin")
	f, err = os.Create(largeFile)
	if err != nil {
		log.Fatalf("创建大文件失败：%v", err)
	}
	bufWriter = bufio.NewWriter(f)
	for i := 0; i < 10*1024; i++ {
		bufWriter.Write(make([]byte, 1024))
	}
	bufWriter.Flush()
	f.Close()
	fmt.Printf("  ✓ 创建大文件：%s (10MB)\n", largeFile)

	fmt.Println()
}

func runFTPTests() {
	fmt.Println("========================================")
	fmt.Println("FTP 测试模块")
	fmt.Println("========================================")
	fmt.Println()

	// 测试 1: 基本连接
	testResult("FTP 基本连接", func() error {
		return testBasicConnection()
	})

	// 测试 2: 用户认证
	testResult("FTP 用户认证", func() error {
		return testAuthentication()
	})

	// 测试 3: 目录操作
	testResult("FTP 目录操作", func() error {
		return testDirectoryOperations()
	})

	// 测试 4: 文件上传
	testResult("FTP 文件上传 (小文件)", func() error {
		return testFileUpload("small.txt")
	})

	// 测试 5: 文件下载
	testResult("FTP 文件下载 (小文件)", func() error {
		return testFileDownload("small.txt")
	})

	// 测试 6: 中断续传
	testResult("FTP 断点续传", func() error {
		return testResumeTransfer()
	})

	// 测试 7: 被动模式
	testResult("FTP 被动模式", func() error {
		return testPassiveMode()
	})

	// 测试 8: TLS 加密 (如果启用)
	if config.UseTLS {
		testResult("FTPS TLS 加密连接", func() error {
			return testTLSConnection()
		})
	}

	fmt.Println()
}

func runSFTPTests() {
	fmt.Println("========================================")
	fmt.Println("SFTP 测试模块")
	fmt.Println("========================================")
	fmt.Println()

	// 测试 1: SFTP 基本连接
	testResult("SFTP 基本连接", func() error {
		return testSFTPConnection()
	})

	// 测试 2: SFTP 目录操作
	testResult("SFTP 目录操作", func() error {
		return testSFTPDirectoryOps()
	})

	// 测试 3: SFTP 文件操作
	testResult("SFTP 文件操作", func() error {
		return testSFTPFileOps()
	})

	// 测试 4: SFTP 重命名操作
	testResult("SFTP 重命名操作", func() error {
		return testSFTRenameOps()
	})

	fmt.Println()
}

func testBasicConnection() error {
	startTime := time.Now()
	fmt.Printf("  [连接] 正在连接到 %s:%d...\n", config.FTPServer, config.FTPPort)

	conn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort), 10*time.Second)
	if err != nil {
		return fmt.Errorf("连接失败：%w", err)
	}
	defer conn.Close()

	reader := bufio.NewReader(conn)
	line, err := reader.ReadString('\n')
	if err != nil {
		return fmt.Errorf("读取欢迎消息失败：%w", err)
	}

	if !strings.HasPrefix(line, "220") {
		return fmt.Errorf("意外的欢迎消息：%s", strings.TrimSpace(line))
	}

	fmt.Printf("  ✓ 连接成功，响应：%s", line)
	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testAuthentication() error {
	startTime := time.Now()
	fmt.Printf("  [认证] 正在认证用户 %s...\n", config.Username)

	conn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort), 10*time.Second)
	if err != nil {
		return fmt.Errorf("连接失败：%w", err)
	}
	defer conn.Close()

	c := textproto.NewConn(conn)
	
	// 读取欢迎消息
	code, msg, err := c.ReadResponse(220)
	if err != nil {
		return fmt.Errorf("欢迎消息错误：%w", err)
	}
	fmt.Printf("  ✓ 服务器响应：%d %s\n", code, strings.TrimSpace(msg))

	// 发送 USER 命令
	err = c.PrintfLine("USER %s", config.Username)
	if err != nil {
		return fmt.Errorf("发送 USER 命令失败：%w", err)
	}
	
	code, msg, err = c.ReadResponse(331)
	if err != nil {
		return fmt.Errorf("USER 命令错误：%d %s", code, msg)
	}
	fmt.Printf("  ✓ USER 响应：%d %s\n", code, strings.TrimSpace(msg))

	// 发送 PASS 命令
	err = c.PrintfLine("PASS %s", config.Password)
	if err != nil {
		return fmt.Errorf("发送 PASS 命令失败：%w", err)
	}
	
	code, msg, err = c.ReadResponse(230)
	if err != nil {
		return fmt.Errorf("PASS 命令错误：%d %s", code, msg)
	}
	fmt.Printf("  ✓ PASS 响应：%d %s\n", code, strings.TrimSpace(msg))

	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testDirectoryOperations() error {
	startTime := time.Now()
	fmt.Printf("  [目录] 测试目录操作...\n")

	conn, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer conn.Close()

	c := textproto.NewConn(conn)

	// PWD - 打印工作目录
	err = c.PrintfLine("PWD")
	if err != nil {
		return fmt.Errorf("发送 PWD 命令失败：%w", err)
	}
	code, msg, err := c.ReadResponse(257)
	if err != nil {
		return fmt.Errorf("PWD 命令错误：%d %s", code, msg)
	}
	fmt.Printf("  ✓ PWD: %s\n", strings.TrimSpace(msg))

	// MKD - 创建目录
	testDir := "test_go_dir"
	err = c.PrintfLine("MKD %s", testDir)
	if err != nil {
		return fmt.Errorf("发送 MKD 命令失败：%w", err)
	}
	code, msg, err = c.ReadResponse(257)
	if err != nil {
		return fmt.Errorf("MKD 命令错误：%d %s", code, msg)
	}
	fmt.Printf("  ✓ MKD: %s\n", strings.TrimSpace(msg))

	// CWD - 改变目录
	err = c.PrintfLine("CWD %s", testDir)
	if err != nil {
		return fmt.Errorf("发送 CWD 命令失败：%w", err)
	}
	code, msg, err = c.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("CWD 命令错误：%d %s", code, msg)
	}
	fmt.Printf("  ✓ CWD: %s\n", strings.TrimSpace(msg))

	// CDUP - 返回上级目录
	err = c.PrintfLine("CDUP")
	if err != nil {
		return fmt.Errorf("发送 CDUP 命令失败：%w", err)
	}
	code, msg, err = c.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("CDUP 命令错误：%d %s", code, msg)
	}
	fmt.Printf("  ✓ CDUP: %s\n", strings.TrimSpace(msg))

	// RMD - 删除目录
	err = c.PrintfLine("RMD %s", testDir)
	if err != nil {
		return fmt.Errorf("发送 RMD 命令失败：%w", err)
	}
	code, msg, err = c.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("RMD 命令错误：%d %s", code, msg)
	}
	fmt.Printf("  ✓ RMD: %s\n", strings.TrimSpace(msg))

	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testFileUpload(filename string) error {
	startTime := time.Now()
	srcPath := filepath.Join(config.TestDataDir, filename)
	fmt.Printf("  [上传] 上传文件 %s...\n", filename)

	conn, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer conn.Close()

	c := textproto.NewConn(conn)

	// 获取文件大小
	fileInfo, err := os.Stat(srcPath)
	if err != nil {
		return fmt.Errorf("获取文件信息失败：%w", err)
	}

	// TYPE I - 二进制模式
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败：%w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误：%w", err)
	}

	// PASV - 被动模式
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败：%w", err)
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误：%d %s", code, msg)
	}
	
	// 解析 PASV 响应
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败：%w", err)
	}

	// 连接数据端口
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), 10*time.Second)
	if err != nil {
		return fmt.Errorf("连接数据端口失败：%w", err)
	}
	defer dataConn.Close()

	// STOR - 上传文件
	err = c.PrintfLine("STOR %s", filename+"_uploaded")
	if err != nil {
		return fmt.Errorf("发送 STOR 命令失败：%w", err)
	}

	// 读取文件并上传
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败：%w", err)
	}
	defer file.Close()

	_, err = io.Copy(dataConn, file)
	if err != nil {
		return fmt.Errorf("传输文件失败：%w", err)
	}
	dataConn.Close()

	// 等待传输完成确认
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误：%w", err)
	}

	fmt.Printf("  ✓ 上传成功：%s (%.2f KB)\n", filename, float64(fileInfo.Size())/1024.0)
	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testFileDownload(filename string) error {
	startTime := time.Now()
	dstPath := filepath.Join(config.TestDataDir, filename+"_downloaded")
	fmt.Printf("  [下载] 下载文件 %s...\n", filename)

	conn, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer conn.Close()

	c := textproto.NewConn(conn)

	// TYPE I - 二进制模式
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败：%w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误：%w", err)
	}

	// PASV - 被动模式
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败：%w", err)
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误：%d %s", code, msg)
	}
	
	// 解析 PASV 响应
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败：%w", err)
	}

	// 连接数据端口
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), 10*time.Second)
	if err != nil {
		return fmt.Errorf("连接数据端口失败：%w", err)
	}
	defer dataConn.Close()

	// RETR - 下载文件
	err = c.PrintfLine("RETR %s", filename)
	if err != nil {
		return fmt.Errorf("发送 RETR 命令失败：%w", err)
	}

	// 接收数据
	outFile, err := os.Create(dstPath)
	if err != nil {
		return fmt.Errorf("创建输出文件失败：%w", err)
	}
	defer outFile.Close()

	_, err = io.Copy(outFile, dataConn)
	if err != nil {
		return fmt.Errorf("接收文件失败：%w", err)
	}
	dataConn.Close()

	// 等待传输完成确认
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误：%w", err)
	}

	fileInfo, _ := os.Stat(dstPath)
	fmt.Printf("  ✓ 下载成功：%s (%.2f KB)\n", filename, float64(fileInfo.Size())/1024.0)
	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testResumeTransfer() error {
	startTime := time.Now()
	fmt.Printf("  [续传] 测试断点续传...\n")

	conn, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer conn.Close()

	c := textproto.NewConn(conn)

	// TYPE I - 二进制模式
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败：%w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误：%w", err)
	}

	// SIZE - 获取远程文件大小
	err = c.PrintfLine("SIZE %s", "small.txt")
	if err != nil {
		return fmt.Errorf("发送 SIZE 命令失败：%w", err)
	}
	code, msg, err := c.ReadResponse(213)
	if err != nil {
		return fmt.Errorf("SIZE 命令错误：%d %s", code, msg)
	}
	
	size, err := strconv.ParseInt(strings.TrimSpace(msg), 10, 64)
	if err != nil {
		return fmt.Errorf("解析文件大小失败：%w", err)
	}
	fmt.Printf("  ✓ 远程文件大小：%d bytes\n", size)

	// REST - 设置重新开始位置
	if size > 0 {
		err = c.PrintfLine("REST %d", size/2)
		if err != nil {
			return fmt.Errorf("发送 REST 命令失败：%w", err)
		}
		code, msg, err = c.ReadResponse(350)
		if err != nil {
			return fmt.Errorf("REST 命令错误：%d %s", code, msg)
		}
		fmt.Printf("  ✓ REST: %s\n", strings.TrimSpace(msg))
	}

	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testPassiveMode() error {
	startTime := time.Now()
	fmt.Printf("  [模式] 测试被动模式...\n")

	conn, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer conn.Close()

	c := textproto.NewConn(conn)

	// EPSV - 扩展被动模式
	err = c.PrintfLine("EPSV")
	if err != nil {
		return fmt.Errorf("发送 EPSV 命令失败：%w", err)
	}
	code, msg, err := c.ReadResponse(229)
	if err != nil {
		return fmt.Errorf("EPSV 命令错误：%d %s", code, msg)
	}
	fmt.Printf("  ✓ EPSV: %s\n", strings.TrimSpace(msg))

	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testTLSConnection() error {
	startTime := time.Now()
	fmt.Printf("  [TLS] 测试 TLS 加密连接...\n")

	addr := fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort)
	
	var conn net.Conn
	var err error
	
	if config.ImplicitFTPS {
		// 隐式 FTPS - 直接 TLS 握手
		tlsConfig := &tls.Config{
			InsecureSkipVerify: true,
		}
		conn, err = tls.DialWithDialer(&net.Dialer{Timeout: 10*time.Second}, "tcp", addr, tlsConfig)
		if err != nil {
			return fmt.Errorf("TLS 连接失败：%w", err)
		}
	} else {
		// 显式 FTPS - 先普通连接再升级
		conn, err = net.DialTimeout("tcp", addr, 10*time.Second)
		if err != nil {
			return fmt.Errorf("连接失败：%w", err)
		}

		reader := bufio.NewReader(conn)
		_, err = reader.ReadString('\n')
		if err != nil {
			return fmt.Errorf("读取欢迎消息失败：%w", err)
		}

		// AUTH TLS - 请求 TLS 加密
		_, err = fmt.Fprintf(conn, "AUTH TLS\r\n")
		if err != nil {
			return fmt.Errorf("发送 AUTH 命令失败：%w", err)
		}

		response, err := reader.ReadString('\n')
		if err != nil {
			return fmt.Errorf("读取 AUTH 响应失败：%w", err)
		}

		if !strings.HasPrefix(response, "234") {
			return fmt.Errorf("AUTH 命令被拒绝：%s", strings.TrimSpace(response))
		}
		fmt.Printf("  ✓ AUTH TLS 响应：%s", response)

		// 升级到 TLS
		tlsConfig := &tls.Config{
			InsecureSkipVerify: true,
		}
		tlsConn := tls.Client(conn, tlsConfig)
		if err := tlsConn.Handshake(); err != nil {
			return fmt.Errorf("TLS 握手失败：%w", err)
		}
		conn = tlsConn
	}
	defer conn.Close()

	fmt.Printf("  ✓ TLS 连接建立成功\n")
	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// 辅助函数：连接并登录
func connectAndLogin() (net.Conn, error) {
	conn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort), 10*time.Second)
	if err != nil {
		return nil, fmt.Errorf("连接失败：%w", err)
	}

	c := textproto.NewConn(conn)
	
	// 读取欢迎消息
	_, _, err = c.ReadResponse(220)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("欢迎消息错误：%w", err)
	}

	// USER
	err = c.PrintfLine("USER %s", config.Username)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("USER 命令失败：%w", err)
	}
	_, _, err = c.ReadResponse(331)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("认证失败：%w", err)
	}

	// PASS
	err = c.PrintfLine("PASS %s", config.Password)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("PASS 命令失败：%w", err)
	}
	_, _, err = c.ReadResponse(230)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("密码错误：%w", err)
	}

	return conn, nil
}

// 辅助函数：解析 PASV 响应
func parsePasvResponse(msg string) (string, int, error) {
	// 格式：(h1,h2,h3,h4,p1,p2)
	start := strings.Index(msg, "(")
	end := strings.Index(msg, ")")
	if start == -1 || end == -1 {
		return "", 0, fmt.Errorf("无效的 PASV 响应格式")
	}

	parts := strings.Split(msg[start+1:end], ",")
	if len(parts) != 6 {
		return "", 0, fmt.Errorf("PASV 响应参数数量错误")
	}

	h1, _ := strconv.Atoi(parts[0])
	h2, _ := strconv.Atoi(parts[1])
	h3, _ := strconv.Atoi(parts[2])
	h4, _ := strconv.Atoi(parts[3])
	p1, _ := strconv.Atoi(parts[4])
	p2, _ := strconv.Atoi(parts[5])

	host := fmt.Sprintf("%d.%d.%d.%d", h1, h2, h3, h4)
	port := p1*256 + p2

	return host, port, nil
}

// 测试执行包装器
func testResult(name string, testFunc func() error) {
	startTime := time.Now()
	result := TestResult{
		Name:     name,
		Passed:   true,
		Duration: time.Since(startTime),
	}

	err := testFunc()
	if err != nil {
		result.Passed = false
		result.Error = err
		fmt.Printf("  ✗ 失败：%v\n", err)
	}

	testResults = append(testResults, result)
	fmt.Println()
}

// 打印测试报告
func printReport() {
	fmt.Println("========================================")
	fmt.Println("测试报告")
	fmt.Println("========================================")
	
	passed := 0
	failed := 0

	for i, result := range testResults {
		status := "✓ 通过"
		if !result.Passed {
			status = "✗ 失败"
			failed++
		} else {
			passed++
		}

		fmt.Printf("%2d. [%s] %s\n", i+1, status, result.Name)
		if result.Error != nil {
			fmt.Printf("    错误：%v\n", result.Error)
		}
		fmt.Printf("    耗时：%.2f ms\n", float64(result.Duration.Microseconds())/1000.0)
	}

	fmt.Println()
	fmt.Printf("总计：%d 项测试，%d 通过，%d 失败\n", passed+failed, passed, failed)
	fmt.Println("========================================")
}
