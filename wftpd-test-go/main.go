package main

import (
	"bufio"
	"crypto/md5"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"log"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// TestConfig 测试配置结构
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

// TestResult 测试结果结构
type TestResult struct {
	Name      string
	Passed    bool
	Duration  time.Duration
	Error     error
	Responses []string
}

// Logger 日志记录器
type Logger struct {
	file    *os.File
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
		// 配置文件不存在时使用默认值，记录警告
		if logger != nil {
			logger.Printf("  ⚠ 配置文件 %s 不存在，使用默认配置\n", configPath)
		}
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
	
	if err := generateTestFiles(); err != nil {
		logger.Printf("生成测试文件失败: %v\n", err)
		return
	}
	
	runFTPTests()
	runSFTPTests()
	
	printReport()
}

// testResult 执行测试并记录结果
func testResult(name string, testFunc func() error) {
	result := TestResult{
		Name:   name,
		Passed: true,
	}
	
	startTime := time.Now()
	err := testFunc()
	result.Duration = time.Since(startTime)
	
	if err != nil {
		result.Passed = false
		result.Error = err
		logger.Printf("  ✗ 失败: %v\n", err)
	}
	
	testResults = append(testResults, result)
	logger.Println()
}

// calculateMD5 计算文件MD5哈希
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

// printReport 打印测试报告
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

func generateTestFiles() error {
	logger.Println("[准备] 生成测试文件...")
	
	smallFile := filepath.Join(config.TestDataDir, "small.txt")
	if err := os.WriteFile(smallFile, []byte(strings.Repeat("A", 1024)), 0644); err != nil {
		return fmt.Errorf("创建小文件失败: %w", err)
	}
	logger.Printf("  ✓ 创建小文件: %s (1KB)\n", smallFile)
	
	mediumFile := filepath.Join(config.TestDataDir, "medium.bin")
	f, err := os.Create(mediumFile)
	if err != nil {
		return fmt.Errorf("创建中文件失败: %w", err)
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
		return fmt.Errorf("创建大文件失败: %w", err)
	}
	bufWriter = bufio.NewWriter(f)
	for i := 0; i < 10*1024; i++ {
		bufWriter.Write(make([]byte, 1024))
	}
	bufWriter.Flush()
	f.Close()
	logger.Printf("  ✓ 创建大文件: %s (10MB)\n", largeFile)
	
	logger.Println()
	return nil
}

// runFTPTests 运行FTP测试套件
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
