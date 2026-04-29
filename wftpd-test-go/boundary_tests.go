package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

func testEmptyFileTransfer() error {
	startTime := time.Now()
	logger.Printf("  [边界] 测试空文件传输...\n")

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

	emptyFile := filepath.Join(config.TestDataDir, "empty.txt")
	err = os.WriteFile(emptyFile, []byte{}, 0644)
	if err != nil {
		return fmt.Errorf("创建空文件失败: %w", err)
	}
	defer os.Remove(emptyFile)

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	file, err := os.Open(emptyFile)
	if err != nil {
		return fmt.Errorf("打开空文件失败: %w", err)
	}
	defer file.Close()

	err = dt.Upload(file, "empty_test.txt")
	if err != nil {
		return fmt.Errorf("上传空文件失败: %w", err)
	}
	logger.Printf("  ✓ 上传空文件成功\n")

	dt2, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt2.Close()

	downloadFile := filepath.Join(config.TestDataDir, "empty_downloaded.txt")
	outFile, err := os.Create(downloadFile)
	if err != nil {
		return fmt.Errorf("创建下载文件失败: %w", err)
	}
	defer outFile.Close()
	defer os.Remove(downloadFile)

	err = dt2.Download(outFile, "empty_test.txt")
	if err != nil {
		return fmt.Errorf("下载空文件失败: %w", err)
	}

	stat, err := outFile.Stat()
	if err != nil {
		return fmt.Errorf("获取下载文件信息失败: %w", err)
	}
	if stat.Size() != 0 {
		return fmt.Errorf("空文件大小不匹配: 期望 0, 实际 %d", stat.Size())
	}
	logger.Printf("  ✓ 下载空文件成功，大小验证通过\n")

	c.PrintfLine("DELE empty_test.txt")
	c.ReadResponse(250)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSpecialCharacterFilename() error {
	startTime := time.Now()
	logger.Printf("  [边界] 测试特殊字符文件名...\n")

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

	specialFilenames := []string{
		"file with spaces.txt",
		"file'with'quotes.txt",
		"file\"with\"double.txt",
		"file(with)parens.txt",
		"file[with]brackets.txt",
		"file{with}braces.txt",
		"file&with&ampersand.txt",
		"file#with#hash.txt",
		"file@with@at.txt",
		"file!with!exclaim.txt",
	}

	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开源文件失败: %w", err)
	}
	defer file.Close()

	successCount := 0
	for _, filename := range specialFilenames {
		dt, err := pasvDataConnect(c)
		if err != nil {
			logger.Printf("  ⚠ 文件名 '%s' 测试失败: %v\n", filename, err)
			continue
		}

		file.Seek(0, 0)
		err = dt.Upload(file, filename)
		dt.Close()

		if err != nil {
			logger.Printf("  ⚠ 文件名 '%s' 上传失败: %v\n", filename, err)
			continue
		}

		c.PrintfLine("DELE %s", filename)
		c.ReadResponse(250)
		successCount++
	}

	logger.Printf("  ✓ 特殊字符文件名测试: %d/%d 成功\n", successCount, len(specialFilenames))
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testLongFilename() error {
	startTime := time.Now()
	logger.Printf("  [边界] 测试超长文件名...\n")

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

	longFilename := strings.Repeat("a", 200) + ".txt"
	if len(longFilename) > 255 {
		longFilename = longFilename[:255]
	}

	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开源文件失败: %w", err)
	}
	defer file.Close()

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	err = dt.Upload(file, longFilename)
	if err != nil {
		logger.Printf("  ⚠ 超长文件名上传失败: %v\n", err)
		logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
		return nil
	}

	logger.Printf("  ✓ 超长文件名上传成功 (长度: %d)\n", len(longFilename))

	c.PrintfLine("DELE %s", longFilename)
	c.ReadResponse(250)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testLargeFileTransfer() error {
	startTime := time.Now()
	logger.Printf("  [边界] 测试超大文件传输 (100MB)...\n")

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

	largeFile := filepath.Join(config.TestDataDir, "large_100mb.bin")
	if _, err := os.Stat(largeFile); os.IsNotExist(err) {
		logger.Printf("  ⚠ large_100mb.bin 不存在，跳过超大文件测试\n")
		return nil
	}

	uploadStart := time.Now()
	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}

	file, err := os.Open(largeFile)
	if err != nil {
		return fmt.Errorf("打开大文件失败: %w", err)
	}
	defer file.Close()

	err = dt.Upload(file, "large_100mb_test.bin")
	if err != nil {
		return fmt.Errorf("上传大文件失败: %w", err)
	}
	uploadDuration := time.Since(uploadStart)

	fi, _ := file.Stat()
	uploadThroughput := float64(fi.Size()) / uploadDuration.Seconds() / 1024 / 1024
	logger.Printf("  ✓ 上传: %.2f MB (%.2f MB/s)\n", float64(fi.Size())/1024/1024, uploadThroughput)

	downloadStart := time.Now()
	dt2, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt2.Close()

	downloadFile := filepath.Join(config.TestDataDir, "large_downloaded.bin")
	outFile, err := os.Create(downloadFile)
	if err != nil {
		return fmt.Errorf("创建下载文件失败: %w", err)
	}
	defer outFile.Close()
	defer os.Remove(downloadFile)

	err = dt2.Download(outFile, "large_100mb_test.bin")
	if err != nil {
		return fmt.Errorf("下载大文件失败: %w", err)
	}
	downloadDuration := time.Since(downloadStart)

	downloadThroughput := float64(fi.Size()) / downloadDuration.Seconds() / 1024 / 1024
	logger.Printf("  ✓ 下载: %.2f MB (%.2f MB/s)\n", float64(fi.Size())/1024/1024, downloadThroughput)

	originalMD5, err := calculateMD5(largeFile)
	if err != nil {
		return fmt.Errorf("计算原始文件 MD5 失败: %w", err)
	}

	downloadedMD5, err := calculateMD5(downloadFile)
	if err != nil {
		return fmt.Errorf("计算下载文件 MD5 失败: %w", err)
	}

	if originalMD5 != downloadedMD5 {
		return fmt.Errorf("数据完整性验证失败: MD5 不匹配")
	}
	logger.Printf("  ✓ 数据完整性验证通过\n")

	c.PrintfLine("DELE large_100mb_test.bin")
	c.ReadResponse(250)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testPathTraversalProtection() error {
	startTime := time.Now()
	logger.Printf("  [边界] 测试路径遍历防护...\n")

	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()

	maliciousPaths := []string{
		"../../../etc/passwd",
		"..\\..\\..\\windows\\system32\\config\\sam",
		"....//....//....//etc/passwd",
		"..%2F..%2F..%2Fetc%2Fpasswd",
		"..%5c..%5c..%5cwindows%5csystem32",
	}

	blockedCount := 0
	for _, path := range maliciousPaths {
		err = c.PrintfLine("RETR %s", path)
		if err != nil {
			logger.Printf("  ✓ 路径遍历攻击被阻止: %s\n", path)
			blockedCount++
			continue
		}

		code, _, _ := c.ReadResponse(0)
		if code >= 400 && code < 600 {
			logger.Printf("  ✓ 路径遍历攻击被阻止: %s (响应码: %d)\n", path, code)
			blockedCount++
		} else {
			logger.Printf("  ⚠ 路径遍历攻击未被阻止: %s (响应码: %d)\n", path, code)
		}
	}

	logger.Printf("  ✓ 路径遍历防护测试: %d/%d 被阻止\n", blockedCount, len(maliciousPaths))
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testBinaryFileTransfer() error {
	startTime := time.Now()
	logger.Printf("  [边界] 测试二进制文件传输...\n")

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

	binaryFile := filepath.Join(config.TestDataDir, "binary_test.bin")
	binaryData := make([]byte, 1024)
	for i := range binaryData {
		binaryData[i] = byte(i % 256)
	}
	err = os.WriteFile(binaryFile, binaryData, 0644)
	if err != nil {
		return fmt.Errorf("创建二进制文件失败: %w", err)
	}
	defer os.Remove(binaryFile)

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt.Close()

	file, err := os.Open(binaryFile)
	if err != nil {
		return fmt.Errorf("打开二进制文件失败: %w", err)
	}
	defer file.Close()

	err = dt.Upload(file, "binary_test.bin")
	if err != nil {
		return fmt.Errorf("上传二进制文件失败: %w", err)
	}
	logger.Printf("  ✓ 上传二进制文件成功\n")

	dt2, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt2.Close()

	downloadFile := filepath.Join(config.TestDataDir, "binary_downloaded.bin")
	outFile, err := os.Create(downloadFile)
	if err != nil {
		return fmt.Errorf("创建下载文件失败: %w", err)
	}
	defer outFile.Close()
	defer os.Remove(downloadFile)

	err = dt2.Download(outFile, "binary_test.bin")
	if err != nil {
		return fmt.Errorf("下载二进制文件失败: %w", err)
	}

	originalMD5, err := calculateMD5(binaryFile)
	if err != nil {
		return fmt.Errorf("计算原始文件 MD5 失败: %w", err)
	}

	downloadedMD5, err := calculateMD5(downloadFile)
	if err != nil {
		return fmt.Errorf("计算下载文件 MD5 失败: %w", err)
	}

	if originalMD5 != downloadedMD5 {
		return fmt.Errorf("二进制文件完整性验证失败: MD5 不匹配")
	}
	logger.Printf("  ✓ 二进制文件完整性验证通过\n")

	c.PrintfLine("DELE binary_test.bin")
	c.ReadResponse(250)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testUnicodeFilename() error {
	startTime := time.Now()
	logger.Printf("  [边界] 测试Unicode文件名...\n")

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

	unicodeFilenames := []string{
		"中文文件名.txt",
		"日本語ファイル.txt",
		"한국어파일.txt",
		"Ελληνικά.txt",
		"العربية.txt",
		"עברית.txt",
		"ไทย.txt",
		"emoji_😀_test.txt",
	}

	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开源文件失败: %w", err)
	}
	defer file.Close()

	successCount := 0
	for _, filename := range unicodeFilenames {
		dt, err := pasvDataConnect(c)
		if err != nil {
			logger.Printf("  ⚠ Unicode文件名 '%s' 测试失败: %v\n", filename, err)
			continue
		}

		file.Seek(0, 0)
		err = dt.Upload(file, filename)
		dt.Close()

		if err != nil {
			logger.Printf("  ⚠ Unicode文件名 '%s' 上传失败: %v\n", filename, err)
			continue
		}

		c.PrintfLine("DELE %s", filename)
		c.ReadResponse(250)
		successCount++
	}

	logger.Printf("  ✓ Unicode文件名测试: %d/%d 成功\n", successCount, len(unicodeFilenames))
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}
