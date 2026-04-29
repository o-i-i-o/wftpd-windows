package main

import (
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"time"
)

func testNetworkInterruption() error {
	startTime := time.Now()
	logger.Printf("  [错误恢复] 测试网络中断恢复...\n")

	c, err := connectAndLogin()
	if err != nil {
		return err
	}

	err = c.PrintfLine("TYPE I")
	if err != nil {
		c.Close()
		return fmt.Errorf("发送 TYPE 命令失败: %w", err)
	}
	_, _, err = c.ReadResponse(200)
	if err != nil {
		c.Close()
		return fmt.Errorf("TYPE 命令错误: %w", err)
	}

	dt, err := pasvDataConnect(c)
	if err != nil {
		c.Close()
		return err
	}

	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	file, err := os.Open(srcPath)
	if err != nil {
		dt.Close()
		c.Close()
		return fmt.Errorf("打开文件失败: %w", err)
	}

	err = dt.Fc.PrintfLine("STOR interrupt_test.txt")
	if err != nil {
		file.Close()
		dt.Close()
		c.Close()
		return fmt.Errorf("发送 STOR 命令失败: %w", err)
	}

	code, msg, err := dt.Fc.ReadResponse(150)
	if err != nil && code != 125 {
		file.Close()
		dt.Close()
		c.Close()
		return fmt.Errorf("STOR 准备响应错误: %d %s", code, msg)
	}

	go func() {
		time.Sleep(100 * time.Millisecond)
		dt.Conn.Close()
		c.Close()
	}()

	_, err = io.CopyN(dt.Conn, file, 512)
	file.Close()

	time.Sleep(200 * time.Millisecond)

	logger.Printf("  ✓ 网络中断模拟完成\n")

	retryStart := time.Now()
	maxRetries := 3
	var retryErr error

	for i := 0; i < maxRetries; i++ {
		logger.Printf("  [重试 %d/%d] 尝试重新连接...\n", i+1, maxRetries)

		c2, err := connectAndLogin()
		if err != nil {
			retryErr = err
			time.Sleep(1 * time.Second)
			continue
		}

		err = c2.PrintfLine("TYPE I")
		if err != nil {
			c2.Close()
			retryErr = err
			time.Sleep(1 * time.Second)
			continue
		}
		_, _, err = c2.ReadResponse(200)
		if err != nil {
			c2.Close()
			retryErr = err
			time.Sleep(1 * time.Second)
			continue
		}

		c2.PrintfLine("DELE interrupt_test.txt")
		c2.ReadResponse(250)
		c2.Close()

		logger.Printf("  ✓ 重连成功 (耗时: %.2f ms)\n", float64(time.Since(retryStart).Microseconds())/1000.0)
		logger.Printf("  [总耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
		return nil
	}

	return fmt.Errorf("重连失败: %w", retryErr)
}

func testPermissionDenied() error {
	startTime := time.Now()
	logger.Printf("  [错误恢复] 测试权限拒绝处理...\n")

	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()

	err = c.PrintfLine("MKD /root_restricted_dir")
	if err != nil {
		return fmt.Errorf("发送 MKD 命令失败: %w", err)
	}

	code, _, err := c.ReadResponse(0)
	if err != nil {
		logger.Printf("  ✓ 权限拒绝测试: MKD 返回错误 (预期行为)\n")
	} else if code >= 400 && code < 600 {
		logger.Printf("  ✓ 权限拒绝测试: MKD 返回 %d (预期行为)\n", code)
	} else {
		logger.Printf("  ⚠ 权限拒绝测试: MKD 返回 %d (可能权限控制不严格)\n", code)
		c.PrintfLine("RMD /root_restricted_dir")
		c.ReadResponse(250)
	}

	err = c.PrintfLine("DELE /etc/passwd")
	if err != nil {
		return fmt.Errorf("发送 DELE 命令失败: %w", err)
	}

	code, _, err = c.ReadResponse(0)
	if err != nil {
		logger.Printf("  ✓ 权限拒绝测试: DELE 返回错误 (预期行为)\n")
	} else if code >= 400 && code < 600 {
		logger.Printf("  ✓ 权限拒绝测试: DELE 返回 %d (预期行为)\n", code)
	} else {
		logger.Printf("  ⚠ 权限拒绝测试: DELE 返回 %d (可能权限控制不严格)\n", code)
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testConcurrentAccess() error {
	startTime := time.Now()
	logger.Printf("  [错误恢复] 测试并发访问冲突...\n")

	numClients := 5
	testFilename := "concurrent_access_test.txt"

	errors := make(chan error, numClients)

	for i := 0; i < numClients; i++ {
		go func(id int) {
			c, err := connectAndLogin()
			if err != nil {
				errors <- fmt.Errorf("客户端 %d 连接失败: %w", id, err)
				return
			}
			defer c.Close()

			err = c.PrintfLine("TYPE I")
			if err != nil {
				errors <- fmt.Errorf("客户端 %d TYPE 命令失败: %w", id, err)
				return
			}
			_, _, err = c.ReadResponse(200)
			if err != nil {
				errors <- fmt.Errorf("客户端 %d TYPE 响应错误: %w", id, err)
				return
			}

			dt, err := pasvDataConnect(c)
			if err != nil {
				errors <- fmt.Errorf("客户端 %d PASV 失败: %w", id, err)
				return
			}
			defer dt.Close()

			srcPath := filepath.Join(config.TestDataDir, "small.txt")
			file, err := os.Open(srcPath)
			if err != nil {
				errors <- fmt.Errorf("客户端 %d 打开文件失败: %w", id, err)
				return
			}
			defer file.Close()

			err = dt.Upload(file, testFilename)
			if err != nil {
				errors <- fmt.Errorf("客户端 %d 上传失败: %w", id, err)
				return
			}

			errors <- nil
		}(i)
	}

	successCount := 0
	failCount := 0
	for i := 0; i < numClients; i++ {
		err := <-errors
		if err != nil {
			logger.Printf("  ⚠ 并发访问冲突: %v\n", err)
			failCount++
		} else {
			successCount++
		}
	}

	c, err := connectAndLogin()
	if err == nil {
		c.PrintfLine("DELE %s", testFilename)
		c.ReadResponse(250)
		c.Close()
	}

	logger.Printf("  ✓ 并发访问测试: %d 成功, %d 失败\n", successCount, failCount)
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testInvalidCommands() error {
	startTime := time.Now()
	logger.Printf("  [错误恢复] 测试无效命令处理...\n")

	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()

	invalidCommands := []string{
		"INVALID",
		"UNKNOWN",
		"FOOBAR",
		"12345",
		"!@#$%",
		"",
	}

	for _, cmd := range invalidCommands {
		if cmd == "" {
			continue
		}

		err = c.PrintfLine(cmd)
		if err != nil {
			logger.Printf("  ⚠ 发送无效命令 '%s' 失败: %v\n", cmd, err)
			continue
		}

		code, msg, err := c.ReadResponse(0)
		if err != nil {
			logger.Printf("  ✓ 无效命令 '%s' 返回错误 (预期行为)\n", cmd)
		} else if code >= 400 && code < 600 {
			logger.Printf("  ✓ 无效命令 '%s' 返回 %d (预期行为)\n", cmd, code)
		} else {
			logger.Printf("  ⚠ 无效命令 '%s' 返回 %d %s (可能未正确处理)\n", cmd, code, strings.TrimSpace(msg))
		}
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testMalformedCommands() error {
	startTime := time.Now()
	logger.Printf("  [错误恢复] 测试畸形命令处理...\n")

	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()

	malformedCommands := []string{
		"USER",
		"PASS",
		"RETR",
		"STOR",
		"CWD",
		"MKD",
		"RMD",
		"DELE",
	}

	for _, cmd := range malformedCommands {
		err = c.PrintfLine(cmd)
		if err != nil {
			logger.Printf("  ⚠ 发送畸形命令 '%s' 失败: %v\n", cmd, err)
			continue
		}

		code, msg, err := c.ReadResponse(0)
		if err != nil {
			logger.Printf("  ✓ 畸形命令 '%s' 返回错误 (预期行为)\n", cmd)
		} else if code >= 400 && code < 600 {
			logger.Printf("  ✓ 畸形命令 '%s' 返回 %d (预期行为)\n", cmd, code)
		} else {
			logger.Printf("  ⚠ 畸形命令 '%s' 返回 %d %s (可能未正确处理)\n", cmd, code, strings.TrimSpace(msg))
		}
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testTimeoutHandling() error {
	startTime := time.Now()
	logger.Printf("  [错误恢复] 测试超时处理...\n")

	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()

	logger.Printf("  ✓ 测试空闲超时 (等待 5 秒)...\n")
	time.Sleep(5 * time.Second)

	err = c.PrintfLine("NOOP")
	if err != nil {
		return fmt.Errorf("NOOP 命令失败: %w", err)
	}

	code, msg, err := c.ReadResponse(200)
	if err != nil {
		return fmt.Errorf("连接可能已超时断开: %d %s (err: %v)", code, msg, err)
	}

	logger.Printf("  ✓ 连接保持活跃 (NOOP 响应: %s)\n", strings.TrimSpace(msg))
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testDataConnectionFailure() error {
	startTime := time.Now()
	logger.Printf("  [错误恢复] 测试数据连接失败处理...\n")

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
		return err
	}

	err = c.PrintfLine("STOR data_fail_test.txt")
	if err != nil {
		return fmt.Errorf("发送 STOR 命令失败: %w", err)
	}

	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	dataConn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", host, port), timeout)
	if err != nil {
		logger.Printf("  ⚠ 数据连接失败: %v\n", err)
		return fmt.Errorf("连接数据端口失败: %w", err)
	}

	dataConn.Close()

	code, msg, err = c.ReadResponse(0)
	if err != nil {
		logger.Printf("  ✓ 数据连接关闭后服务器正确响应: %v\n", err)
	} else {
		logger.Printf("  ✓ 数据连接关闭后服务器响应: %d %s\n", code, strings.TrimSpace(msg))
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}
