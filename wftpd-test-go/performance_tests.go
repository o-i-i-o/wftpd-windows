package main

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"time"
)

func testBatchTransfer() error {
	startTime := time.Now()
	logger.Printf("  [性能] 测试批量文件传输...\n")

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

	numFiles := 10
	testFiles := make([]string, numFiles)
	for i := 0; i < numFiles; i++ {
		testFiles[i] = fmt.Sprintf("batch_test_%d.txt", i)
	}

	srcPath := filepath.Join(config.TestDataDir, "small.txt")
	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开源文件失败: %w", err)
	}
	defer file.Close()

	uploadStart := time.Now()
	for i, filename := range testFiles {
		file.Seek(0, 0)

		dt, err := pasvDataConnect(c)
		if err != nil {
			return fmt.Errorf("文件 %d PASV 失败: %w", i, err)
		}

		err = dt.Upload(file, filename)
		dt.Close()
		if err != nil {
			return fmt.Errorf("上传文件 %d 失败: %w", i, err)
		}

		if (i+1)%5 == 0 {
			logger.Printf("  ✓ 上传进度: %d/%d\n", i+1, numFiles)
		}
	}
	uploadDuration := time.Since(uploadStart)
	logger.Printf("  ✓ 批量上传完成: %d 个文件 (耗时: %.2f s)\n", numFiles, uploadDuration.Seconds())

	downloadStart := time.Now()
	for i, filename := range testFiles {
		dt, err := pasvDataConnect(c)
		if err != nil {
			return fmt.Errorf("文件 %d PASV 失败: %w", i, err)
		}

		dstPath := filepath.Join(config.TestDataDir, filename+"_downloaded")
		outFile, err := os.Create(dstPath)
		if err != nil {
			dt.Close()
			return fmt.Errorf("创建下载文件 %d 失败: %w", i, err)
		}

		err = dt.Download(outFile, filename)
		dt.Close()
		outFile.Close()
		if err != nil {
			os.Remove(dstPath)
			return fmt.Errorf("下载文件 %d 失败: %w", i, err)
		}

		os.Remove(dstPath)

		if (i+1)%5 == 0 {
			logger.Printf("  ✓ 下载进度: %d/%d\n", i+1, numFiles)
		}
	}
	downloadDuration := time.Since(downloadStart)
	logger.Printf("  ✓ 批量下载完成: %d 个文件 (耗时: %.2f s)\n", numFiles, downloadDuration.Seconds())

	for _, filename := range testFiles {
		c.PrintfLine("DELE %s", filename)
		c.ReadResponse(250)
	}

	avgUploadTime := uploadDuration.Seconds() / float64(numFiles)
	avgDownloadTime := downloadDuration.Seconds() / float64(numFiles)
	logger.Printf("  ✓ 平均上传时间: %.2f s/文件\n", avgUploadTime)
	logger.Printf("  ✓ 平均下载时间: %.2f s/文件\n", avgDownloadTime)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testTransferQueue() error {
	startTime := time.Now()
	logger.Printf("  [性能] 测试传输队列管理...\n")

	numWorkers := 3
	numFiles := 9

	var wg sync.WaitGroup
	errors := make(chan error, numFiles)

	uploadQueue := make(chan int, numFiles)
	for i := 0; i < numFiles; i++ {
		uploadQueue <- i
	}
	close(uploadQueue)

	workerStart := time.Now()
	for w := 0; w < numWorkers; w++ {
		wg.Add(1)
		go func(workerID int) {
			defer wg.Done()

			c, err := connectAndLogin()
			if err != nil {
				errors <- fmt.Errorf("worker %d 连接失败: %w", workerID, err)
				return
			}
			defer c.Close()

			err = c.PrintfLine("TYPE I")
			if err != nil {
				errors <- fmt.Errorf("worker %d TYPE 失败: %w", workerID, err)
				return
			}
			_, _, err = c.ReadResponse(200)
			if err != nil {
				errors <- fmt.Errorf("worker %d TYPE 响应错误: %w", workerID, err)
				return
			}

			for fileID := range uploadQueue {
				filename := fmt.Sprintf("queue_test_%d.txt", fileID)
				srcPath := filepath.Join(config.TestDataDir, "small.txt")

				dt, err := pasvDataConnect(c)
				if err != nil {
					errors <- fmt.Errorf("worker %d file %d PASV 失败: %w", workerID, fileID, err)
					continue
				}

				file, err := os.Open(srcPath)
				if err != nil {
					dt.Close()
					errors <- fmt.Errorf("worker %d file %d 打开文件失败: %w", workerID, fileID, err)
					continue
				}

				err = dt.Upload(file, filename)
				file.Close()
				dt.Close()
				if err != nil {
					errors <- fmt.Errorf("worker %d file %d 上传失败: %w", workerID, fileID, err)
					continue
				}

				c.PrintfLine("DELE %s", filename)
				c.ReadResponse(250)

				errors <- nil
			}
		}(w)
	}

	wg.Wait()
	close(errors)

	successCount := 0
	failCount := 0
	for err := range errors {
		if err != nil {
			logger.Printf("  ⚠ 队列传输失败: %v\n", err)
			failCount++
		} else {
			successCount++
		}
	}

	workerDuration := time.Since(workerStart)
	logger.Printf("  ✓ 队列传输完成: %d 成功, %d 失败\n", successCount, failCount)
	logger.Printf("  ✓ 队列处理时间: %.2f s (使用 %d 个工作线程)\n", workerDuration.Seconds(), numWorkers)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testBandwidthLimit() error {
	startTime := time.Now()
	logger.Printf("  [性能] 测试带宽限制效果...\n")

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
		logger.Printf("  ⚠ medium.bin 不存在，跳过带宽限制测试\n")
		return nil
	}

	dt, err := pasvDataConnect(c)
	if err != nil {
		return err
	}

	file, err := os.Open(srcPath)
	if err != nil {
		return fmt.Errorf("打开文件失败: %w", err)
	}
	defer file.Close()

	fileInfo, _ := file.Stat()
	totalSize := fileInfo.Size()

	uploadStart := time.Now()
	err = dt.Upload(file, "bandwidth_test.bin")
	if err != nil {
		return fmt.Errorf("上传失败: %w", err)
	}
	uploadDuration := time.Since(uploadStart)

	uploadRate := float64(totalSize) / uploadDuration.Seconds() / 1024 / 1024
	logger.Printf("  ✓ 上传速率: %.2f MB/s (%.2f MB in %.2f s)\n", uploadRate, float64(totalSize)/1024/1024, uploadDuration.Seconds())

	dt2, err := pasvDataConnect(c)
	if err != nil {
		return err
	}
	defer dt2.Close()

	dstPath := filepath.Join(config.TestDataDir, "bandwidth_downloaded.bin")
	outFile, err := os.Create(dstPath)
	if err != nil {
		return fmt.Errorf("创建下载文件失败: %w", err)
	}
	defer outFile.Close()
	defer os.Remove(dstPath)

	downloadStart := time.Now()
	err = dt2.Download(outFile, "bandwidth_test.bin")
	if err != nil {
		return fmt.Errorf("下载失败: %w", err)
	}
	downloadDuration := time.Since(downloadStart)

	downloadRate := float64(totalSize) / downloadDuration.Seconds() / 1024 / 1024
	logger.Printf("  ✓ 下载速率: %.2f MB/s (%.2f MB in %.2f s)\n", downloadRate, float64(totalSize)/1024/1024, downloadDuration.Seconds())

	c.PrintfLine("DELE bandwidth_test.bin")
	c.ReadResponse(250)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testResourceUsage() error {
	startTime := time.Now()
	logger.Printf("  [性能] 测试资源使用情况...\n")

	var m1, m2 runtime.MemStats
	runtime.ReadMemStats(&m1)

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

	numTransfers := 5
	for i := 0; i < numTransfers; i++ {
		dt, err := pasvDataConnect(c)
		if err != nil {
			return err
		}

		srcPath := filepath.Join(config.TestDataDir, "small.txt")
		file, err := os.Open(srcPath)
		if err != nil {
			dt.Close()
			return fmt.Errorf("打开文件失败: %w", err)
		}

		filename := fmt.Sprintf("resource_test_%d.txt", i)
		err = dt.Upload(file, filename)
		file.Close()
		dt.Close()
		if err != nil {
			return fmt.Errorf("上传失败: %w", err)
		}

		c.PrintfLine("DELE %s", filename)
		c.ReadResponse(250)
	}

	runtime.ReadMemStats(&m2)

	memAllocDiff := int64(m2.Alloc) - int64(m1.Alloc)
	memTotalDiff := int64(m2.TotalAlloc) - int64(m1.TotalAlloc)
	memSysDiff := int64(m2.Sys) - int64(m1.Sys)

	logger.Printf("  ✓ 内存分配差异: %d bytes\n", memAllocDiff)
	logger.Printf("  ✓ 总分配差异: %d bytes\n", memTotalDiff)
	logger.Printf("  ✓ 系统内存差异: %d bytes\n", memSysDiff)
	logger.Printf("  ✓ GC 次数: %d\n", m2.NumGC-m1.NumGC)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}

func testStressTest() error {
	startTime := time.Now()
	logger.Printf("  [性能] 压力测试 (多用户并发)...\n")

	numUsers := 5
	opsPerUser := 3

	var wg sync.WaitGroup
	errors := make(chan error, numUsers*opsPerUser)

	stressStart := time.Now()
	for u := 0; u < numUsers; u++ {
		wg.Add(1)
		go func(userID int) {
			defer wg.Done()

			for op := 0; op < opsPerUser; op++ {
				c, err := connectAndLogin()
				if err != nil {
					errors <- fmt.Errorf("user %d op %d 连接失败: %w", userID, op, err)
					continue
				}

				err = c.PrintfLine("TYPE I")
				if err != nil {
					c.Close()
					errors <- fmt.Errorf("user %d op %d TYPE 失败: %w", userID, op, err)
					continue
				}
				_, _, err = c.ReadResponse(200)
				if err != nil {
					c.Close()
					errors <- fmt.Errorf("user %d op %d TYPE 响应错误: %w", userID, op, err)
					continue
				}

				filename := fmt.Sprintf("stress_%d_%d.txt", userID, op)
				srcPath := filepath.Join(config.TestDataDir, "small.txt")

				dt, err := pasvDataConnect(c)
				if err != nil {
					c.Close()
					errors <- fmt.Errorf("user %d op %d PASV 失败: %w", userID, op, err)
					continue
				}

				file, err := os.Open(srcPath)
				if err != nil {
					dt.Close()
					c.Close()
					errors <- fmt.Errorf("user %d op %d 打开文件失败: %w", userID, op, err)
					continue
				}

				err = dt.Upload(file, filename)
				file.Close()
				dt.Close()
				if err != nil {
					c.Close()
					errors <- fmt.Errorf("user %d op %d 上传失败: %w", userID, op, err)
					continue
				}

				c.PrintfLine("DELE %s", filename)
				c.ReadResponse(250)
				c.Close()

				errors <- nil
			}
		}(u)
	}

	wg.Wait()
	close(errors)

	successCount := 0
	failCount := 0
	for err := range errors {
		if err != nil {
			logger.Printf("  ⚠ 压力测试失败: %v\n", err)
			failCount++
		} else {
			successCount++
		}
	}

	stressDuration := time.Since(stressStart)
	totalOps := numUsers * opsPerUser
	opsPerSec := float64(totalOps) / stressDuration.Seconds()

	logger.Printf("  ✓ 压力测试完成: %d 成功, %d 失败\n", successCount, failCount)
	logger.Printf("  ✓ 总操作数: %d (耗时: %.2f s)\n", totalOps, stressDuration.Seconds())
	logger.Printf("  ✓ 吞吐量: %.2f ops/s\n", opsPerSec)

	logger.Printf("  [耗时] %.2f ms\n", float64(time.Since(startTime).Microseconds())/1000.0)
	return nil
}
