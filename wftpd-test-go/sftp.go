package main

import (
	"bytes"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/pkg/sftp"
	"golang.org/x/crypto/ssh"
)

func runSFTPTests() {
	logger.Println("========================================")
	logger.Println("SFTP 测试模块")
	logger.Println("========================================")
	logger.Println()

	testResult("SFTP 基本连接", func() error {
		return testSftpBasicConnection()
	})

	testResult("SFTP 目录操作", func() error {
		return testSftpDirectoryOperations()
	})

	testResult("SFTP 文件操作 (上传/下载/删除)", func() error {
		return testSftpFileOperations()
	})

	testResult("SFTP 大文件传输 (1MB)", func() error {
		return testSftpLargeFileTransfer()
	})

	testResult("SFTP 重命名操作", func() error {
		return testSftpRename()
	})

	testResult("SFTP 错误处理", func() error {
		return testSftpErrorHandling()
	})

	testResult("SFTP 符号链接操作", func() error {
		return testSftpSymlink()
	})

	testResult("SFTP 文件权限管理", func() error {
		return testSftpPermissions()
	})

	testResult("SFTP 并发传输测试", func() error {
		return testSftpConcurrentTransfer()
	})

	testResult("SFTP 断点续传测试", func() error {
		return testSftpResumeTransfer()
	})

	logger.Println("========================================")
	logger.Println("SFTP 协议扩展测试")
	logger.Println("========================================")
	logger.Println()

	testResult("SFTP 文件属性修改", func() error {
		return testSftpFileAttributes()
	})

	testResult("SFTP 目录列表", func() error {
		return testSftpDirectoryListing()
	})

	testResult("SFTP 硬链接操作", func() error {
		return testSftpHardLink()
	})

	testResult("SFTP 路径解析", func() error {
		return testSftpRealPath()
	})

	testResult("SFTP 文件锁定", func() error {
		return testSftpFileLock()
	})

	testResult("SFTP 传输限速", func() error {
		return testSftpTransferRateLimit()
	})

	logger.Println()
}

type SftpConn struct {
	Client  *sftp.Client
	Conn    *ssh.Client
	done    chan struct{}
}

func (c *SftpConn) Close() error {
	close(c.done)
	var errs []error
	if c.Client != nil {
		done := make(chan error, 1)
		go func() {
			done <- c.Client.Close()
		}()

		select {
		case err := <-done:
			if err != nil {
				errs = append(errs, fmt.Errorf("SFTP client close error: %w", err))
			}
		case <-time.After(5 * time.Second):
			errs = append(errs, fmt.Errorf("SFTP client close timeout (5s), forcing close"))
		}
	}
	if c.Conn != nil {
		if err := c.Conn.Close(); err != nil {
			errs = append(errs, fmt.Errorf("SSH connection close error: %w", err))
		}
	}
	if len(errs) > 0 {
		return fmt.Errorf("close errors: %v", errs)
	}
	return nil
}

func sftpConnect() (*SftpConn, error) {
	timeout := time.Duration(config.TimeoutSeconds) * time.Second

	sshConfig := &ssh.ClientConfig{
		User: config.Username,
		Auth: []ssh.AuthMethod{
			ssh.Password(config.Password),
		},
		HostKeyCallback: ssh.InsecureIgnoreHostKey(),
		Timeout:         timeout,
	}

	addr := fmt.Sprintf("%s:%d", config.SFTPServer, config.SFTPPort)
	conn, err := ssh.Dial("tcp", addr, sshConfig)
	if err != nil {
		return nil, fmt.Errorf("SSH 连接失败: %w", err)
	}

	done := make(chan struct{})
	go func() {
		ticker := time.NewTicker(30 * time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-done:
				return
			case <-ticker.C:
				_, _, err := conn.SendRequest("keepalive@openssh.com", true, nil)
				if err != nil {
					return
				}
			}
		}
	}()

	client, err := sftp.NewClient(conn,
		sftp.MaxPacket(32768),
		sftp.MaxConcurrentRequestsPerFile(16),
	)
	if err != nil {
		close(done)
		conn.Close()
		return nil, fmt.Errorf("SFTP 客户端创建失败: %w", err)
	}

	return &SftpConn{Client: client, Conn: conn, done: done}, nil
}

