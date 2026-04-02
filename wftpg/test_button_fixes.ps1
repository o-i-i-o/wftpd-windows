# WFTPG 按钮修复与配置自动重载 - 快速验证脚本
# 用于自动化测试三个关键功能的修复效果

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "WFTPG 功能验证脚本" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# 检查程序是否运行
Write-Host "[1/5] 检查 WFTPG 进程..." -ForegroundColor Yellow
$process = Get-Process wftpg -ErrorAction SilentlyContinue
if ($process) {
    Write-Host "✓ WFTPG 正在运行 (PID: $($process.Id))" -ForegroundColor Green
} else {
    Write-Host "✗ WFTPG 未运行，请先启动程序" -ForegroundColor Red
    Write-Host "  运行命令：.\target\release\wftpg.exe" -ForegroundColor Gray
    exit 1
}

# 检查配置文件
Write-Host "`n[2/5] 检查配置文件..." -ForegroundColor Yellow
$configPath = "C:\ProgramData\wftpg\config.toml"
if (Test-Path $configPath) {
    Write-Host "✓ 配置文件存在: $configPath" -ForegroundColor Green
    
    # 读取当前配置
    $config = Get-Content $configPath -Raw
    Write-Host "  文件大小：$((Get-Item $configPath).Length) 字节" -ForegroundColor Gray
    
    # 检查安全配置部分
    if ($config -match "\[security\]") {
        Write-Host "✓ [security] 配置节存在" -ForegroundColor Green
    } else {
        Write-Host "⚠ [security] 配置节不存在" -ForegroundColor Yellow
    }
} else {
    Write-Host "✗ 配置文件不存在" -ForegroundColor Red
    Write-Host "  路径：$configPath" -ForegroundColor Gray
}

# 检查日志文件
Write-Host "`n[3/5] 检查日志文件..." -ForegroundColor Yellow
$logDir = "C:\ProgramData\wftpg\logs"
if (Test-Path $logDir) {
    Write-Host "✓ 日志目录存在: $logDir" -ForegroundColor Green
    
    # 获取最新的日志文件
    $latestLog = Get-ChildItem $logDir -Filter "*.log" | 
                 Sort-Object LastWriteTime -Descending | 
                 Select-Object -First 1
    
    if ($latestLog) {
        Write-Host "  最新日志：$($latestLog.Name)" -ForegroundColor Gray
        Write-Host "  修改时间：$($latestLog.LastWriteTime)" -ForegroundColor Gray
        
        # 检查是否有自动重载日志
        $logContent = Get-Content $latestLog.FullName -Raw
        if ($logContent -match "Configuration auto-reloaded") {
            Write-Host "✓ 检测到配置自动重载记录" -ForegroundColor Green
        } else {
            Write-Host "⚠ 未发现自动重载记录（可能需要手动测试）" -ForegroundColor Yellow
        }
        
        if ($logContent -match "开始保存安全配置") {
            Write-Host "✓ 检测到安全配置保存记录" -ForegroundColor Green
        } else {
            Write-Host "⚠ 未发现保存记录（可能需要手动测试）" -ForegroundColor Yellow
        }
    }
} else {
    Write-Host "⚠ 日志目录不存在" -ForegroundColor Yellow
}

