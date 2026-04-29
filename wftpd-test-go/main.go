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
	var totalDuration time.Duration
	var failedTests []TestResult
	
	ftpPassed := 0
	ftpFailed := 0
	sftpPassed := 0
	sftpFailed := 0
	
	for i, result := range testResults {
		status := "✓ 通过"
		if !result.Passed {
			status = "✗ 失败"
			failed++
			failedTests = append(failedTests, result)
		} else {
			passed++
		}
		
		totalDuration += result.Duration
		
		if strings.HasPrefix(result.Name, "FTP") {
			if result.Passed {
				ftpPassed++
			} else {
				ftpFailed++
			}
		} else if strings.HasPrefix(result.Name, "SFTP") {
			if result.Passed {
				sftpPassed++
			} else {
				sftpFailed++
			}
		}
		
		logger.Printf("%2d. [%s] %s\n", i+1, status, result.Name)
		if result.Error != nil {
			logger.Printf("    错误: %v\n", result.Error)
		}
		logger.Printf("    耗时: %.2f ms\n", float64(result.Duration.Microseconds())/1000.0)
	}
	
	logger.Println()
	logger.Println("========================================")
	logger.Println("测试统计")
	logger.Println("========================================")
	logger.Printf("总计: %d 项测试\n", passed+failed)
	logger.Printf("通过: %d 项 (%.1f%%)\n", passed, float64(passed)/float64(passed+failed)*100)
	logger.Printf("失败: %d 项 (%.1f%%)\n", failed, float64(failed)/float64(passed+failed)*100)
	logger.Printf("总耗时: %.2f 秒\n", totalDuration.Seconds())
	logger.Printf("平均耗时: %.2f 毫秒/测试\n", float64(totalDuration.Milliseconds())/float64(passed+failed))
	
	logger.Println()
	logger.Println("========================================")
	logger.Println("分类统计")
	logger.Println("========================================")
	logger.Printf("FTP 测试: %d 通过, %d 失败 (总计 %d)\n", ftpPassed, ftpFailed, ftpPassed+ftpFailed)
	logger.Printf("SFTP 测试: %d 通过, %d 失败 (总计 %d)\n", sftpPassed, sftpFailed, sftpPassed+sftpFailed)
	
	if len(failedTests) > 0 {
		logger.Println()
		logger.Println("========================================")
		logger.Println("失败测试详情")
		logger.Println("========================================")
		for i, test := range failedTests {
			logger.Printf("%d. %s\n", i+1, test.Name)
			logger.Printf("   错误: %v\n", test.Error)
			logger.Printf("   耗时: %.2f ms\n", float64(test.Duration.Microseconds())/1000.0)
		}
	}
	
	logger.Println()
	logger.Println("========================================")
	logger.Println("测试完成")
	logger.Println("========================================")
	logger.Printf("测试时间: %s\n", time.Now().Format("2006-01-02 15:04:05"))
	logger.Printf("测试结果: ")
	if failed == 0 {
		logger.Println("✓ 全部通过")
	} else {
		logger.Printf("✗ %d 项失败\n", failed)
	}
	logger.Println("========================================")
}