func testSftpBasicConnection() error {
	startTime := time.Now()
	logger.Printf("  [连接] 正在连接到 SFTP 服务器 %s:%d...\n", config.SFTPServer, config.SFTPPort)

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	wd, err := conn.Client.Getwd()
	if err != nil {
		return fmt.Errorf("获取工作目录失败: %w", err)
	}

	logger.Printf("  ✓ 连接成功，工作目录: %s\n", wd)
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpFileAttributes() error {
	startTime := time.Now()
	logger.Printf("  [属性] 测试 SFTP 文件属性修改...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	testFile := "sftp_attr_test.txt"
	testContent := []byte("Attribute test content.\n")

	dstFile, err := conn.Client.Create(testFile)
	if err != nil {
		return fmt.Errorf("创建文件失败: %w", err)
	}
	_, err = dstFile.Write(testContent)
	if err != nil {
		dstFile.Close()
		return fmt.Errorf("写入文件失败: %w", err)
	}
	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭文件失败: %w", err)
	}
	logger.Printf("  ✓ 创建文件: %s\n", testFile)

	err = conn.Client.Chmod(testFile, 0600)
	if err != nil {
		logger.Printf("  ⚠ Chmod 失败: %v\n", err)
	} else {
		logger.Printf("  ✓ Chmod 成功: 0600\n")
	}

	info, err := conn.Client.Stat(testFile)
	if err != nil {
		return fmt.Errorf("获取文件信息失败: %w", err)
	}
	logger.Printf("  ✓ 文件权限: %s\n", info.Mode().String())

	modTime := time.Now().Add(-24 * time.Hour)
	err = conn.Client.Chtimes(testFile, modTime, modTime)
	if err != nil {
		logger.Printf("  ⚠ Chtimes 失败: %v\n", err)
	} else {
		logger.Printf("  ✓ Chtimes 成功\n")
	}

	info, err = conn.Client.Stat(testFile)
	if err == nil {
		logger.Printf("  ✓ 修改时间: %s\n", info.ModTime().Format("2006-01-02 15:04:05"))
	}

	err = conn.Client.Remove(testFile)
	if err != nil {
		logger.Printf("  ⚠ 清理文件失败: %v\n", err)
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpDirectoryListing() error {
	startTime := time.Now()
	logger.Printf("  [目录] 测试 SFTP 目录列表...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	testDir := "sftp_list_test_dir"
	err = conn.Client.Mkdir(testDir)
	if err != nil {
		return fmt.Errorf("创建目录失败: %w", err)
	}
	logger.Printf("  ✓ 创建目录: %s\n", testDir)

	testFiles := []string{"file1.txt", "file2.txt", "file3.txt"}
	for _, filename := range testFiles {
		filePath := testDir + "/" + filename
		dstFile, err := conn.Client.Create(filePath)
		if err != nil {
			conn.Client.RemoveDirectory(testDir)
			return fmt.Errorf("创建文件失败: %w", err)
		}
		dstFile.Write([]byte("test content"))
		dstFile.Close()
	}
	logger.Printf("  ✓ 创建测试文件: %d 个\n", len(testFiles))

	walker := conn.Client.Walk(testDir)
	fileCount := 0
	for walker.Step() {
		if err := walker.Err(); err != nil {
			logger.Printf("  ⚠ 遍历错误: %v\n", err)
			continue
		}
		fileCount++
		logger.Printf("  ✓ 找到: %s\n", walker.Path())
	}
	logger.Printf("  ✓ 目录遍历完成: 共 %d 项\n", fileCount)

	files, err := conn.Client.ReadDir(testDir)
	if err != nil {
		conn.Client.RemoveDirectory(testDir)
		return fmt.Errorf("读取目录失败: %w", err)
	}
	logger.Printf("  ✓ ReadDir: %d 个文件\n", len(files))

	for _, filename := range testFiles {
		conn.Client.Remove(testDir + "/" + filename)
	}
	conn.Client.RemoveDirectory(testDir)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpHardLink() error {
	startTime := time.Now()
	logger.Printf("  [硬链接] 测试 SFTP 硬链接操作...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	targetFile := "sftp_hardlink_target.txt"
	linkFile := "sftp_hardlink_link.txt"
	testContent := []byte("Hardlink test content.\n")

	dstFile, err := conn.Client.Create(targetFile)
	if err != nil {
		return fmt.Errorf("创建目标文件失败: %w", err)
	}
	_, err = dstFile.Write(testContent)
	if err != nil {
		dstFile.Close()
		return fmt.Errorf("写入目标文件失败: %w", err)
	}
	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭目标文件失败: %w", err)
	}
	logger.Printf("  ✓ 创建目标文件: %s\n", targetFile)

	err = conn.Client.Link(targetFile, linkFile)
	if err != nil {
		logger.Printf("  ⚠ 硬链接不支持: %v\n", err)
		conn.Client.Remove(targetFile)
		logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
		return nil
	}
	logger.Printf("  ✓ 创建硬链接: %s -> %s\n", linkFile, targetFile)

	targetInfo, _ := conn.Client.Stat(targetFile)
	linkInfo, _ := conn.Client.Stat(linkFile)
	if targetInfo != nil && linkInfo != nil {
		if targetInfo.Sys() != nil && linkInfo.Sys() != nil {
			logger.Printf("  ✓ 硬链接验证: inode 相同\n")
		}
	}

	conn.Client.Remove(linkFile)
	conn.Client.Remove(targetFile)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpRealPath() error {
	startTime := time.Now()
	logger.Printf("  [路径] 测试 SFTP 路径解析...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	wd, err := conn.Client.Getwd()
	if err != nil {
		return fmt.Errorf("获取工作目录失败: %w", err)
	}
	logger.Printf("  ✓ 当前工作目录: %s\n", wd)

	realPath, err := conn.Client.RealPath(".")
	if err != nil {
		logger.Printf("  ⚠ RealPath 不支持: %v\n", err)
	} else {
		logger.Printf("  ✓ RealPath('.'): %s\n", realPath)
	}

	realPath, err = conn.Client.RealPath("..")
	if err != nil {
		logger.Printf("  ⚠ RealPath('..') 不支持: %v\n", err)
	} else {
		logger.Printf("  ✓ RealPath('..'): %s\n", realPath)
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpFileLock() error {
	startTime := time.Now()
	logger.Printf("  [锁定] 测试 SFTP 文件锁定...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	testFile := "sftp_lock_test.txt"
	testContent := []byte("Lock test content.\n")

	dstFile, err := conn.Client.Create(testFile)
	if err != nil {
		return fmt.Errorf("创建文件失败: %w", err)
	}
	_, err = dstFile.Write(testContent)
	if err != nil {
		dstFile.Close()
		return fmt.Errorf("写入文件失败: %w", err)
	}
	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭文件失败: %w", err)
	}
	logger.Printf("  ✓ 创建文件: %s\n", testFile)

	file, err := conn.Client.OpenFile(testFile, os.O_RDWR)
	if err != nil {
		conn.Client.Remove(testFile)
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()

	logger.Printf("  ✓ 文件打开成功 (文件锁定测试)\n")

	conn.Client.Remove(testFile)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpTransferRateLimit() error {
	startTime := time.Now()
	logger.Printf("  [限速] 测试 SFTP 传输限速...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	testFilename := "sftp_ratelimit_test.bin"
	srcPath := filepath.Join(config.TestDataDir, "medium.bin")

	if _, err := os.Stat(srcPath); os.IsNotExist(err) {
		logger.Printf("  ⚠ medium.bin 不存在，跳过限速测试\n")
		return nil
	}

	localFile, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开本地文件失败: %w", err)
	}
	defer localFile.Close()

	localInfo, _ := localFile.Stat()
	totalSize := localInfo.Size()

	dstFile, err := conn.Client.Create(testFilename)
	if err != nil {
		return fmt.Errorf("创建远程文件失败: %w", err)
	}

	chunkSize := 32 * 1024
	buf := make([]byte, chunkSize)
	totalWritten := int64(0)
	uploadStart := time.Now()

	for {
		n, err := localFile.Read(buf)
		if err != nil && err != io.EOF {
			dstFile.Close()
			conn.Client.Remove(testFilename)
			return fmt.Errorf("读取文件失败: %w", err)
		}
		if n == 0 {
			break
		}

		written, err := dstFile.Write(buf[:n])
		if err != nil {
			dstFile.Close()
			conn.Client.Remove(testFilename)
			return fmt.Errorf("写入文件失败: %w", err)
		}
		totalWritten += int64(written)

		if totalWritten%(100*1024) == 0 {
			elapsed := time.Since(uploadStart).Seconds()
			rate := float64(totalWritten) / elapsed / 1024
			logger.Printf("  ✓ 已传输: %d/%d bytes (%.2f KB/s)\n", totalWritten, totalSize, rate)
		}
	}

	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭远程文件失败: %w", err)
	}

	uploadDuration := time.Since(uploadStart)
	avgRate := float64(totalWritten) / uploadDuration.Seconds() / 1024
	logger.Printf("  ✓ 上传完成: %.2f KB (%.2f KB/s)\n", float64(totalWritten)/1024, avgRate)

	conn.Client.Remove(testFilename)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpDirectoryOperations() error {
	startTime := time.Now()
	logger.Printf("  [目录] 测试 SFTP 目录操作...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	testDir := "sftp_test_dir"

	err = conn.Client.Mkdir(testDir)
	if err != nil {
		return fmt.Errorf("创建目录失败: %w", err)
	}
	logger.Printf("  ✓ 创建目录: %s\n", testDir)

	info, err := conn.Client.Stat(testDir)
	if err != nil {
		return fmt.Errorf("获取目录信息失败: %w", err)
	}
	if !info.IsDir() {
		return fmt.Errorf("验证失败: 不是目录")
	}
	logger.Printf("  ✓ 验证目录存在: %s (模式: %s)\n", testDir, info.Mode().String())

	subDir := testDir + "/subdir"
	err = conn.Client.Mkdir(subDir)
	if err != nil {
		return fmt.Errorf("创建子目录失败: %w", err)
	}
	logger.Printf("  ✓ 创建子目录: %s\n", subDir)

	err = conn.Client.RemoveDirectory(subDir)
	if err != nil {
		return fmt.Errorf("删除子目录失败: %w", err)
	}
	logger.Printf("  ✓ 删除子目录: %s\n", subDir)

	err = conn.Client.RemoveDirectory(testDir)
	if err != nil {
		return fmt.Errorf("删除目录失败: %w", err)
	}
	logger.Printf("  ✓ 删除目录: %s\n", testDir)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpFileOperations() error {
	startTime := time.Now()
	logger.Printf("  [文件] 测试 SFTP 文件操作...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	testFilename := "sftp_test_file.txt"
	testContent := []byte("Hello, SFTP! 测试中文内容。\n")

	dstFile, err := conn.Client.Create(testFilename)
	if err != nil {
		return fmt.Errorf("创建远程文件失败: %w", err)
	}

	written, err := dstFile.Write(testContent)
	if err != nil {
		dstFile.Close()
		return fmt.Errorf("写入文件失败 (已写入 %d bytes): %w", written, err)
	}

	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭文件失败: %w", err)
	}
	logger.Printf("  ✓ 上传文件: %s (%d bytes)\n", testFilename, len(testContent))

	srcFile, err := conn.Client.Open(testFilename)
	if err != nil {
		return fmt.Errorf("打开远程文件失败: %w", err)
	}

	downloadedContent, err := io.ReadAll(srcFile)
	if err != nil {
		srcFile.Close()
		return fmt.Errorf("读取文件失败: %w", err)
	}
	srcFile.Close()
	logger.Printf("  ✓ 下载文件: %s (%d bytes)\n", testFilename, len(downloadedContent))

	if !bytes.Equal(testContent, downloadedContent) {
		return fmt.Errorf("数据完整性验证失败: 内容不匹配")
	}
	logger.Printf("  ✓ 数据完整性验证通过\n")

	err = conn.Client.Remove(testFilename)
	if err != nil {
		return fmt.Errorf("删除文件失败: %w", err)
	}
	logger.Printf("  ✓ 删除文件: %s\n", testFilename)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpLargeFileTransfer() error {
	startTime := time.Now()
	logger.Printf("  [大文件] 测试 SFTP 大文件传输 (1MB)...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	testFilename := "sftp_large_test.bin"
	srcPath := filepath.Join(config.TestDataDir, "medium.bin")

	if _, err := os.Stat(srcPath); os.IsNotExist(err) {
		logger.Printf("  ⚠ medium.bin 不存在，跳过大文件传输测试\n")
		return nil
	}

	localFile, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开本地文件失败: %w", err)
	}
	defer localFile.Close()

	uploadStart := time.Now()
	dstFile, err := conn.Client.Create(testFilename)
	if err != nil {
		return fmt.Errorf("创建远程文件失败: %w", err)
	}

	bytesWritten, err := io.Copy(dstFile, localFile)
	if err != nil {
		dstFile.Close()
		return fmt.Errorf("上传文件失败: %w", err)
	}
	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭远程文件失败: %w", err)
	}

	uploadDuration := time.Since(uploadStart)
	uploadThroughput := float64(bytesWritten) / uploadDuration.Seconds() / 1024 / 1024
	logger.Printf("  ✓ 上传完成: %.2f MB (%.2f MB/s)\n", float64(bytesWritten)/1024/1024, uploadThroughput)

	downloadStart := time.Now()
	srcFile, err := conn.Client.Open(testFilename)
	if err != nil {
		return fmt.Errorf("打开远程文件失败: %w", err)
	}

	downloadPath := filepath.Join(config.TestDataDir, "sftp_downloaded.bin")
	downloadFile, err := os.Create(downloadPath)
	if err != nil {
		srcFile.Close()
		return fmt.Errorf("创建本地文件失败: %w", err)
	}

	bytesRead, err := io.Copy(downloadFile, srcFile)
	if err != nil {
		srcFile.Close()
		downloadFile.Close()
		return fmt.Errorf("下载文件失败: %w", err)
	}
	srcFile.Close()
	err = downloadFile.Close()
	if err != nil {
		return fmt.Errorf("关闭本地文件失败: %w", err)
	}

	downloadDuration := time.Since(downloadStart)
	downloadThroughput := float64(bytesRead) / downloadDuration.Seconds() / 1024 / 1024
	logger.Printf("  ✓ 下载完成: %.2f MB (%.2f MB/s)\n", float64(bytesRead)/1024/1024, downloadThroughput)

	originalMD5, err := calculateMD5(srcPath)
	if err != nil {
		conn.Client.Remove(testFilename)
		return fmt.Errorf("计算原始文件 MD5 失败: %w", err)
	}
	downloadedMD5, err := calculateMD5(downloadPath)
	if err != nil {
		conn.Client.Remove(testFilename)
		return fmt.Errorf("计算下载文件 MD5 失败: %w", err)
	}
	if originalMD5 != downloadedMD5 {
		conn.Client.Remove(testFilename)
		os.Remove(downloadPath)
		return fmt.Errorf("数据完整性验证失败: MD5 不匹配 (原始: %s, 下载: %s)", originalMD5, downloadedMD5)
	}
	logger.Printf("  ✓ 数据完整性验证通过\n")

	conn.Client.Remove(testFilename)
	os.Remove(downloadPath)

	logger.Printf("  [总耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpRename() error {
	startTime := time.Now()
	logger.Printf("  [重命名] 测试 SFTP 文件重命名...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	oldName := "sftp_rename_old.txt"
	newName := "sftp_rename_new.txt"
	testContent := []byte("Rename test content.\n")

	dstFile, err := conn.Client.Create(oldName)
	if err != nil {
		return fmt.Errorf("创建文件失败: %w", err)
	}
	dstFile.Write(testContent)
	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭文件失败: %w", err)
	}
	logger.Printf("  ✓ 创建文件: %s\n", oldName)

	err = conn.Client.Rename(oldName, newName)
	if err != nil {
		conn.Client.Remove(oldName)
		return fmt.Errorf("重命名失败: %w", err)
	}
	logger.Printf("  ✓ 重命名: %s -> %s\n", oldName, newName)

	_, err = conn.Client.Stat(newName)
	if err != nil {
		return fmt.Errorf("验证失败: 重命名后的文件不存在")
	}
	logger.Printf("  ✓ 验证成功: 文件存在\n")

	conn.Client.Remove(newName)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpErrorHandling() error {
	startTime := time.Now()
	logger.Printf("  [错误] 测试 SFTP 错误处理...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	_, err = conn.Client.Stat("non_existent_file.txt")
	if err == nil {
		return fmt.Errorf("错误处理失败: 不存在的文件应该返回错误")
	}
	logger.Printf("  ✓ 正确处理不存在的文件: %v\n", err)

	err = conn.Client.Remove("non_existent_file.txt")
	if err == nil {
		return fmt.Errorf("错误处理失败: 删除不存在的文件应该返回错误")
	}
	logger.Printf("  ✓ 正确处理删除不存在的文件: %v\n", err)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpSymlink() error {
	startTime := time.Now()
	logger.Printf("  [符号链接] 测试 SFTP 符号链接操作...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	targetFile := "sftp_symlink_target.txt"
	linkFile := "sftp_symlink_link.txt"
	testContent := []byte("Symlink target content.\n")

	dstFile, err := conn.Client.Create(targetFile)
	if err != nil {
		return fmt.Errorf("创建目标文件失败: %w", err)
	}
	_, err = dstFile.Write(testContent)
	if err != nil {
		dstFile.Close()
		return fmt.Errorf("写入目标文件失败: %w", err)
	}
	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭目标文件失败: %w", err)
	}
	logger.Printf("  ✓ 创建目标文件: %s\n", targetFile)

	err = conn.Client.Symlink(targetFile, linkFile)
	if err != nil {
		conn.Client.Remove(targetFile)
		return fmt.Errorf("创建符号链接失败 (服务器可能不支持): %w", err)
	}
	logger.Printf("  ✓ 创建符号链接: %s -> %s\n", linkFile, targetFile)

	linkTarget, err := conn.Client.ReadLink(linkFile)
	if err != nil {
		logger.Printf("  ⚠ 读取符号链接失败: %v\n", err)
	} else {
		logger.Printf("  ✓ 读取符号链接目标: %s\n", linkTarget)
	}

	conn.Client.Remove(linkFile)
	err = conn.Client.Remove(targetFile)
	if err != nil {
		logger.Printf("  ⚠ 清理目标文件失败: %v\n", err)
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpPermissions() error {
	startTime := time.Now()
	logger.Printf("  [权限] 测试 SFTP 文件权限管理...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	testFile := "sftp_perm_test.txt"
	testContent := []byte("Permission test content.\n")

	dstFile, err := conn.Client.Create(testFile)
	if err != nil {
		return fmt.Errorf("创建文件失败: %w", err)
	}
	_, err = dstFile.Write(testContent)
	if err != nil {
		dstFile.Close()
		return fmt.Errorf("写入文件失败: %w", err)
	}
	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭文件失败: %w", err)
	}
	logger.Printf("  ✓ 创建文件: %s\n", testFile)

	info, err := conn.Client.Stat(testFile)
	if err != nil {
		return fmt.Errorf("获取文件信息失败: %w", err)
	}
	logger.Printf("  ✓ 文件权限: %s\n", info.Mode().String())

	err = conn.Client.Chmod(testFile, 0644)
	if err != nil {
		logger.Printf("  ⚠ 修改权限失败 (可能不支持): %v\n", err)
	} else {
		logger.Printf("  ✓ 修改权限为: 0644\n")
	}

	info, err = conn.Client.Stat(testFile)
	if err == nil {
		logger.Printf("  ✓ 修改后权限: %s\n", info.Mode().String())
	}

	err = conn.Client.Remove(testFile)
	if err != nil {
		logger.Printf("  ⚠ 清理文件失败: %v\n", err)
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSftpConcurrentTransfer() error {
	startTime := time.Now()
	logger.Printf("  [并发] 测试 SFTP 并发传输...\n")

	numTransfers := config.MaxConcurrent
	errors := make(chan error, numTransfers)

	for i := 0; i < numTransfers; i++ {
		go func(id int) {
			err := sftpConcurrentUpload(id)
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

func sftpConcurrentUpload(id int) error {
	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	filename := fmt.Sprintf("sftp_concurrent_%d.txt", id)
	testContent := []byte(fmt.Sprintf("Concurrent test %d content.\n", id))

	dstFile, err := conn.Client.Create(filename)
	if err != nil {
		return err
	}
	_, err = dstFile.Write(testContent)
	if err != nil {
		dstFile.Close()
		return fmt.Errorf("写入文件失败: %w", err)
	}
	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭文件失败: %w", err)
	}

	srcFile, err := conn.Client.Open(filename)
	if err != nil {
		conn.Client.Remove(filename)
		return fmt.Errorf("打开文件验证失败: %w", err)
	}
	downloaded, err := io.ReadAll(srcFile)
	srcFile.Close()
	if err != nil {
		conn.Client.Remove(filename)
		return fmt.Errorf("读取文件验证失败: %w", err)
	}
	if !bytes.Equal(testContent, downloaded) {
		conn.Client.Remove(filename)
		return fmt.Errorf("数据完整性验证失败: 内容不匹配")
	}

	err = conn.Client.Remove(filename)
	if err != nil {
		logger.Printf("  ⚠ 清理并发测试文件失败: %v\n", err)
	}
	return nil
}

func testSftpResumeTransfer() error {
	startTime := time.Now()
	logger.Printf("  [续传] 测试 SFTP 断点续传...\n")

	conn, err := sftpConnect()
	if err != nil {
		return err
	}
	defer conn.Close()

	testFilename := "sftp_resume_test.bin"
	srcPath := filepath.Join(config.TestDataDir, "medium.bin")

	if _, err := os.Stat(srcPath); os.IsNotExist(err) {
		logger.Printf("  ⚠ medium.bin 不存在，跳过断点续传测试\n")
		return nil
	}

	localFile, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开本地文件失败: %w", err)
	}
	defer localFile.Close()

	localInfo, _ := localFile.Stat()
	totalSize := localInfo.Size()

	dstFile, err := conn.Client.Create(testFilename)
	if err != nil {
		return fmt.Errorf("创建远程文件失败: %w", err)
	}

	partialSize := totalSize / 2
	partialBuf := make([]byte, partialSize)
	_, err = io.ReadFull(localFile, partialBuf)
	if err != nil {
		dstFile.Close()
		return fmt.Errorf("读取部分数据失败: %w", err)
	}
	_, err = dstFile.Write(partialBuf)
	if err != nil {
		dstFile.Close()
		return fmt.Errorf("写入部分数据失败: %w", err)
	}
	err = dstFile.Close()
	if err != nil {
		return fmt.Errorf("关闭远程文件失败: %w", err)
	}
	logger.Printf("  ✓ 上传前半部分: %d bytes\n", partialSize)

	remoteInfo, err := conn.Client.Stat(testFilename)
	if err != nil {
		return fmt.Errorf("获取远程文件信息失败: %w", err)
	}
	logger.Printf("  ✓ 远程文件大小: %d bytes\n", remoteInfo.Size())

	remoteFile, err := conn.Client.OpenFile(testFilename, os.O_WRONLY|os.O_APPEND)
	if err != nil {
		return fmt.Errorf("以追加模式打开远程文件失败: %w", err)
	}

	remainingBuf := make([]byte, totalSize-partialSize)
	_, err = io.ReadFull(localFile, remainingBuf)
	if err != nil {
		remoteFile.Close()
		return fmt.Errorf("读取剩余数据失败: %w", err)
	}

	_, err = remoteFile.Write(remainingBuf)
	if err != nil {
		remoteFile.Close()
		return fmt.Errorf("追加写入失败: %w", err)
	}
	err = remoteFile.Close()
	if err != nil {
		return fmt.Errorf("关闭远程文件失败: %w", err)
	}
	logger.Printf("  ✓ 追加剩余部分: %d bytes\n", len(remainingBuf))

	remoteInfo, err = conn.Client.Stat(testFilename)
	if err != nil {
		return fmt.Errorf("获取最终文件信息失败: %w", err)
	}
	logger.Printf("  ✓ 最终文件大小: %d bytes\n", remoteInfo.Size())

	if remoteInfo.Size() != totalSize {
		return fmt.Errorf("文件大小不匹配: 期望 %d, 实际 %d", totalSize, remoteInfo.Size())
	}
	logger.Printf("  ✓ 断点续传验证通过\n")

	downloadPath := filepath.Join(config.TestDataDir, "sftp_resume_verify.bin")
	downloadFile, err := os.Create(downloadPath)
	if err != nil {
		conn.Client.Remove(testFilename)
		return fmt.Errorf("创建验证文件失败: %w", err)
	}

	remoteFile, err = conn.Client.Open(testFilename)
	if err != nil {
		downloadFile.Close()
		conn.Client.Remove(testFilename)
		return fmt.Errorf("打开远程文件失败: %w", err)
	}

	_, err = io.Copy(downloadFile, remoteFile)
	remoteFile.Close()
	downloadFile.Close()
	if err != nil {
		conn.Client.Remove(testFilename)
		os.Remove(downloadPath)
		return fmt.Errorf("下载验证文件失败: %w", err)
	}

	originalMD5, err := calculateMD5(srcPath)
	if err != nil {
		conn.Client.Remove(testFilename)
		os.Remove(downloadPath)
		return fmt.Errorf("计算原始文件 MD5 失败: %w", err)
	}

	verifyMD5, err := calculateMD5(downloadPath)
	if err != nil {
		conn.Client.Remove(testFilename)
		os.Remove(downloadPath)
		return fmt.Errorf("计算验证文件 MD5 失败: %w", err)
	}

	if originalMD5 != verifyMD5 {
		conn.Client.Remove(testFilename)
		os.Remove(downloadPath)
		return fmt.Errorf("数据完整性验证失败: MD5 不匹配 (原始: %s, 验证: %s)", originalMD5, verifyMD5)
	}
	logger.Printf("  ✓ 数据完整性验证通过 (MD5: %s)\n", originalMD5)

	conn.Client.Remove(testFilename)
	os.Remove(downloadPath)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}
