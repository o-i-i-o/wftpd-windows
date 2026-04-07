package main

import (
	"bufio"
	"context"
	"crypto/md5"
	"crypto/sha256"
	"crypto/tls"
	"encoding/hex"
	"encoding/json"
	"flag"
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

type TestConfig struct {
	FTPServer      string `json:"ftp_server"`
	FTPPort        int    `json:"ftp_port"`
	SFTPServer     string `json:"sftp_server"`
	SFTPPort       int    `json:"sftp_port"`
	Username       string `json:"username"`
	Password       string `json:"password"`
	TestDataDir    string `json:"test_data_dir"`
	LogFile        string `json:"log_file"`
	UseTLS         bool   `json:"use_tls"`
	ImplicitFTPS   bool   `json:"implicit_ftps"`
	TimeoutSeconds int    `json:"timeout_seconds"`
	MaxConcurrent  int    `json:"max_concurrent"`
}

type TestResult struct {
	Name      string
	Passed    bool
	Duration  time.Duration
	Error     error
	Responses []string
}

type Logger struct {
	file   *os.File
	console *log.Logger
	fileLog *log.Logger
}

var config TestConfig
var testResults []TestResult
var logger *Logger

func NewLogger(logPath string) (*Logger, error) {
	if err := os.MkdirAll(filepath.Dir(logPath), 0755); err != nil {
		logPath = "./test_result.log"
	}
	
	file, err := os.OpenFile(logPath, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0644)
	if err != nil {
		return nil, err
	}
	
	return &Logger{
		file:     file,
		console:  log.New(os.Stdout, "", 0),
		fileLog:  log.New(file, "", log.LstdFlags),
	}, nil
}

func (l *Logger) Close() {
	if l.file != nil {
		l.file.Close()
	}
}

func (l *Logger) Println(v ...interface{}) {
	l.console.Println(v...)
	l.fileLog.Println(v...)
}

func (l *Logger) Printf(format string, v ...interface{}) {
	l.console.Printf(format, v...)
	l.fileLog.Printf(format, v...)
}

func (l *Logger) Print(v ...interface{}) {
	l.console.Print(v...)
	l.fileLog.Print(v...)
}

func loadConfig(configPath string) (TestConfig, error) {
	cfg := TestConfig{
		FTPServer:      "127.0.0.1",
		FTPPort:        21,
		SFTPServer:     "127.0.0.1",
		SFTPPort:       2222,
		Username:       "123",
		Password:       "123123",
		TestDataDir:    "./testdata",
		LogFile:        "./test_result.log",
		UseTLS:         false,
		ImplicitFTPS:   false,
		TimeoutSeconds: 10,
		MaxConcurrent:  3,
	}
	
	file, err := os.Open(configPath)
	if err != nil {
		return cfg, nil
	}
	defer file.Close()
	
	decoder := json.NewDecoder(file)
	if err := decoder.Decode(&cfg); err != nil {
		return cfg, fmt.Errorf("解析配置文件失败: %w", err)
	}
	
	return cfg, nil
}

func main() {
	configPath := flag.String("config", "config.json", "配置文件路径")
	ftpServer := flag.String("ftp", "", "FTP 服务器地址 (覆盖配置文件)")
	ftpPort := flag.Int("ftp-port", 0, "FTP 端口 (覆盖配置文件)")
	sftpServer := flag.String("sftp", "", "SFTP 服务器地址 (覆盖配置文件)")
	sftpPort := flag.Int("sftp-port", 0, "SFTP 端口 (覆盖配置文件)")
	username := flag.String("user", "", "用户名 (覆盖配置文件)")
	password := flag.String("pass", "", "密码 (覆盖配置文件)")
	logFile := flag.String("log", "", "日志文件路径 (覆盖配置文件)")
	
	flag.Parse()
	
	var err error
	config, err = loadConfig(*configPath)
	if err != nil {
		log.Fatalf("加载配置失败: %v", err)
	}
	
	if *ftpServer != "" {
		config.FTPServer = *ftpServer
	}
	if *ftpPort != 0 {
		config.FTPPort = *ftpPort
	}
	if *sftpServer != "" {
		config.SFTPServer = *sftpServer
	}
	if *sftpPort != 0 {
		config.SFTPPort = *sftpPort
	}
	if *username != "" {
		config.Username = *username
	}
	if *password != "" {
		config.Password = *password
	}
	if *logFile != "" {
		config.LogFile = *logFile
	}
	
	logger, err = NewLogger(config.LogFile)
	if err != nil {
		log.Fatalf("创建日志文件失败: %v", err)
	}
	defer logger.Close()
	
	logger.Println("========================================")
	logger.Println("WFTPD FTP/SFTP 测试套件")
	logger.Println("========================================")
	logger.Println()
	logger.Printf("FTP 服务器: %s:%d\n", config.FTPServer, config.FTPPort)
	logger.Printf("SFTP 服务器: %s:%d\n", config.SFTPServer, config.SFTPPort)
	logger.Printf("用户名: %s\n", config.Username)
	logger.Printf("测试数据目录: %s\n", config.TestDataDir)
	logger.Printf("日志文件: %s\n", config.LogFile)
	logger.Println()
	
	if err := os.MkdirAll(config.TestDataDir, 0755); err != nil {
		logger.Printf("创建测试目录失败: %v\n", err)
		return
	}
	
	generateTestFiles()
	
	runFTPTests()
	runSFTPTests()
	
	printReport()
}

func generateTestFiles() {
	logger.Println("[准备] 生成测试文件...")
	
	smallFile := filepath.Join(config.TestDataDir, "small.txt")
	if err := os.WriteFile(smallFile, []byte(strings.Repeat("A", 1024)), 0644); err != nil {
		logger.Printf("创建小文件失败: %v\n", err)
		return
	}
	logger.Printf("  ✓ 创建小文件: %s (1KB)\n", smallFile)
	
	mediumFile := filepath.Join(config.TestDataDir, "medium.bin")
	f, err := os.Create(mediumFile)
	if err != nil {
		logger.Printf("创建中文件失败: %v\n", err)
		return
	}
	bufWriter := bufio.NewWriter(f)
	for i := 0; i < 1024; i++ {
		bufWriter.Write(make([]byte, 1024))
	}
	bufWriter.Flush()
	f.Close()
	logger.Printf("  ✓ 创建中文件: %s (1MB)\n", mediumFile)
	
	largeFile := filepath.Join(config.TestDataDir, "large.bin")
	f, err = os.Create(largeFile)
	if err != nil {
		logger.Printf("创建大文件失败: %v\n", err)
		return
	}
	bufWriter = bufio.NewWriter(f)
	for i := 0; i < 10*1024; i++ {
		bufWriter.Write(make([]byte, 1024))
	}
	bufWriter.Flush()
	f.Close()
	logger.Printf("  ✓ 创建大文件: %s (10MB)\n", largeFile)
	
	logger.Println()
}

func runFTPTests() {
	logger.Println("========================================")
	logger.Println("FTP 测试模块")
	logger.Println("========================================")
	logger.Println()
	
	testResult("FTP 基本连接", func() error {
		return testBasicConnection()
	})
	
	testResult("FTP 用户认证", func() error {
		return testAuthentication()
	})
	
	testResult("FTP 目录操作", func() error {
		return testDirectoryOperations()
	})
	
	testResult("FTP 文件上传 (小文件)", func() error {
		return testFileUpload("small.txt")
	})
	
	testResult("FTP 文件下载 (小文件)", func() error {
		return testFileDownload("small.txt")
	})
	
	testResult("FTP 文件列表 (LIST/NLST)", func() error {
		return testFileList()
	})
	
	testResult("FTP 文件删除 (DELE)", func() error {
		return testFileDelete()
	})
	
	testResult("FTP 断点续传", func() error {
		return testResumeTransfer()
	})
	
	testResult("FTP 被动模式 (PASV/EPSV)", func() error {
		return testPassiveMode()
	})
	
	testResult("FTP 主动模式 (PORT)", func() error {
		return testActiveMode()
	})
	
	if config.UseTLS {
		testResult("FTPS TLS 加密连接", func() error {
			return testTLSConnection()
		})
	}
	
	testResult("FTP 功能查询 (FEAT/SYST)", func() error {
		return testFeatAndSyst()
	})
	
	testResult("FTP 文件重命名 (RNFR/RNTO)", func() error {
		return testFtpRename()
	})
	
	testResult("FTP 文件时间/列表 (MDTM/MLST)", func() error {
		return testMdtmAndMlst()
	})
	
	testResult("FTP UTF-8 文件名支持", func() error {
		return testUtf8Filename()
	})
	
	testResult("FTP 并发传输测试", func() error {
		return testConcurrentTransfer()
	})
	
	testResult("FTP 性能基准测试", func() error {
		return testPerformanceBenchmark()
	})
	
	testResult("FTP ABOR 中止传输", func() error {
		return testAbortTransfer()
	})
	
	testResult("FTP QUIT 优雅退出", func() error {
		return testQuitGracefully()
	})
	
	testResult("FTP ASCII 传输模式", func() error {
		return testAsciiMode()
	})
	
	testResult("FTP 长时间连接保活", func() error {
		return testLongConnectionKeepalive()
	})
	
	testResult("FTP 状态/帮助 (STAT/HELP)", func() error {
		return testStatAndHelp()
	})
	
	testResult("FTP 文件追加 (APPE)", func() error {
		return testAppendFile()
	})
	
	testResult("FTP 传输模式/结构 (MODE/STRU)", func() error {
		return testModeAndStru()
	})
	
	logger.Println()
}

func testBasicConnection() error {
	startTime := time.Now()
	logger.Printf("  [连接] 正在连接到 %s:%d...\n", config.FTPServer, config.FTPPort)
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	conn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort), timeout)
	if err != nil {
		return fmt.Errorf("连接失败: %w", err)
	}
	defer conn.Close()
	
	reader := bufio.NewReader(conn)
	line, err := reader.ReadString('\n')
	if err != nil {
		return fmt.Errorf("读取欢迎消息失败: %w", err)
	}
	
	if !strings.HasPrefix(line, "220") {
		return fmt.Errorf("意外的欢迎消息: %s", strings.TrimSpace(line))
	}
	
	logger.Printf("  ✓ 连接成功，响应: %s", line)
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testAuthentication() error {
	startTime := time.Now()
	logger.Printf("  [认证] 正在认证用户 %s...\n", config.Username)
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	conn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort), timeout)
	if err != nil {
		return fmt.Errorf("连接失败: %w", err)
	}
	defer conn.Close()
	
	c := textproto.NewConn(conn)
	
	_, _, err = c.ReadResponse(220)
	if err != nil {
		return fmt.Errorf("欢迎消息错误: %w", err)
	}
	
	err = c.PrintfLine("USER %s", config.Username)
	if err != nil {
		return fmt.Errorf("发送 USER 命令失败: %w", err)
	}
	
	code, msg, err := c.ReadResponse(331)
	if err != nil {
		return fmt.Errorf("USER 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ USER 响应: %d %s\n", code, strings.TrimSpace(msg))
	
	err = c.PrintfLine("PASS %s", config.Password)
	if err != nil {
		return fmt.Errorf("发送 PASS 命令失败: %w", err)
	}
	
	code, msg, err = c.ReadResponse(230)
	if err != nil {
		return fmt.Errorf("PASS 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ PASS 响应: %d %s\n", code, strings.TrimSpace(msg))
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testDirectoryOperations() error {
	startTime := time.Now()
	logger.Printf("  [目录] 测试目录操作...\n")
	
	fc, err := connectAndLogin()
	if err != nil {
	 return err
    }
    defer fc.Close()
    
    err = fc.PrintfLine("PWD")
	if err != nil {
		return fmt.Errorf("发送 PWD 命令失败: %w", err)
	}
	code, msg, err := fc.ReadResponse(257)
	if err != nil {
		return fmt.Errorf("PWD 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ PWD: %s\n", strings.TrimSpace(msg))
	
	testDir := "test_go_dir"
	err = fc.PrintfLine("MKD %s", testDir)
	if err != nil {
		return fmt.Errorf("发送 MKD 命令失败: %w", err)
	}
	code, msg, err = fc.ReadResponse(257)
	if err != nil {
		return fmt.Errorf("MKD 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ MKD: %s\n", strings.TrimSpace(msg))
	
	err = fc.PrintfLine("CWD %s", testDir)
	if err != nil {
		return fmt.Errorf("发送 CWD 命令失败: %w", err)
	}
	code, msg, err = fc.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("CWD 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ CWD: %s\n", strings.TrimSpace(msg))
	
	err = fc.PrintfLine("CDUP")
	if err != nil {
		return fmt.Errorf("发送 CDUP 命令失败: %w", err)
	}
	code, msg, err = fc.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("CDUP 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ CDUP: %s\n", strings.TrimSpace(msg))
	
	err = fc.PrintfLine("RMD %s", testDir)
	if err != nil {
		return fmt.Errorf("发送 RMD 命令失败: %w", err)
	}
	code, msg, err = fc.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("RMD 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ RMD: %s\n", strings.TrimSpace(msg))
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testFileUpload(filename string) error {
	startTime := time.Now()
	srcPath := filepath.Join(config.TestDataDir, filename)
	logger.Printf("  [上传] 上传文件 %s...\n", filename)
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	fileInfo, err := os.Stat(srcPath)
	if err != nil {
		return fmt.Errorf("获取文件信息失败: %w", err)
	}
	
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		logger.Printf("  [DEBUG] PASV 响应: code=%d, msg=%s, err=%v\n", code, msg, err)
		return fmt.Errorf("PASV 命令错误: %d %s (err: %v)", code, msg, err)
	}
	logger.Printf("  [DEBUG] PASV 成功: %d %s\n", code, msg)
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败: %w", err)
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("STOR %s", filename+"_uploaded")
	if err != nil {
		return fmt.Errorf("发送 STOR 命令失败: %w", err)
	}
	
	code, msg, err := c.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = c.ReadResponse(125)
		}
		if err != nil {
			return fmt.Errorf("STOR 准备响应错误: %d %s (err: %v)", code, msg, err)
		}
	}
	logger.Printf("  [DEBUG] STOR 准备响应: %d %s\n", code, strings.TrimSpace(msg))
	
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()
	
	_, err = io.Copy(dataConn, file)
	if err != nil {
		return fmt.Errorf("传输文件失败: %w", err)
	}
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	
	logger.Printf("  ✓ 上传成功: %s (%.2f KB)\n", filename, float64(fileInfo.Size())/1024.0)
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testFileDownload(filename string) error {
	startTime := time.Now()
	dstPath := filepath.Join(config.TestDataDir, filename+"_downloaded")
	srcPath := filepath.Join(config.TestDataDir, filename)
	logger.Printf("  [下载] 下载文件 %s...\n", filename)
	
	originalMD5, err := calculateMD5(srcPath)
	if err != nil {
		return fmt.Errorf("计算原始文件 MD5 失败: %w", err)
	}
	logger.Printf("  ✓ 原始文件 MD5: %s\n", originalMD5)
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败: %w", err)
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("RETR %s", filename)
	if err != nil {
		return fmt.Errorf("发送 RETR 命令失败: %w", err)
	}
	
	code, msg, err := c.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = c.ReadResponse(125)
		}
		if err != nil {
			return fmt.Errorf("RETR 准备响应错误: %d %s (err: %v)", code, msg, err)
		}
	}
	logger.Printf("  [DEBUG] RETR 准备响应: %d %s\n", code, strings.TrimSpace(msg))
	
	outFile, err := os.Create(dstPath)
	if err != nil {
		return fmt.Errorf("创建输出文件失败: %w", err)
	}
	defer outFile.Close()
	
	_, err = io.Copy(outFile, dataConn)
	if err != nil {
		return fmt.Errorf("接收文件失败: %w", err)
	}
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	
	fileInfo, _ := os.Stat(dstPath)
	logger.Printf("  ✓ 下载成功: %s (%.2f KB)\n", filename, float64(fileInfo.Size())/1024.0)
	
	downloadedMD5, err := calculateMD5(dstPath)
	if err != nil {
		return fmt.Errorf("计算下载文件 MD5 失败: %w", err)
	}
	logger.Printf("  ✓ 下载文件 MD5: %s\n", downloadedMD5)
	
	if originalMD5 != downloadedMD5 {
		return fmt.Errorf("数据完整性验证失败: MD5 不匹配")
	}
	logger.Printf("  ✓ 数据完整性验证通过\n")
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testResumeTransfer() error {
	startTime := time.Now()
	logger.Printf("  [续传] 测试断点续传...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}
	
	err = c.PrintfLine("SIZE %s", "small.txt")
	if err != nil {
		return fmt.Errorf("发送 SIZE 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(213)
	if err != nil {
		return fmt.Errorf("SIZE 命令错误: %d %s", code, msg)
	}
	
	size, err := strconv.ParseInt(strings.TrimSpace(msg), 10, 64)
	if err != nil {
		return fmt.Errorf("解析文件大小失败: %w", err)
	}
	logger.Printf("  ✓ 远程文件大小: %d bytes\n", size)
	
	if size > 0 {
		err = c.PrintfLine("REST %d", size/2)
		if err != nil {
			return fmt.Errorf("发送 REST 命令失败: %w", err)
		}
		code, msg, err = c.ReadResponse(350)
		if err != nil {
			return fmt.Errorf("REST 命令错误: %d %s", code, msg)
		}
		logger.Printf("  ✓ REST: %s\n", strings.TrimSpace(msg))
	}
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testPassiveMode() error {
	startTime := time.Now()
	logger.Printf("  [模式] 测试被动模式...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	err = c.PrintfLine("EPSV")
	if err != nil {
		return fmt.Errorf("发送 EPSV 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(229)
	if err != nil {
		return fmt.Errorf("EPSV 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ EPSV: %s\n", strings.TrimSpace(msg))
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testTLSConnection() error {
	startTime := time.Now()
	logger.Printf("  [TLS] 测试 TLS 加密连接...\n")
	
	addr := fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort)
	
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
	
	return host, port, nil
}

func testFileList() error {
	startTime := time.Now()
	logger.Printf("  [列表] 测试文件列表...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败: %w", err)
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("LIST")
	if err != nil {
		return fmt.Errorf("发送 LIST 命令失败: %w", err)
	}
	
	reader := bufio.NewReader(dataConn)
	var listData strings.Builder
	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			if err == io.EOF {
				break
			}
			return fmt.Errorf("读取列表数据失败: %w", err)
		}
		listData.WriteString(line)
	}
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	
	logger.Printf("  ✓ LIST 成功，获取 %d 字节\n", listData.Len())
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}
	
	host, port, err = parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败: %w", err)
	}
	
	dataConn, err = net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("NLST")
	if err != nil {
		return fmt.Errorf("发送 NLST 命令失败: %w", err)
	}
	
	reader = bufio.NewReader(dataConn)
	var nlstData strings.Builder
	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			if err == io.EOF {
				break
			}
			return fmt.Errorf("读取 NLST 数据失败: %w", err)
		}
		nlstData.WriteString(line)
	}
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	
	logger.Printf("  ✓ NLST 成功，获取 %d 字节\n", nlstData.Len())
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testFileDelete() error {
	startTime := time.Now()
	logger.Printf("  [删除] 测试文件删除...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	testFilename := "delete_test.txt"
	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败: %w", err)
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("STOR %s", testFilename)
	if err != nil {
		return fmt.Errorf("发送 STOR 命令失败: %w", err)
	}
	
	code, msg, err = c.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = c.ReadResponse(125)
		}
		if err != nil {
			return fmt.Errorf("STOR 准备响应错误: %d %s (err: %v)", code, msg, err)
		}
	}
	
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()
	
	_, err = io.Copy(dataConn, file)
	if err != nil {
		return fmt.Errorf("传输文件失败: %w", err)
	}
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	
	logger.Printf("  ✓ 上传测试文件: %s\n", testFilename)
	
	err = c.PrintfLine("DELE %s", testFilename)
	if err != nil {
		return fmt.Errorf("发送 DELE 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("DELE 命令错误: %d %s", code, msg)
	}
	
	logger.Printf("  ✓ 删除文件成功: %s\n", strings.TrimSpace(msg))
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testActiveMode() error {
	startTime := time.Now()
	logger.Printf("  [模式] 测试主动模式...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return fmt.Errorf("创建监听器失败: %w", err)
	}
	
	addr := listener.Addr().(*net.TCPAddr)
	ipParts := strings.Split(addr.IP.String(), ".")
	if len(ipParts) != 4 {
		listener.Close()
		return fmt.Errorf("无效的 IP 地址格式")
	}
	
	h1, _ := strconv.Atoi(ipParts[0])
	h2, _ := strconv.Atoi(ipParts[1])
	h3, _ := strconv.Atoi(ipParts[2])
	h4, _ := strconv.Atoi(ipParts[3])
	p1 := addr.Port / 256
	p2 := addr.Port % 256
	
	portCmd := fmt.Sprintf("PORT %d,%d,%d,%d,%d,%d", h1, h2, h3, h4, p1, p2)
	err = c.PrintfLine(portCmd)
	if err != nil {
		listener.Close()
		return fmt.Errorf("发送 PORT 命令失败: %w", err)
	}
	
	code, msg, err := c.ReadResponse(200)
	if err != nil {
		listener.Close()
		return fmt.Errorf("PORT 命令错误: %d %s", code, msg)
	}
	
	logger.Printf("  ✓ PORT 命令成功: %s\n", strings.TrimSpace(msg))
	
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	
	type acceptResult struct {
		conn net.Conn
		err  error
	}
	acceptChan := make(chan acceptResult, 1)
	
	go func() {
		conn, err := listener.Accept()
		acceptChan <- acceptResult{conn: conn, err: err}
	}()
	
	err = c.PrintfLine("LIST")
	if err != nil {
		listener.Close()
		return fmt.Errorf("发送 LIST 命令失败: %w", err)
	}
	
	select {
	case <-ctx.Done():
		listener.Close()
		return fmt.Errorf("等待数据连接超时")
	case result := <-acceptChan:
		listener.Close()
		if result.err != nil {
			return fmt.Errorf("接受数据连接失败: %w", result.err)
		}
		if result.conn != nil {
			result.conn.Close()
		}
		logger.Printf("  ✓ 主动模式数据连接建立成功\n")
	}
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		logger.Printf("  ⚠ 控制连接响应: %v\n", err)
	}
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

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
		logger.Printf("  ✗ 失败: %v\n", err)
	}
	
	testResults = append(testResults, result)
	logger.Println()
}

func calculateMD5(filePath string) (string, error) {
	file, err := os.Open(filePath)
	if err != nil {
		return "", err
	}
	defer file.Close()
	
	hash := md5.New()
	if _, err := io.Copy(hash, file); err != nil {
		return "", err
	}
	
	return hex.EncodeToString(hash.Sum(nil)), nil
}

func calculateSHA256(filePath string) (string, error) {
	file, err := os.Open(filePath)
	if err != nil {
		return "", err
	}
	defer file.Close()
	
	hash := sha256.New()
	if _, err := io.Copy(hash, file); err != nil {
		return "", err
	}
	
	return hex.EncodeToString(hash.Sum(nil)), nil
}

func testFeatAndSyst() error {
	startTime := time.Now()
	logger.Printf("  [功能] 测试 FEAT/SYST 命令...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	err = c.PrintfLine("SYST")
	if err != nil {
		return fmt.Errorf("发送 SYST 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(215)
	if err != nil {
		return fmt.Errorf("SYST 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ SYST: %s\n", strings.TrimSpace(msg))
	
	err = c.PrintfLine("FEAT")
	if err != nil {
		return fmt.Errorf("发送 FEAT 命令失败: %w", err)
	}
	
	code, msg, err = c.ReadResponse(211)
	if err != nil {
		return fmt.Errorf("FEAT 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ FEAT 响应:\n%s\n", msg)
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testFtpRename() error {
	startTime := time.Now()
	logger.Printf("  [重命名] 测试 RNFR/RNTO 重命名...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	testFilename := "rename_ftp_test.txt"
	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败: %w", err)
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("STOR %s", testFilename)
	if err != nil {
		return fmt.Errorf("发送 STOR 命令失败: %w", err)
	}
	
	code, msg, err = c.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = c.ReadResponse(125)
		}
		if err != nil {
			return fmt.Errorf("STOR 准备响应错误: %d %s (err: %v)", code, msg, err)
		}
	}
	
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()
	
	_, err = io.Copy(dataConn, file)
	if err != nil {
		return fmt.Errorf("传输文件失败: %w", err)
	}
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	
	logger.Printf("  ✓ 上传测试文件: %s\n", testFilename)
	
	newFilename := "renamed_ftp_test.txt"
	err = c.PrintfLine("RNFR %s", testFilename)
	if err != nil {
		return fmt.Errorf("发送 RNFR 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(350)
	if err != nil {
		return fmt.Errorf("RNFR 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ RNFR: %s\n", strings.TrimSpace(msg))
	
	err = c.PrintfLine("RNTO %s", newFilename)
	if err != nil {
		return fmt.Errorf("发送 RNTO 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("RNTO 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ RNTO: %s\n", strings.TrimSpace(msg))
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}
	
	host, port, err = parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败: %w", err)
	}
	
	dataConn, err = net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	
	err = c.PrintfLine("NLST")
	if err != nil {
		return fmt.Errorf("发送 NLST 命令失败: %w", err)
	}
	
	reader := bufio.NewReader(dataConn)
	var fileList strings.Builder
	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			if err == io.EOF {
				break
			}
			return fmt.Errorf("读取列表数据失败: %w", err)
		}
		fileList.WriteString(line)
	}
	dataConn.Close()
	if _, _, err := c.ReadResponse(226); err != nil {
		logger.Printf("  ⚠ NLST 传输确认响应异常: %v\n", err)
	}
	
	if !strings.Contains(fileList.String(), newFilename) {
		return fmt.Errorf("重命名验证失败: 未找到文件 %s", newFilename)
	}
	logger.Printf("  ✓ 重命名验证成功: %s -> %s\n", testFilename, newFilename)
	
	err = c.PrintfLine("DELE %s", newFilename)
	if err != nil {
		return fmt.Errorf("发送 DELE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("DELE 命令错误: %w", err)
	}
	logger.Printf("  ✓ 清理测试文件\n")
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testMdtmAndMlst() error {
	startTime := time.Now()
	logger.Printf("  [时间/列表] 测试 MDTM/MLST 命令...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	err = c.PrintfLine("MDTM small.txt")
	if err != nil {
		return fmt.Errorf("发送 MDTM 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(213)
	if err != nil {
		logger.Printf("  ⚠ MDTM 不支持或文件不存在: %d %s\n", code, msg)
	} else {
		logger.Printf("  ✓ MDTM: %s\n", strings.TrimSpace(msg))
	}
	
	err = c.PrintfLine("MLST small.txt")
	if err != nil {
		return fmt.Errorf("发送 MLST 命令失败: %w", err)
	}
	
	code, msg, err = c.ReadResponse(250)
	if err != nil {
		logger.Printf("  ⚠ MLST 不支持或文件不存在: %d %s\n", code, msg)
	} else {
		logger.Printf("  ✓ MLST 响应:\n%s\n", msg)
	}
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败: %w", err)
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("MLSD")
	if err != nil {
		return fmt.Errorf("发送 MLSD 命令失败: %w", err)
	}
	
	reader := bufio.NewReader(dataConn)
	var mlsdData strings.Builder
	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			if err == io.EOF {
				break
			}
			return fmt.Errorf("读取 MLSD 数据失败: %w", err)
		}
		mlsdData.WriteString(line)
	}
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		logger.Printf("  ⚠ MLSD 不支持: %v\n", err)
	} else {
		logger.Printf("  ✓ MLSD: 获取 %d 字节\n", mlsdData.Len())
	}
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testUtf8Filename() error {
	startTime := time.Now()
	logger.Printf("  [UTF-8] 测试 UTF-8 文件名支持...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	err = c.PrintfLine("OPTS UTF8 ON")
	if err != nil {
		return fmt.Errorf("发送 OPTS UTF8 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(200)
	if err != nil {
		logger.Printf("  ⚠ UTF-8 选项不支持: %d %s\n", code, msg)
		logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
		return nil
	}
	logger.Printf("  ✓ UTF-8 已启用: %s\n", strings.TrimSpace(msg))
	
	utf8Filename := "测试文件_中文.txt"
	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	if _, _, err := c.ReadResponse(200); err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败: %w", err)
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("STOR %s", utf8Filename)
	if err != nil {
		return fmt.Errorf("发送 STOR 命令失败: %w", err)
	}
	
	code, msg, err = c.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = c.ReadResponse(125)
		}
		if err != nil {
			return fmt.Errorf("STOR 准备响应错误: %d %s (err: %v)", code, msg, err)
		}
	}
	
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()
	
	_, err = io.Copy(dataConn, file)
	if err != nil {
		return fmt.Errorf("传输文件失败: %w", err)
	}
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	
	logger.Printf("  ✓ 上传 UTF-8 文件名文件: %s\n", utf8Filename)
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}
	
	host, port, err = parsePasvResponse(msg)
	if err != nil {
		return fmt.Errorf("解析 PASV 响应失败: %w", err)
	}
	
	dataConn, err = net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	
	err = c.PrintfLine("NLST")
	if err != nil {
		return fmt.Errorf("发送 NLST 命令失败: %w", err)
	}
	
	reader := bufio.NewReader(dataConn)
	var fileList strings.Builder
	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			if err == io.EOF {
				break
			}
			return fmt.Errorf("读取列表数据失败: %w", err)
		}
		fileList.WriteString(line)
	}
	dataConn.Close()
	if _, _, err := c.ReadResponse(226); err != nil {
		logger.Printf("  ⚠ NLST 传输确认响应异常: %v\n", err)
	}
	
	logger.Printf("  ✓ UTF-8 文件名测试完成\n")
	
	err = c.PrintfLine("DELE %s", utf8Filename)
	if err != nil {
		return fmt.Errorf("发送 DELE 命令失败: %w", err)
	}
	if _, _, err := c.ReadResponse(250); err != nil {
		logger.Printf("  ⚠ 删除 UTF-8 文件失败: %v\n", err)
	}
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testConcurrentTransfer() error {
	startTime := time.Now()
	logger.Printf("  [并发] 测试并发传输...\n")
	
	numTransfers := config.MaxConcurrent
	errors := make(chan error, numTransfers)
	
	for i := 0; i < numTransfers; i++ {
		go func(id int) {
			err := concurrentUpload(id)
			errors <- err
		}(i)
	}
	
	successCount := 0
	failCount := 0
	for i := 0; i < numTransfers; i++ {
		err := <-errors
		if err != nil {
			logger.Printf("  ✗ 并发传输 %d 失败: %v\n", i+1, err)
			failCount++
		} else {
			successCount++
		}
	}
	
	if failCount > 0 {
		return fmt.Errorf("并发传输测试部分失败: %d 成功，%d 失败", successCount, failCount)
	}
	
	logger.Printf("  ✓ 并发传输测试成功: %d 个并发传输全部成功\n", successCount)
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func concurrentUpload(id int) error {
	c, err := connectAndLogin()
	if err != nil {
		return fmt.Errorf("连接失败: %w", err)
	}
	defer c.Close()
	
	filename := fmt.Sprintf("concurrent_test_%d.txt", id)
	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return err
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return err
	}
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return err
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 错误: %d %s", code, msg)
	}
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return err
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return err
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("STOR %s", filename)
	if err != nil {
		return err
	}
	
	code, msg, err = c.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = c.ReadResponse(125)
		}
		if err != nil {
			return fmt.Errorf("STOR 准备响应错误: %d %s", code, msg)
		}
	}
	
	file, err := os.Open(srcPath)
	if err != nil {
		return err
	}
	defer file.Close()
	
	_, err = io.Copy(dataConn, file)
	if err != nil {
		return err
	}
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return err
	}
	
	c.PrintfLine("DELE %s", filename)
	c.ReadResponse(250)
	
	return nil
}

func printReport() {
	logger.Println("========================================")
	logger.Println("测试报告")
	logger.Println("========================================")
	
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
		
		logger.Printf("%2d. [%s] %s\n", i+1, status, result.Name)
		if result.Error != nil {
			logger.Printf("    错误: %v\n", result.Error)
		}
		logger.Printf("    耗时: %.2f ms\n", float64(result.Duration.Microseconds())/1000.0)
	}
	
	logger.Println()
	logger.Printf("总计: %d 项测试，%d 通过，%d 失败\n", passed+failed, passed, failed)
	logger.Println("========================================")
}

func testPerformanceBenchmark() error {
	startTime := time.Now()
	logger.Printf("  [性能] 测试传输性能...\n")
	
	testFiles := []struct {
		name     string
		size     int
		category string
	}{
		{"small.txt", 1024, "小文件 (1KB)"},
		{"medium.bin", 1024 * 1024, "中文件 (1MB)"},
	}
	
	for _, tf := range testFiles {
		testPath := filepath.Join(config.TestDataDir, tf.name)
		if _, err := os.Stat(testPath); os.IsNotExist(err) {
			f, err := os.Create(testPath)
			if err != nil {
				return fmt.Errorf("创建测试文件失败: %w", err)
			}
			bufWriter := bufio.NewWriter(f)
			written := 0
			for written < tf.size {
				chunk := tf.size - written
				if chunk > 4096 {
					chunk = 4096
				}
				bufWriter.Write(make([]byte, chunk))
				written += chunk
			}
			bufWriter.Flush()
			f.Close()
		}
		
		uploadStart := time.Now()
		c, err := connectAndLogin()
		if err != nil {
			return err
		}
		
		err = c.PrintfLine("TYPE I")
		if err != nil {
			c.Close()
			return err
		}
		if _, _, err := c.ReadResponse(200); err != nil {
			c.Close()
			return fmt.Errorf("TYPE 命令错误: %w", err)
		}
		
		err = c.PrintfLine("PASV")
		if err != nil {
			c.Close()
			return err
		}
		code, msg, err := c.ReadResponse(227)
		if err != nil {
			c.Close()
			return fmt.Errorf("PASV 错误: %d %s", code, msg)
		}
		
		host, port, err := parsePasvResponse(msg)
		if err != nil {
			c.Close()
			return err
		}
		
		timeout := time.Duration(config.TimeoutSeconds) * time.Second
		dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
		if err != nil {
			c.Close()
			return err
		}
		
		err = c.PrintfLine("STOR perf_test_%s", tf.name)
		if err != nil {
			dataConn.Close()
			c.Close()
			return err
		}
		
		code, msg, err = c.ReadResponse(150)
		if err != nil {
			if code == 0 {
				code, msg, err = c.ReadResponse(125)
			}
			if err != nil {
				dataConn.Close()
				c.Close()
				return fmt.Errorf("STOR 准备响应错误: %d %s", code, msg)
			}
		}
		
		file, err := os.Open(testPath)
		if err != nil {
			dataConn.Close()
			c.Close()
			return err
		}
		
		bytesSent, err := io.Copy(dataConn, file)
		file.Close()
		dataConn.Close()
		
		if err != nil {
			c.Close()
			return err
		}
		
		if _, _, err := c.ReadResponse(226); err != nil {
			logger.Printf("  ⚠ 传输确认响应异常: %v\n", err)
		}
		c.Close()
		
		uploadDuration := time.Since(uploadStart)
		throughput := float64(bytesSent) / uploadDuration.Seconds() / 1024 / 1024
		
		logger.Printf("  ✓ %s: %.2f MB/s (%.2f ms)\n",
			tf.category,
			throughput,
			float64(uploadDuration.Microseconds())/1000.0)
		
		c2, _ := connectAndLogin()
		if c2 != nil {
			c2.PrintfLine("DELE perf_test_%s", tf.name)
			c2.ReadResponse(250)
			c2.Close()
		}
	}
	
	logger.Printf("  [总耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testAbortTransfer() error {
	startTime := time.Now()
	logger.Printf("  [ABOR] 测试中止传输...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	srcPath := filepath.Join(config.TestDataDir, "medium.bin")
	if _, err := os.Stat(srcPath); os.IsNotExist(err) {
		logger.Printf("  ⚠ medium.bin 不存在，跳过 ABOR 测试\n")
		return nil
	}
	
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return err
	}
	if _, _, err := c.ReadResponse(200); err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return err
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 错误: %d %s", code, msg)
	}
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return err
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return err
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("STOR abort_test.bin")
	if err != nil {
		return err
	}
	
	file, err := os.Open(srcPath)
	if err != nil {
		return err
	}
	defer file.Close()
	
	buf := make([]byte, 1024)
	file.Read(buf)
	dataConn.Write(buf)
	
	err = c.SendAbort()
	if err != nil {
		logger.Printf("  ⚠ 发送 ABOR 失败: %v\n", err)
	}
	
	code, msg, err = c.ReadResponse(225)
	if err != nil {
		code, msg, err = c.ReadResponse(426)
		if err != nil {
			logger.Printf("  ⚠ ABOR 响应异常: %v\n", err)
		} else {
			logger.Printf("  ✓ ABOR: %d %s\n", code, strings.TrimSpace(msg))
		}
	} else {
		logger.Printf("  ✓ ABOR: %d %s\n", code, strings.TrimSpace(msg))
	}
	
	c.PrintfLine("DELE abort_test.bin")
	c.ReadResponse(250)
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testQuitGracefully() error {
	startTime := time.Now()
	logger.Printf("  [QUIT] 测试优雅退出...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	
	err = c.PrintfLine("QUIT")
	if err != nil {
		c.Close()
		return fmt.Errorf("发送 QUIT 命令失败: %w", err)
	}
	
	code, msg, err := c.ReadResponse(221)
	if err != nil {
		c.Close()
		return fmt.Errorf("QUIT 响应错误: %w", err)
	}
	
	logger.Printf("  ✓ QUIT: %d %s\n", code, strings.TrimSpace(msg))
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testAsciiMode() error {
	startTime := time.Now()
	logger.Printf("  [ASCII] 测试 ASCII 传输模式...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	err = c.PrintfLine("TYPE A")
	if err != nil {
		return fmt.Errorf("发送 TYPE A 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE A 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ TYPE A: %s\n", strings.TrimSpace(msg))
	
	asciiFile := "ascii_test.txt"
	asciiContent := "Line 1\r\nLine 2\r\nLine 3\r\n"
	asciiPath := filepath.Join(config.TestDataDir, asciiFile)
	os.WriteFile(asciiPath, []byte(asciiContent), 0644)
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return err
	}
	code, msg, err = c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 错误: %d %s", code, msg)
	}
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return err
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return err
	}
	defer dataConn.Close()
	
	err = c.PrintfLine("STOR %s", asciiFile)
	if err != nil {
		return err
	}
	
	code, msg, err = c.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = c.ReadResponse(125)
		}
		if err != nil {
			return fmt.Errorf("STOR 准备响应错误: %d %s (err: %v)", code, msg, err)
		}
	}
	
	file, err := os.Open(asciiPath)
	if err != nil {
		return err
	}
	defer file.Close()
	
	_, err = io.Copy(dataConn, file)
	if err != nil {
		return err
	}
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	
	logger.Printf("  ✓ ASCII 模式上传成功\n")
	
	c.PrintfLine("DELE %s", asciiFile)
	c.ReadResponse(250)
	os.Remove(asciiPath)
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testLongConnectionKeepalive() error {
	startTime := time.Now()
	logger.Printf("  [保活] 测试长时间连接保活...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	keepaliveDuration := 5 * time.Second
	interval := 1 * time.Second
	elapsed := time.Duration(0)
	noopCount := 0
	
	logger.Printf("  ✓ 保持连接 %v...\n", keepaliveDuration)
	
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	
	done := time.After(keepaliveDuration)
	
	for {
		select {
		case <-done:
			logger.Printf("  ✓ 保活测试完成，发送 %d 次 NOOP\n", noopCount)
			logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
			return nil
		case <-ticker.C:
			err = c.PrintfLine("NOOP")
			if err != nil {
				return fmt.Errorf("NOOP 命令失败: %w", err)
			}
			code, msg, err := c.ReadResponse(200)
			if err != nil {
				return fmt.Errorf("NOOP 响应错误: %d %s", code, msg)
			}
			noopCount++
			elapsed += interval
			logger.Printf("    [%v] NOOP #%d: %s\n", elapsed, noopCount, strings.TrimSpace(msg))
		}
	}
}

func testStatAndHelp() error {
	startTime := time.Now()
	logger.Printf("  [状态] 测试 STAT/HELP 命令...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	err = c.PrintfLine("STAT")
	if err != nil {
		return fmt.Errorf("发送 STAT 命令失败: %w", err)
	}
	
	_, msg, err := c.ReadResponse(211)
	if err != nil {
		_, msg, err = c.ReadResponse(213)
		if err != nil {
			logger.Printf("  ⚠ STAT 不支持或异常: %v\n", err)
		} else {
			logger.Printf("  ✓ STAT (213): %d 字节\n", len(msg))
		}
	} else {
		logger.Printf("  ✓ STAT (211): %d 字节\n", len(msg))
	}
	
	err = c.PrintfLine("HELP")
	if err != nil {
		return fmt.Errorf("发送 HELP 命令失败: %w", err)
	}
	
	_, msg, err = c.ReadResponse(214)
	if err != nil {
		logger.Printf("  ⚠ HELP 不支持或异常: %v\n", err)
	} else {
		logger.Printf("  ✓ HELP: %d 字节\n", len(msg))
	}
	
	err = c.PrintfLine("SITE HELP")
	if err != nil {
		return fmt.Errorf("发送 SITE HELP 命令失败: %w", err)
	}
	
	_, msg, err = c.ReadResponse(200)
	if err != nil {
		logger.Printf("  ⚠ SITE HELP 不支持: %v\n", err)
	} else {
		logger.Printf("  ✓ SITE HELP: %s\n", strings.TrimSpace(msg))
	}
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testAppendFile() error {
	startTime := time.Now()
	logger.Printf("  [APPE] 测试文件追加模式...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	appeFile := "appe_test.txt"
	testPath := filepath.Join(config.TestDataDir, appeFile)
	
	initialContent := "Initial content.\n"
	os.WriteFile(testPath, []byte(initialContent), 0644)
	
	err = c.PrintfLine("TYPE I")
	if err != nil {
		return err
	}
	if _, _, err := c.ReadResponse(200); err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return err
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 错误: %d %s", code, msg)
	}
	
	host, port, err := parsePasvResponse(msg)
	if err != nil {
		return err
	}
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return err
	}
	
	err = c.PrintfLine("STOR %s", appeFile)
	if err != nil {
		dataConn.Close()
		return err
	}
	
	code, msg, err = c.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = c.ReadResponse(125)
		}
		if err != nil {
			dataConn.Close()
			return fmt.Errorf("STOR 准备响应错误: %d %s", code, msg)
		}
	}
	
	file, err := os.Open(testPath)
	if err != nil {
		dataConn.Close()
		return err
	}
	
	io.Copy(dataConn, file)
	file.Close()
	dataConn.Close()
	
	if _, _, err := c.ReadResponse(226); err != nil {
		logger.Printf("  ⚠ STOR 传输确认响应异常: %v\n", err)
	}
	logger.Printf("  ✓ 初始上传完成\n")
	
	appendContent := "Appended content.\n"
	os.WriteFile(testPath, []byte(appendContent), 0644)
	
	err = c.PrintfLine("PASV")
	if err != nil {
		return err
	}
	code, msg, err = c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 错误: %d %s", code, msg)
	}
	
	host, port, err = parsePasvResponse(msg)
	if err != nil {
		return err
	}
	
	dataConn, err = net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return err
	}
	
	err = c.PrintfLine("APPE %s", appeFile)
	if err != nil {
		dataConn.Close()
		return err
	}
	
	code, msg, err = c.ReadResponse(150)
	if err != nil {
		if code == 0 {
			code, msg, err = c.ReadResponse(125)
		}
		if err != nil {
			dataConn.Close()
			return fmt.Errorf("APPE 准备响应错误: %d %s", code, msg)
		}
	}
	
	file, err = os.Open(testPath)
	if err != nil {
		dataConn.Close()
		return err
	}
	
	io.Copy(dataConn, file)
	file.Close()
	dataConn.Close()
	
	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("APPE 传输确认错误: %w", err)
	}
	
	logger.Printf("  ✓ 追加模式上传成功\n")
	
	c.PrintfLine("DELE %s", appeFile)
	c.ReadResponse(250)
	os.Remove(testPath)
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testModeAndStru() error {
	startTime := time.Now()
	logger.Printf("  [MODE] 测试 MODE/STRU 命令...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	err = c.PrintfLine("MODE S")
	if err != nil {
		return fmt.Errorf("发送 MODE S 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(200)
	if err != nil {
		logger.Printf("  ⚠ MODE S 不支持: %d %s\n", code, msg)
	} else {
		logger.Printf("  ✓ MODE S: %s\n", strings.TrimSpace(msg))
	}
	
	err = c.PrintfLine("MODE B")
	if err != nil {
		return fmt.Errorf("发送 MODE B 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(200)
	if err != nil {
		logger.Printf("  ⚠ MODE B 不支持: %d %s\n", code, msg)
	} else {
		logger.Printf("  ✓ MODE B: %s\n", strings.TrimSpace(msg))
	}
	
	err = c.PrintfLine("STRU F")
	if err != nil {
		return fmt.Errorf("发送 STRU F 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(200)
	if err != nil {
		logger.Printf("  ⚠ STRU F 不支持: %d %s\n", code, msg)
	} else {
		logger.Printf("  ✓ STRU F: %s\n", strings.TrimSpace(msg))
	}
	
	err = c.PrintfLine("STRU R")
	if err != nil {
		return fmt.Errorf("发送 STRU R 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(200)
	if err != nil {
		logger.Printf("  ⚠ STRU R 不支持: %d %s\n", code, msg)
	} else {
		logger.Printf("  ✓ STRU R: %s\n", strings.TrimSpace(msg))
	}
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}
