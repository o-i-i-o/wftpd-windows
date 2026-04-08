package main

import (
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

func testBasicConnection() error {
	startTime := time.Now()
	logger.Printf("  [连接] 正在连接到 %s:%d...\n", config.FTPServer, config.FTPPort)

	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	conn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort), timeout)
	if err != nil {
		return fmt.Errorf("连接失败: %w", err)
	}
	defer conn.Close()

	buf := make([]byte, 1024)
	n, err := conn.Read(buf)
	if err != nil {
		return fmt.Errorf("读取欢迎消息失败: %w", err)
	}

	line := string(buf[:n])
	if !strings.HasPrefix(line, "220") {
		return fmt.Errorf("意外的欢迎消息: %s", strings.TrimSpace(line))
	}

	logger.Printf("  ✓ 连接成功，响应: %s", strings.TrimSpace(line))
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

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

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()

	err = dt.Upload(file, filename)
	if err != nil {
		return err
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

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	outFile, err := os.Create(dstPath)
	if err != nil {
		return fmt.Errorf("创建输出文件失败: %w", err)
	}
	defer outFile.Close()

	err = dt.Download(outFile, filename)
	if err != nil {
		return err
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

	err = c.PrintfLine("PASV")
	if err != nil {
		return fmt.Errorf("发送 PASV 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(227)
	if err != nil {
		return fmt.Errorf("PASV 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ PASV: %s\n", strings.TrimSpace(msg))

	err = c.PrintfLine("EPSV")
	if err != nil {
		return fmt.Errorf("发送 EPSV 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(229)
	if err != nil {
		return fmt.Errorf("EPSV 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ EPSV: %s\n", strings.TrimSpace(msg))

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

	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}

	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return fmt.Errorf("创建监听器失败: %w", err)
	}

	addr := listener.Addr().(*net.TCPAddr)
	ipParts := strings.Split(addr.IP.String(), ".")
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

	err = c.PrintfLine("STOR active_test.txt")
	if err != nil {
		listener.Close()
		return fmt.Errorf("发送 STOR 命令失败: %w", err)
	}

	dataConn, err := listener.Accept()
	if err != nil {
		listener.Close()
		return fmt.Errorf("接受数据连接失败: %w", err)
	}
	listener.Close()

	code, msg, err = c.ReadResponse(150)
	if err != nil {
		dataConn.Close()
		return fmt.Errorf("STOR 准备响应错误: %d %s", code, msg)
	}

	testData := []byte("Active mode test content.\n")
	_, err = dataConn.Write(testData)
	dataConn.Close()
	if err != nil {
		return fmt.Errorf("写入数据失败: %w", err)
	}

	_, _, err = c.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	logger.Printf("  ✓ 主动模式上传成功\n")

	c.PrintfLine("DELE active_test.txt")
	c.ReadResponse(250)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testFileList() error {
	startTime := time.Now()
	logger.Printf("  [列表] 测试文件列表...\n")

	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()

	err = c.PrintfLine("TYPE A")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	err = dt.Fc.PrintfLine("LIST")
	if err != nil {
		return fmt.Errorf("发送 LIST 命令失败: %w", err)
	}

	code, msg, err := dt.Fc.ReadResponse(150)
	if err != nil && code != 125 {
		return fmt.Errorf("LIST 准备响应错误: %d %s", code, msg)
	}

	listing, err := dt.ReadListing()
	if err != nil {
		return err
	}
	lineCount := len(strings.Split(strings.TrimSpace(listing), "\n"))
	logger.Printf("  ✓ LIST: %d 行\n", lineCount)

	dt2, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt2.Close()

	err = dt2.Fc.PrintfLine("NLST")
	if err != nil {
		return fmt.Errorf("发送 NLST 命令失败: %w", err)
	}

	code, msg, err = dt2.Fc.ReadResponse(150)
	if err != nil && code != 125 {
		return fmt.Errorf("NLST 准备响应错误: %d %s", code, msg)
	}

	nlst, err := dt2.ReadListing()
	if err != nil {
		return err
	}
	nlstCount := len(strings.Split(strings.TrimSpace(nlst), "\n"))
	logger.Printf("  ✓ NLST: %d 条\n", nlstCount)

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

	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()

	err = dt.Upload(file, "dele_test.txt")
	if err != nil {
		return fmt.Errorf("上传测试文件失败: %w", err)
	}
	logger.Printf("  ✓ 上传测试文件: dele_test.txt\n")

	err = c.PrintfLine("DELE dele_test.txt")
	if err != nil {
		return fmt.Errorf("发送 DELE 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("DELE 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ DELE: %s\n", strings.TrimSpace(msg))

	err = c.PrintfLine("DELE dele_test.txt")
	if err != nil {
		return fmt.Errorf("发送 DELE 命令失败: %w", err)
	}
	code, _, _ = c.ReadResponse(550)
	if code == 550 {
		logger.Printf("  ✓ 删除不存在的文件正确返回 550\n")
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
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

	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()

	err = dt.Upload(file, "rename_old.txt")
	if err != nil {
		return fmt.Errorf("上传测试文件失败: %w", err)
	}
	logger.Printf("  ✓ 上传测试文件: rename_old.txt\n")

	err = c.PrintfLine("RNFR rename_old.txt")
	if err != nil {
		return fmt.Errorf("发送 RNFR 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(350)
	if err != nil {
		return fmt.Errorf("RNFR 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ RNFR: %s\n", strings.TrimSpace(msg))

	err = c.PrintfLine("RNTO rename_new.txt")
	if err != nil {
		return fmt.Errorf("发送 RNTO 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(250)
	if err != nil {
		return fmt.Errorf("RNTO 命令错误: %d %s", code, msg)
	}
	logger.Printf("  ✓ RNTO: %s\n", strings.TrimSpace(msg))

	c.PrintfLine("DELE rename_new.txt")
	c.ReadResponse(250)

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
		logger.Printf("  ⚠ MDTM 不支持: %d %s\n", code, strings.TrimSpace(msg))
	} else {
		logger.Printf("  ✓ MDTM: %s\n", strings.TrimSpace(msg))
	}

	err = c.PrintfLine("MLST small.txt")
	if err != nil {
		return fmt.Errorf("发送 MLST 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(250)
	if err != nil {
		logger.Printf("  ⚠ MLST 不支持: %d %s\n", code, strings.TrimSpace(msg))
	} else {
		logger.Printf("  ✓ MLST: %s\n", strings.TrimSpace(msg))
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
		logger.Printf("  ⚠ UTF8 OPTS 不支持: %d %s\n", code, strings.TrimSpace(msg))
	} else {
		logger.Printf("  ✓ OPTS UTF8 ON: %s\n", strings.TrimSpace(msg))
	}

	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	utf8Filename := "中文文件名_test.txt"
	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()

	err = dt.Upload(file, utf8Filename)
	if err != nil {
		return fmt.Errorf("上传 UTF-8 文件名失败: %w", err)
	}
	logger.Printf("  ✓ 上传 UTF-8 文件名: %s\n", utf8Filename)

	c.PrintfLine("DELE %s", utf8Filename)
	c.ReadResponse(250)

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

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	file, err := os.Open(srcPath)
	if err != nil {
		return err
	}
	defer file.Close()

	err = dt.Upload(file, filename)
	if err != nil {
		return err
	}

	c.PrintfLine("DELE %s", filename)
	c.ReadResponse(250)

	return nil
}

func testPerformanceBenchmark() error {
	startTime := time.Now()
	logger.Printf("  [性能] 测试传输性能...\n")

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

	srcPath := filepath.Join(config.TestDataDir, "medium.bin")
	if _, err := os.Stat(srcPath); os.IsNotExist(err) {
		logger.Printf("  ⚠ medium.bin 不存在，跳过性能测试\n")
		return nil
	}

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()

	uploadStart := time.Now()
	err = dt.Upload(file, "perf_test.bin")
	if err != nil {
		return fmt.Errorf("上传失败: %w", err)
	}
	uploadDuration := time.Since(uploadStart)

	fi, _ := os.Stat(srcPath)
	throughput := float64(fi.Size()) / uploadDuration.Seconds() / 1024 / 1024
	logger.Printf("  ✓ 上传: %.2f MB (%.2f MB/s)\n", float64(fi.Size())/1024/1024, throughput)

	dt2, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt2.Close()

	dstPath := filepath.Join(config.TestDataDir, "perf_downloaded.bin")
	outFile, err := os.Create(dstPath)
	if err != nil {
		return fmt.Errorf("创建输出文件失败: %w", err)
	}
	defer outFile.Close()

	downloadStart := time.Now()
	err = dt2.Download(outFile, "perf_test.bin")
	if err != nil {
		return fmt.Errorf("下载失败: %w", err)
	}
	downloadDuration := time.Since(downloadStart)

	downloadThroughput := float64(fi.Size()) / downloadDuration.Seconds() / 1024 / 1024
	logger.Printf("  ✓ 下载: %.2f MB (%.2f MB/s)\n", float64(fi.Size())/1024/1024, downloadThroughput)

	c.PrintfLine("DELE perf_test.bin")
	c.ReadResponse(250)
	os.Remove(dstPath)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
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
		return err
	}

	err = c.PrintfLine("STOR abort_test.txt")
	if err != nil {
		return fmt.Errorf("发送 STOR 命令失败: %w", err)
	}

	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		return fmt.Errorf("连接数据端口失败: %w", err)
	}

	code, msg, err = c.ReadResponse(150)
	if err != nil && code != 125 {
		dataConn.Close()
		return fmt.Errorf("STOR 准备响应错误: %d %s", code, msg)
	}

	dataConn.Write([]byte("partial data"))
	dataConn.Close()

	err = c.SendAbort()
	if err != nil {
		logger.Printf("  ⚠ ABOR 发送失败: %v\n", err)
	} else {
		code, _, _ := c.ReadResponse(226)
		if code == 226 || code == 426 {
			code2, _, _ := c.ReadResponse(226)
			if code2 == 226 {
				logger.Printf("  ✓ ABOR 成功 (226)\n")
			}
		} else {
			logger.Printf("  ✓ ABOR 响应: %d\n", code)
		}
	}

	c.PrintfLine("DELE abort_test.txt")
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

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()

	err = dt.Upload(file, "ascii_test.txt")
	if err != nil {
		return fmt.Errorf("ASCII 上传失败: %w", err)
	}
	logger.Printf("  ✓ ASCII 上传成功\n")

	dt2, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt2.Close()

	dstPath := filepath.Join(config.TestDataDir, "ascii_downloaded.txt")
	outFile, err := os.Create(dstPath)
	if err != nil {
		return fmt.Errorf("创建输出文件失败: %w", err)
	}
	defer outFile.Close()

	err = dt2.Download(outFile, "ascii_test.txt")
	if err != nil {
		return fmt.Errorf("ASCII 下载失败: %w", err)
	}
	logger.Printf("  ✓ ASCII 下载成功\n")

	c.PrintfLine("DELE ascii_test.txt")
	c.ReadResponse(250)
	os.Remove(dstPath)

	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE I 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE I 命令错误: %w", err)
	}
	logger.Printf("  ✓ 恢复 TYPE I\n")

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

	for i := 0; i < 3; i++ {
		err = c.PrintfLine("NOOP")
		if err != nil {
			return fmt.Errorf("NOOP 命令失败: %w", err)
		}
		code, msg, err := c.ReadResponse(200)
		if err != nil {
			return fmt.Errorf("NOOP 响应错误: %d %s (err: %v)", code, msg, err)
		}
		logger.Printf("  ✓ NOOP #%d: %s\n", i+1, strings.TrimSpace(msg))
		time.Sleep(1 * time.Second)
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
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
	code, msg, err := c.ReadResponse(211)
	if err != nil {
		logger.Printf("  ⚠ STAT 不支持: %d %s\n", code, strings.TrimSpace(msg))
	} else {
		logger.Printf("  ✓ STAT: %s\n", strings.TrimSpace(msg))
	}

	err = c.PrintfLine("HELP")
	if err != nil {
		return fmt.Errorf("发送 HELP 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(214)
	if err != nil {
		logger.Printf("  ⚠ HELP 不支持: %d %s\n", code, strings.TrimSpace(msg))
	} else {
		logger.Printf("  ✓ HELP: %s\n", strings.TrimSpace(msg))
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

	err = c.PrintfLine("TYPE I")
	if err != nil {
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	firstContent := []byte("First line.\n")
	reader := io.LimitReader(newInfiniteReader(firstContent), int64(len(firstContent)))
	err = dt.Upload(reader, "appe_test.txt")
	if err != nil {
		return fmt.Errorf("首次上传失败: %w", err)
	}
	logger.Printf("  ✓ 首次上传: appe_test.txt\n")

	dt2, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt2.Close()

	err = dt2.Fc.PrintfLine("APPE appe_test.txt")
	if err != nil {
		return fmt.Errorf("发送 APPE 命令失败: %w", err)
	}

	code, msg, err := dt2.Fc.ReadResponse(150)
	if err != nil && code != 125 {
		return fmt.Errorf("APPE 准备响应错误: %d %s", code, msg)
	}

	secondContent := []byte("Second line.\n")
	_, err = dt2.Conn.Write(secondContent)
	dt2.Conn.Close()
	if err != nil {
		return fmt.Errorf("追加写入失败: %w", err)
	}

	_, _, err = dt2.Fc.ReadResponse(226)
	if err != nil {
		return fmt.Errorf("传输确认错误: %w", err)
	}
	logger.Printf("  ✓ 追加写入成功\n")

	err = c.PrintfLine("SIZE appe_test.txt")
	if err != nil {
		return fmt.Errorf("发送 SIZE 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(213)
	if err != nil {
		return fmt.Errorf("SIZE 命令错误: %d %s", code, msg)
	}
	expectedSize := len(firstContent) + len(secondContent)
	size, _ := strconv.ParseInt(strings.TrimSpace(msg), 10, 64)
	if int(size) != expectedSize {
		return fmt.Errorf("文件大小不匹配: 期望 %d, 实际 %d", expectedSize, size)
	}
	logger.Printf("  ✓ 追加后文件大小: %d bytes (正确)\n", size)

	c.PrintfLine("DELE appe_test.txt")
	c.ReadResponse(250)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

type infiniteReader struct {
	data   []byte
	offset int
}

func newInfiniteReader(data []byte) *infiniteReader {
	return &infiniteReader{data: data}
}

func (r *infiniteReader) Read(p []byte) (int, error) {
	n := copy(p, r.data[r.offset:])
	r.offset += n
	if r.offset >= len(r.data) {
		r.offset = 0
	}
	return n, nil
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
		return fmt.Errorf("发送 MODE 命令失败: %w", err)
	}
	code, msg, err := c.ReadResponse(200)
	if err != nil {
		logger.Printf("  ⚠ MODE S 不支持: %d %s\n", code, strings.TrimSpace(msg))
	} else {
		logger.Printf("  ✓ MODE S: %s\n", strings.TrimSpace(msg))
	}

	err = c.PrintfLine("STRU F")
	if err != nil {
		return fmt.Errorf("发送 STRU 命令失败: %w", err)
	}
	code, msg, err = c.ReadResponse(200)
	if err != nil {
		logger.Printf("  ⚠ STRU F 不支持: %d %s\n", code, strings.TrimSpace(msg))
	} else {
		logger.Printf("  ✓ STRU F: %s\n", strings.TrimSpace(msg))
	}

	err = c.PrintfLine("STRU R")
	if err != nil {
		return fmt.Errorf("发送 STRU R 命令失败: %w", err)
	}
	code, _, _ = c.ReadResponse(504)
	if code == 504 {
		logger.Printf("  ✓ STRU R 正确拒绝 (504)\n")
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}
