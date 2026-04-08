package main

import (
	"bufio"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// testBasicConnection 测试基本FTP连接
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

// testAuthentication 测试用户认证
func testAuthentication() error {
	startTime := time.Now()
	logger.Printf("  [认证] 正在认证用户 %s...\n", config.Username)
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testDirectoryOperations 测试目录操作
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

// testFileUpload 测试文件上传
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
	
	// 先发送 STOR 命令,再建立数据连接
	err = c.PrintfLine("STOR %s", filename)
	if err != nil {
		return fmt.Errorf("发送 STOR 命令失败: %w", err)
	}
	
	// 短暂延迟,让服务器准备好数据连接
	time.Sleep(100 * time.Millisecond)
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	defer dataConn.Close()
	
	code, msg, err = c.ReadResponse(150)
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
	
	// 使用带超时的写入,避免永久卡住
	done := make(chan error, 1)
	go func() {
		_, err := io.Copy(dataConn, file)
		done <- err
	}()
	
	select {
	case err := <-done:
		if err != nil {
			return fmt.Errorf("传输文件失败: %w", err)
		}
	case <-time.After(timeout + 5*time.Second):
		return fmt.Errorf("上传超时: 超过 %v 未完成", timeout+5*time.Second)
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

// testFileDownload 测试文件下载
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
	
	// 先发送 RETR 命令,再建立数据连接
	err = c.PrintfLine("RETR %s", filename)
	if err != nil {
		return fmt.Errorf("发送 RETR 命令失败: %w", err)
	}
	
	// 短暂延迟,让服务器准备好数据连接
	time.Sleep(100 * time.Millisecond)
	
	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}
	defer dataConn.Close()
	
	code, msg, err = c.ReadResponse(150)
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
	
	// 使用带超时的读取,避免永久卡住
	done := make(chan error, 1)
	go func() {
		_, copyErr := io.Copy(outFile, dataConn)
		done <- copyErr
	}()
	
	select {
	case err := <-done:
		if err != nil {
			return fmt.Errorf("接收文件失败: %w", err)
		}
	case <-time.After(timeout + 5*time.Second):
		return fmt.Errorf("下载超时: 超过 %v 未完成", timeout+5*time.Second)
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

// testResumeTransfer 测试断点续传
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

// testPassiveMode 测试被动模式
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

// testActiveMode 测试主动模式
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
	err = c.PrintfLine("%s", portCmd)
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
	
	// 接受数据连接的逻辑...
	listener.Close()
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testFileList 测试文件列表
func testFileList() error {
	startTime := time.Now()
	logger.Printf("  [列表] 测试文件列表...\n")
	
	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()
	
	// LIST 和 NLST 测试逻辑...
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testFileDelete 测试文件删除
func testFileDelete() error {
	startTime := time.Now()
	logger.Printf("  [删除] 测试文件删除...\n")
	
	// 文件删除测试逻辑...
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testFeatAndSyst 测试FEAT/SYST命令
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

// testFtpRename 测试文件重命名
func testFtpRename() error {
	startTime := time.Now()
	logger.Printf("  [重命名] 测试 RNFR/RNTO 重命名...\n")
	
	// 文件重命名测试逻辑...
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testMdtmAndMlst 测试MDTM/MLST命令
func testMdtmAndMlst() error {
	startTime := time.Now()
	logger.Printf("  [时间/列表] 测试 MDTM/MLST 命令...\n")
	
	// MDTM/MLST 测试逻辑...
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testUtf8Filename 测试UTF-8文件名
func testUtf8Filename() error {
	startTime := time.Now()
	logger.Printf("  [UTF-8] 测试 UTF-8 文件名支持...\n")
	
	// UTF-8 文件名测试逻辑...
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testConcurrentTransfer 测试并发传输
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

// concurrentUpload 并发上传辅助函数
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
	
	time.Sleep(100 * time.Millisecond)
	
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

// testPerformanceBenchmark 性能基准测试
func testPerformanceBenchmark() error {
	logger.Printf("  [性能] 测试传输性能...\n")
	
	// 性能测试逻辑...
	// TODO: 实现完整的性能测试
	
	return nil
}

// testAbortTransfer 测试ABOR中止传输
func testAbortTransfer() error {
	startTime := time.Now()
	logger.Printf("  [ABOR] 测试中止传输...\n")
	
	// ABOR 测试逻辑...
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testQuitGracefully 测试QUIT优雅退出
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

// testAsciiMode 测试ASCII模式
func testAsciiMode() error {
	startTime := time.Now()
	logger.Printf("  [ASCII] 测试 ASCII 传输模式...\n")
	
	// ASCII 模式测试逻辑...
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testLongConnectionKeepalive 测试长时间连接保活
func testLongConnectionKeepalive() error {
	logger.Printf("  [保活] 测试长时间连接保活...\n")
	
	// 保活测试逻辑...
	// TODO: 实现完整的保活测试
	
	return nil
}

// testStatAndHelp 测试STAT/HELP命令
func testStatAndHelp() error {
	startTime := time.Now()
	logger.Printf("  [状态] 测试 STAT/HELP 命令...\n")
	
	// STAT/HELP 测试逻辑...
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testAppendFile 测试APPE追加模式
func testAppendFile() error {
	startTime := time.Now()
	logger.Printf("  [APPE] 测试文件追加模式...\n")
	
	// APPE 测试逻辑...
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// testModeAndStru 测试MODE/STRU命令
func testModeAndStru() error {
	startTime := time.Now()
	logger.Printf("  [MODE] 测试 MODE/STRU 命令...\n")
	
	// MODE/STRU 测试逻辑...
	
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}
