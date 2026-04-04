package main

import (
	"fmt"
	"golang.org/x/crypto/ssh"
	"github.com/pkg/sftp"
	"os"
	"path/filepath"
	"time"
)

// SFTP 测试函数
func testSFTPConnection() error {
	startTime := time.Now()
	fmt.Printf("  [SFTP] 正在连接到 %s:%d...\n", config.SFTPServer, config.SFTPPort)

	// SSH 连接配置
	sshConfig := &ssh.ClientConfig{
		User: config.Username,
		Auth: []ssh.AuthMethod{
			ssh.Password(config.Password),
		},
		HostKeyCallback: ssh.InsecureIgnoreHostKey(), // 注意：生产环境中不应忽略主机密钥
		Timeout:         10 * time.Second,
	}

	// 连接到 SSH 服务器
	conn, err := ssh.Dial("tcp", fmt.Sprintf("%s:%d", config.SFTPServer, config.SFTPPort), sshConfig)
	if err != nil {
		return fmt.Errorf("SSH 连接失败：%w", err)
	}
	defer conn.Close()

	// 创建 SFTP 客户端
	client, err := sftp.New(conn)
	if err != nil {
		return fmt.Errorf("创建 SFTP 客户端失败：%w", err)
	}
	defer client.Close()

	fmt.Printf("  ✓ SFTP 连接建立成功\n")
	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// SFTP 目录操作测试
func testSFTPDirectoryOps() error {
	startTime := time.Now()
	fmt.Printf("  [SFTP] 测试目录操作...\n")

	// SSH 连接配置
	sshConfig := &ssh.ClientConfig{
		User: config.Username,
		Auth: []ssh.AuthMethod{
			ssh.Password(config.Password),
		},
		HostKeyCallback: ssh.InsecureIgnoreHostKey(),
		Timeout:         10 * time.Second,
	}

	// 连接到 SSH 服务器
	conn, err := ssh.Dial("tcp", fmt.Sprintf("%s:%d", config.SFTPServer, config.SFTPPort), sshConfig)
	if err != nil {
		return fmt.Errorf("SSH 连接失败：%w", err)
	}
	defer conn.Close()

	// 创建 SFTP 客户端
	client, err := sftp.New(conn)
	if err != nil {
		return fmt.Errorf("创建 SFTP 客户端失败：%w", err)
	}
	defer client.Close()

	// 获取当前工作目录
	wd, err := client.Getwd()
	if err != nil {
		return fmt.Errorf("获取当前目录失败：%w", err)
	}
	fmt.Printf("  ✓ 当前目录：%s\n", wd)

	// 创建测试目录
	testDir := "sftp_test_dir"
	err = client.Mkdir(testDir)
	if err != nil {
		return fmt.Errorf("创建目录失败：%w", err)
	}
	fmt.Printf("  ✓ 创建目录：%s\n", testDir)

	// 列出目录内容
	files, err := client.ReadDir("/")
	if err != nil {
		return fmt.Errorf("读取目录失败：%w", err)
	}
	fmt.Printf("  ✓ 根目录包含 %d 个项目\n", len(files))

	// 删除测试目录
	err = client.RemoveDirectory(testDir)
	if err != nil {
		return fmt.Errorf("删除目录失败：%w", err)
	}
	fmt.Printf("  ✓ 删除目录：%s\n", testDir)

	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// SFTP 文件操作测试
func testSFTPFileOps() error {
	startTime := time.Now()
	fmt.Printf("  [SFTP] 测试文件操作...\n")

	// SSH 连接配置
	sshConfig := &ssh.ClientConfig{
		User: config.Username,
		Auth: []ssh.AuthMethod{
			ssh.Password(config.Password),
		},
		HostKeyCallback: ssh.InsecureIgnoreHostKey(),
		Timeout:         10 * time.Second,
	}

	// 连接到 SSH 服务器
	conn, err := ssh.Dial("tcp", fmt.Sprintf("%s:%d", config.SFTPServer, config.SFTPPort), sshConfig)
	if err != nil {
		return fmt.Errorf("SSH 连接失败：%w", err)
	}
	defer conn.Close()

	// 创建 SFTP 客户端
	client, err := sftp.New(conn)
	if err != nil {
		return fmt.Errorf("创建 SFTP 客户端失败：%w", err)
	}
	defer client.Close()

	// 上传小文件测试
	filename := "small.txt"
	localPath := filepath.Join(config.TestDataDir, filename)
	remotePath := filename

	// 打开本地文件
	localFile, err := os.Open(localPath)
	if err != nil {
		return fmt.Errorf("打开本地文件失败：%w", err)
	}
	defer localFile.Close()

	// 创建远程文件
	remoteFile, err := client.Create(remotePath)
	if err != nil {
		return fmt.Errorf("创建远程文件失败：%w", err)
	}

	// 复制文件内容
	bytesWritten, err := remoteFile.ReadFrom(localFile)
	if err != nil {
		remoteFile.Close()
		return fmt.Errorf("上传文件失败：%w", err)
	}
	remoteFile.Close()

	fmt.Printf("  ✓ 上传文件：%s (%.2f KB)\n", filename, float64(bytesWritten)/1024.0)

	// 下载文件测试
	downloadPath := filepath.Join(config.TestDataDir, filename+"_sftp_downloaded")
	downloadFile, err := os.Create(downloadPath)
	if err != nil {
		return fmt.Errorf("创建下载文件失败：%w", err)
	}

	// 打开远程文件
	remoteReadFile, err := client.Open(remotePath)
	if err != nil {
		downloadFile.Close()
		return fmt.Errorf("打开远程文件失败：%w", err)
	}

	// 复制到本地
	bytesRead, err := remoteReadFile.WriteTo(downloadFile)
	if err != nil {
		remoteReadFile.Close()
		downloadFile.Close()
		return fmt.Errorf("下载文件失败：%w", err)
	}

	remoteReadFile.Close()
	downloadFile.Close()

	fmt.Printf("  ✓ 下载文件：%s (%.2f KB)\n", filename, float64(bytesRead)/1024.0)

	// 获取文件信息
	fileInfo, err := client.Stat(remotePath)
	if err != nil {
		return fmt.Errorf("获取文件信息失败：%w", err)
	}
	fmt.Printf("  ✓ 文件信息：%s (%d bytes)\n", fileInfo.Name(), fileInfo.Size())

	// 删除上传的文件
	err = client.Remove(remotePath)
	if err != nil {
		return fmt.Errorf("删除远程文件失败：%w", err)
	}
	fmt.Printf("  ✓ 删除远程文件：%s\n", remotePath)

	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// SFTP 文件重命名测试
func testSFTRenameOps() error {
	startTime := time.Now()
	fmt.Printf("  [SFTP] 测试文件重命名...\n")

	// SSH 连接配置
	sshConfig := &ssh.ClientConfig{
		User: config.Username,
		Auth: []ssh.AuthMethod{
			ssh.Password(config.Password),
		},
		HostKeyCallback: ssh.InsecureIgnoreHostKey(),
		Timeout:         10 * time.Second,
	}

	// 连接到 SSH 服务器
	conn, err := ssh.Dial("tcp", fmt.Sprintf("%s:%d", config.SFTPServer, config.SFTPPort), sshConfig)
	if err != nil {
		return fmt.Errorf("SSH 连接失败：%w", err)
	}
	defer conn.Close()

	// 创建 SFTP 客户端
	client, err := sftp.New(conn)
	if err != nil {
		return fmt.Errorf("创建 SFTP 客户端失败：%w", err)
	}
	defer client.Close()

	// 创建测试文件
	testFile := "rename_test.txt"
	testFileContent := "This is a test file for rename operation."

	// 创建并写入内容
	remoteFile, err := client.Create(testFile)
	if err != nil {
		return fmt.Errorf("创建测试文件失败：%w", err)
	}
	_, err = remoteFile.WriteString(testFileContent)
	if err != nil {
		remoteFile.Close()
		return fmt.Errorf("写入测试文件失败：%w", err)
	}
	remoteFile.Close()

	fmt.Printf("  ✓ 创建测试文件：%s\n", testFile)

	// 重命名文件
	newName := "renamed_test.txt"
	err = client.Rename(testFile, newName)
	if err != nil {
		return fmt.Errorf("重命名文件失败：%w", err)
	}
	fmt.Printf("  ✓ 重命名文件：%s -> %s\n", testFile, newName)

	// 验证重命名后文件存在
	_, err = client.Stat(newName)
	if err != nil {
		return fmt.Errorf("验证重命名文件失败：%w", err)
	}
	fmt.Printf("  ✓ 验证重命名文件存在\n")

	// 删除测试文件
	err = client.Remove(newName)
	if err != nil {
		return fmt.Errorf("删除测试文件失败：%w", err)
	}
	fmt.Printf("  ✓ 删除测试文件：%s\n", newName)

	fmt.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

// 运行 SFTP 测试
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