func generateTestFiles() error {
	logger.Println("[准备] 生成测试文件...")
	
	emptyFile := filepath.Join(config.TestDataDir, "empty.txt")
	if err := os.WriteFile(emptyFile, []byte{}, 0644); err != nil {
		return fmt.Errorf("创建空文件失败: %w", err)
	}
	logger.Printf("  ✓ 创建空文件: %s (0 bytes)\n", emptyFile)
	
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
	
	large100MBFile := filepath.Join(config.TestDataDir, "large_100mb.bin")
	if _, err := os.Stat(large100MBFile); os.IsNotExist(err) {
		logger.Printf("  [提示] 正在创建超大文件 (100MB)，可能需要一些时间...\n")
		f, err = os.Create(large100MBFile)
		if err != nil {
			return fmt.Errorf("创建超大文件失败: %w", err)
		}
		bufWriter = bufio.NewWriter(f)
		for i := 0; i < 100*1024; i++ {
			bufWriter.Write(make([]byte, 1024))
		}
		bufWriter.Flush()
		f.Close()
		logger.Printf("  ✓ 创建超大文件: %s (100MB)\n", large100MBFile)
	} else {
		logger.Printf("  ✓ 超大文件已存在: %s (100MB)\n", large100MBFile)
	}
	
	binaryFile := filepath.Join(config.TestDataDir, "binary_test.bin")
	binaryData := make([]byte, 1024)
	for i := range binaryData {
		binaryData[i] = byte(i % 256)
	}
	if err := os.WriteFile(binaryFile, binaryData, 0644); err != nil {
		return fmt.Errorf("创建二进制文件失败: %w", err)
	}
	logger.Printf("  ✓ 创建二进制文件: %s (1KB)\n", binaryFile)
	
	unicodeFile := filepath.Join(config.TestDataDir, "unicode_test.txt")
	unicodeContent := "测试中文内容\n日本語テスト\n한국어 테스트\nΕλληνικά\nالعربية\nעברית\nไทย\nemoji 😀🎉\n"
	if err := os.WriteFile(unicodeFile, []byte(unicodeContent), 0644); err != nil {
		return fmt.Errorf("创建Unicode文件失败: %w", err)
	}
	logger.Printf("  ✓ 创建Unicode文件: %s\n", unicodeFile)
	
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
	
	logger.Println("========================================")
	logger.Println("FTP 边界条件测试")
	logger.Println("========================================")
	logger.Println()
	
	testResult("FTP 空文件传输", func() error {
		return testEmptyFileTransfer()
	})
	
	testResult("FTP 特殊字符文件名", func() error {
		return testSpecialCharacterFilename()
	})
	
	testResult("FTP 超长文件名", func() error {
		return testLongFilename()
	})
	
	testResult("FTP 超大文件传输 (100MB)", func() error {
		return testLargeFileTransfer()
	})
	
	testResult("FTP 路径遍历防护", func() error {
		return testPathTraversalProtection()
	})
	
	testResult("FTP 二进制文件传输", func() error {
		return testBinaryFileTransfer()
	})
	
	testResult("FTP Unicode文件名", func() error {
		return testUnicodeFilename()
	})
	
	logger.Println("========================================")
	logger.Println("FTP 错误恢复测试")
	logger.Println("========================================")
	logger.Println()
	
	testResult("FTP 网络中断恢复", func() error {
		return testNetworkInterruption()
	})
	
	testResult("FTP 权限拒绝处理", func() error {
		return testPermissionDenied()
	})
	
	testResult("FTP 并发访问冲突", func() error {
		return testConcurrentAccess()
	})
	
	testResult("FTP 无效命令处理", func() error {
		return testInvalidCommands()
	})
	
	testResult("FTP 畸形命令处理", func() error {
		return testMalformedCommands()
	})
	
	testResult("FTP 超时处理", func() error {
		return testTimeoutHandling()
	})
	
	testResult("FTP 数据连接失败", func() error {
		return testDataConnectionFailure()
	})
	
	logger.Println("========================================")
	logger.Println("FTP 安全测试")
	logger.Println("========================================")
	logger.Println()
	
	testResult("FTP 命令注入防护", func() error {
		return testCommandInjection()
	})
	
	testResult("FTP 缓冲区溢出防护", func() error {
		return testBufferOverflow()
	})
	
	testResult("FTP 未授权访问防护", func() error {
		return testUnauthorizedAccess()
	})
	
	testResult("FTP 敏感信息泄露防护", func() error {
		return testSensitiveDataLeak()
	})
	
	testResult("FTP 匿名访问控制", func() error {
		return testAnonymousAccess()
	})
	
	testResult("FTP PORT 命令安全性", func() error {
		return testPortCommandSecurity()
	})
	
	testResult("FTP PASV 命令安全性", func() error {
		return testPasvSecurity()
	})
	
	testResult("FTP 暴力破解防护", func() error {
		return testBruteForceProtection()
	})
	
	logger.Println("========================================")
	logger.Println("FTP 协议扩展测试")
	logger.Println("========================================")
	logger.Println()
	
	testResult("FTP TYPE 命令扩展", func() error {
		return testTypeCommand()
	})
	
	testResult("FTP ALLO 命令", func() error {
		return testAlloCommand()
	})
	
	testResult("FTP SITE 命令", func() error {
		return testSiteCommand()
	})
	
	testResult("FTP ACCT 命令", func() error {
		return testAcctCommand()
	})
	
	testResult("FTP SMNT 命令", func() error {
		return testSmntCommand()
	})
	
	testResult("FTP REIN 命令", func() error {
		return testReinCommand()
	})
	
	testResult("FTP STOU 命令", func() error {
		return testStouCommand()
	})
	
	testResult("FTP APPE 命令", func() error {
		return testAppeCommand()
	})
	
	logger.Println("========================================")
	logger.Println("FTP 性能测试")
	logger.Println("========================================")
	logger.Println()
	
	testResult("FTP 批量文件传输", func() error {
		return testBatchTransfer()
	})
	
	testResult("FTP 传输队列管理", func() error {
		return testTransferQueue()
	})
	
	testResult("FTP 带宽限制效果", func() error {
		return testBandwidthLimit()
	})
	
	testResult("FTP 资源使用情况", func() error {
		return testResourceUsage()
	})
	
	testResult("FTP 压力测试", func() error {
		return testStressTest()
	})
	
	logger.Println()
}