# 检查 IPC 连接
Write-Host "`n[4/5] 检查 IPC 通信..." -ForegroundColor Yellow
try {
    # 尝试连接命名管道
    $pipeName = "\\.\pipe\wftpg"
    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream(".", $pipeName, [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(1000)  # 1 秒超时
    
    if ($pipe.IsConnected) {
        Write-Host "✓ IPC 管道连接成功" -ForegroundColor Green
        $pipe.Disconnect()
        $pipe.Dispose()
    } else {
        Write-Host "⚠ IPC 管道连接超时（后端可能未运行）" -ForegroundColor Yellow
    }
} catch {
    Write-Host "⚠ IPC 连接失败：$($_.Exception.Message)" -ForegroundColor Red
    Write-Host "  这不影响 GUI 功能，但配置保存后无法通知后端" -ForegroundColor Gray
}

# 生成测试报告
Write-Host "`n[5/5] 生成测试建议..." -ForegroundColor Yellow
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "手动测试步骤" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

Write-Host "【测试 1】安全配置保存按钮反馈" -ForegroundColor White
Write-Host "1. 在 GUI 中导航到 '🔒 安全设置' 标签页" -ForegroundColor Gray
Write-Host "2. 修改 '最大连接数' 为任意值（如 500）" -ForegroundColor Gray
Write-Host "3. 点击 '💾 保存安全配置' 按钮" -ForegroundColor Gray
Write-Host ""
Write-Host "预期结果:" -ForegroundColor Gray
Write-Host "  ✓ 按钮立即变为 '💾 保存中...' 并禁用" -ForegroundColor DarkGray
Write-Host "  ✓ 状态栏显示 '正在保存配置...'" -ForegroundColor DarkGray
Write-Host "  ✓ 1-2 秒后显示成功消息" -ForegroundColor DarkGray
Write-Host "  ✓ 日志中出现 '开始保存安全配置'" -ForegroundColor DarkGray
Write-Host ""

Write-Host "【测试 2】系统服务按钮操作" -ForegroundColor White
Write-Host "1. 导航到 '🖥 系统服务管理' 标签页" -ForegroundColor Gray
Write-Host "2. 观察当前服务状态" -ForegroundColor Gray
Write-Host "3. 点击任意操作按钮（如 '▶️ 启动服务'）" -ForegroundColor Gray
Write-Host ""
Write-Host "预期结果:" -ForegroundColor Gray
Write-Host "  ✓ 按钮立即变为 'XX 中...' 并禁用" -ForegroundColor DarkGray
Write-Host "  ✓ 30 秒内完成或超时" -ForegroundColor DarkGray
Write-Host "  ✓ 显示操作结果消息" -ForegroundColor DarkGray
Write-Host "  ✓ 状态自动刷新" -ForegroundColor DarkGray
Write-Host ""

Write-Host "【测试 3】配置文件自动重载" -ForegroundColor White
Write-Host "1. 保持 GUI 打开" -ForegroundColor Gray
Write-Host "2. 用文本编辑器打开配置文件:" -ForegroundColor Gray
Write-Host "   notepad C:\ProgramData\wftpg\config.toml" -ForegroundColor DarkGray
Write-Host "3. 修改 [security] 节的 max_connections 值" -ForegroundColor Gray
Write-Host "4. 保存文件 (Ctrl+S)" -ForegroundColor Gray
Write-Host "5. 等待约 1 秒" -ForegroundColor Gray
Write-Host ""
Write-Host "预期结果:" -ForegroundColor Gray
Write-Host "  ✓ 日志中出现 'Config file changed'" -ForegroundColor DarkGray
Write-Host "  ✓ 日志中出现 'Configuration auto-reloaded successfully'" -ForegroundColor DarkGray
Write-Host "  ✓ 切换到其他标签页再返回，配置值已更新" -ForegroundColor DarkGray
Write-Host ""

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "快速诊断命令" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

Write-Host "# 查看最新日志（最后 20 行）" -ForegroundColor White
Write-Host "Get-Content C:\ProgramData\wftpg\logs\*.log -Tail 20" -ForegroundColor DarkGray
Write-Host ""

Write-Host "# 实时监控日志" -ForegroundColor White
Write-Host "Get-Content C:\ProgramData\wftpg\logs\*.log -Wait -Tail 10" -ForegroundColor DarkGray
Write-Host ""

Write-Host "# 检查配置文件修改时间" -ForegroundColor White
Write-Host "(Get-Item C:\ProgramData\wftpg\config.toml).LastWriteTime" -ForegroundColor DarkGray
Write-Host ""

Write-Host "# 强制重启服务（如需测试）" -ForegroundColor White
Write-Host "Restart-Service wftpd -Force" -ForegroundColor DarkGray
Write-Host ""

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "验证完成" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# 总结
Write-Host "如果以上所有检查项都通过，说明修复已成功应用。" -ForegroundColor Green
Write-Host "请按照手动测试步骤验证实际效果。" -ForegroundColor Green
Write-Host ""
