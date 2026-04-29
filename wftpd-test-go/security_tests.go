package main

import (
	"fmt"
	"net"
	"net/textproto"
	"strings"
	"time"
)

func testCommandInjection() error {
	startTime := time.Now()
	logger.Printf("  [安全] 测试命令注入防护...\n")

	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()

	injectionPayloads := []string{
		"test.txt; rm -rf /",
		"test.txt && cat /etc/passwd",
		"test.txt | ls -la",
		"test.txt`whoami`",
		"test.txt$(id)",
		"test.txt; DROP TABLE users;--",
		"test.txt\x00.exe",
		"test.txt%0AUSER anonymous",
		"test.txt%0D%0AUSER anonymous",
	}

	blockedCount := 0
	for _, payload := range injectionPayloads {
		err = c.PrintfLine("RETR %s", payload)
		if err != nil {
			logger.Printf("  ✓ 命令注入被阻止: %s\n", payload)
			blockedCount++
			continue
		}

		code, msg, _ := c.ReadResponse(0)
		if code >= 400 && code < 600 {
			logger.Printf("  ✓ 命令注入被阻止: %s (响应码: %d)\n", payload, code)
			blockedCount++
		} else {
			logger.Printf("  ⚠ 命令注入未被阻止: %s (响应码: %d, 消息: %s)\n", payload, code, strings.TrimSpace(msg))
		}
	}

	logger.Printf("  ✓ 命令注入防护测试: %d/%d 被阻止\n", blockedCount, len(injectionPayloads))
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testBufferOverflow() error {
	startTime := time.Now()
	logger.Printf("  [安全] 测试缓冲区溢出防护...\n")

	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()

	longString := strings.Repeat("A", 4096)
	longCommand := fmt.Sprintf("USER %s", longString)

	err = c.PrintfLine(longCommand)
	if err != nil {
		logger.Printf("  ✓ 超长命令被拒绝: %v\n", err)
		logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
		return nil
	}

	code, msg, err := c.ReadResponse(0)
	if err != nil {
		logger.Printf("  ✓ 超长命令导致连接关闭或错误 (预期行为)\n")
	} else if code >= 400 && code < 600 {
		logger.Printf("  ✓ 超长命令被拒绝 (响应码: %d)\n", code)
	} else {
		logger.Printf("  ⚠ 超长命令未被正确处理 (响应码: %d, 消息: %s)\n", code, strings.TrimSpace(msg))
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testUnauthorizedAccess() error {
	startTime := time.Now()
	logger.Printf("  [安全] 测试未授权访问防护...\n")

	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	conn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort), timeout)
	if err != nil {
		return fmt.Errorf("连接失败: %w", err)
	}
	defer conn.Close()

	c := textproto.NewConn(conn)

	_, _, err = c.ReadResponse(220)
	if err != nil {
		return fmt.Errorf("读取欢迎消息失败: %w", err)
	}

	logger.Printf("  ✓ 服务器欢迎消息已接收\n")

	err = c.PrintfLine("LIST")
	if err != nil {
		logger.Printf("  ✓ 未认证用户命令被拒绝: %v\n", err)
	} else {
		code, msg, _ := c.ReadResponse(0)
		if code >= 400 && code < 600 {
			logger.Printf("  ✓ 未认证用户命令被拒绝 (响应码: %d)\n", code)
		} else {
			logger.Printf("  ⚠ 未认证用户可以执行命令 (响应码: %d, 消息: %s)\n", code, strings.TrimSpace(msg))
		}
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testSensitiveDataLeak() error {
	startTime := time.Now()
	logger.Printf("  [安全] 测试敏感信息泄露防护...\n")

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
		logger.Printf("  ⚠ STAT 命令不支持: %d %s\n", code, strings.TrimSpace(msg))
	} else {
		sensitivePatterns := []string{
			"password",
			"passwd",
			"secret",
			"key",
			"token",
			"credential",
		}

		foundSensitive := false
		lowerMsg := strings.ToLower(msg)
		for _, pattern := range sensitivePatterns {
			if strings.Contains(lowerMsg, pattern) {
				logger.Printf("  ⚠ 可能泄露敏感信息: 发现 '%s'\n", pattern)
				foundSensitive = true
			}
		}

		if !foundSensitive {
			logger.Printf("  ✓ STAT 响应未发现敏感信息\n")
		}
	}

	err = c.PrintfLine("HELP")
	if err != nil {
		return fmt.Errorf("发送 HELP 命令失败: %w", err)
	}

	code, msg, err = c.ReadResponse(214)
	if err != nil {
		logger.Printf("  ⚠ HELP 命令不支持: %d %s\n", code, strings.TrimSpace(msg))
	} else {
		logger.Printf("  ✓ HELP 响应: %s\n", strings.TrimSpace(msg))
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testAnonymousAccess() error {
	startTime := time.Now()
	logger.Printf("  [安全] 测试匿名访问控制...\n")

	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	conn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort), timeout)
	if err != nil {
		return fmt.Errorf("连接失败: %w", err)
	}
	defer conn.Close()

	c := textproto.NewConn(conn)

	_, _, err = c.ReadResponse(220)
	if err != nil {
		return fmt.Errorf("读取欢迎消息失败: %w", err)
	}

	anonymousUsers := []string{
		"anonymous",
		"Anonymous",
		"ANONYMOUS",
		"ftp",
		"FTP",
	}

	for _, user := range anonymousUsers {
		err = c.PrintfLine("USER %s", user)
		if err != nil {
			logger.Printf("  ✓ 匿名用户 '%s' 被拒绝\n", user)
			continue
		}

		code, msg, err := c.ReadResponse(0)
		if err != nil {
			logger.Printf("  ✓ 匿名用户 '%s' 被拒绝\n", user)
			continue
		}

		if code == 331 {
			err = c.PrintfLine("PASS anonymous@anonymous.com")
			if err != nil {
				logger.Printf("  ✓ 匿名用户 '%s' 密码被拒绝\n", user)
				continue
			}

			code, msg, err = c.ReadResponse(0)
			if err != nil || code != 230 {
				logger.Printf("  ✓ 匿名用户 '%s' 登录失败 (响应码: %d)\n", user, code)
			} else {
				logger.Printf("  ⚠ 匿名用户 '%s' 登录成功 (可能存在安全风险)\n", user)
				c.PrintfLine("QUIT")
				c.ReadResponse(221)
			}
		} else if code >= 400 && code < 600 {
			logger.Printf("  ✓ 匿名用户 '%s' 被拒绝 (响应码: %d)\n", user, code)
		} else {
			logger.Printf("  ⚠ 匿名用户 '%s' 响应异常 (响应码: %d, 消息: %s)\n", user, code, strings.TrimSpace(msg))
		}
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testPortCommandSecurity() error {
	startTime := time.Now()
	logger.Printf("  [安全] 测试 PORT 命令安全性...\n")

	c, err := connectAndLogin()
	if err != nil {
		return err
	}
	defer c.Close()

	maliciousPortCommands := []struct {
		desc string
		cmd  string
	}{
		{"尝试连接到其他服务器", "PORT 8,8,8,8,0,21"},
		{"尝试连接到本地非FTP端口", "PORT 127,0,0,1,0,80"},
		{"尝试连接到特权端口", "PORT 127,0,0,1,0,22"},
		{"尝试连接到广播地址", "PORT 255,255,255,255,0,21"},
		{"尝试连接到私有网络", "PORT 192,168,1,1,0,21"},
	}

	blockedCount := 0
	for _, test := range maliciousPortCommands {
		err = c.PrintfLine(test.cmd)
		if err != nil {
			logger.Printf("  ✓ PORT 命令被阻止: %s\n", test.desc)
			blockedCount++
			continue
		}

		code, msg, _ := c.ReadResponse(0)
		if code >= 400 && code < 600 {
			logger.Printf("  ✓ PORT 命令被阻止: %s (响应码: %d)\n", test.desc, code)
			blockedCount++
		} else {
			logger.Printf("  ⚠ PORT 命令未被阻止: %s (响应码: %d, 消息: %s)\n", test.desc, code, strings.TrimSpace(msg))
		}
	}

	logger.Printf("  ✓ PORT 命令安全测试: %d/%d 被阻止\n", blockedCount, len(maliciousPortCommands))
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testPasvSecurity() error {
	startTime := time.Now()
	logger.Printf("  [安全] 测试 PASV 命令安全性...\n")

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

	ip := net.ParseIP(host)
	if ip == nil {
		return fmt.Errorf("无效的 IP 地址: %s", host)
	}

	if !ip.IsLoopback() && !ip.IsPrivate() {
		logger.Printf("  ⚠ PASV 返回公网 IP %s，可能存在安全风险\n", host)
	} else {
		logger.Printf("  ✓ PASV 返回安全的 IP 地址: %s\n", host)
	}

	if port < 1024 {
		logger.Printf("  ⚠ PASV 返回特权端口 %d，可能存在安全风险\n", port)
	} else {
		logger.Printf("  ✓ PASV 返回非特权端口: %d\n", port)
	}

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testBruteForceProtection() error {
	startTime := time.Now()
	logger.Printf("  [安全] 测试暴力破解防护...\n")

	timeout := time.Duration(config.TimeoutSeconds) * time.Second
	maxAttempts := 5

	for i := 0; i < maxAttempts; i++ {
		conn, err := net.DialTimeout("tcp", fmt.Sprintf("%s:%d", config.FTPServer, config.FTPPort), timeout)
		if err != nil {
			logger.Printf("  ⚠ 连接失败 (尝试 %d/%d): %v\n", i+1, maxAttempts, err)
			continue
		}

		c := textproto.NewConn(conn)

		_, _, err = c.ReadResponse(220)
		if err != nil {
			conn.Close()
			logger.Printf("  ⚠ 读取欢迎消息失败 (尝试 %d/%d): %v\n", i+1, maxAttempts, err)
			continue
		}

		err = c.PrintfLine("USER %s", config.Username)
		if err != nil {
			conn.Close()
			logger.Printf("  ⚠ USER 命令失败 (尝试 %d/%d): %v\n", i+1, maxAttempts, err)
			continue
		}

		_, _, err = c.ReadResponse(331)
		if err != nil {
			conn.Close()
			logger.Printf("  ⚠ USER 响应错误 (尝试 %d/%d): %v\n", i+1, maxAttempts, err)
			continue
		}

		err = c.PrintfLine("PASS wrongpassword%d", i)
		if err != nil {
			conn.Close()
			logger.Printf("  ⚠ PASS 命令失败 (尝试 %d/%d): %v\n", i+1, maxAttempts, err)
			continue
		}

		code, msg, err := c.ReadResponse(0)
		conn.Close()

		if err != nil {
			logger.Printf("  ✓ 连接被关闭 (尝试 %d/%d)，可能触发了暴力破解防护\n", i+1, maxAttempts)
			logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
			return nil
		}

		if code == 530 {
			logger.Printf("  ✓ 密码错误被拒绝 (尝试 %d/%d)\n", i+1, maxAttempts)
		} else if code >= 400 && code < 600 {
			logger.Printf("  ✓ 登录失败 (尝试 %d/%d, 响应码: %d)\n", i+1, maxAttempts, code)
		} else {
			logger.Printf("  ⚠ 意外响应 (尝试 %d/%d, 响应码: %d, 消息: %s)\n", i+1, maxAttempts, code, strings.TrimSpace(msg))
		}

		time.Sleep(100 * time.Millisecond)
	}

	logger.Printf("  ⚠ 未触发暴力破解防护 (尝试了 %d 次错误密码)\n", maxAttempts)
	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}
