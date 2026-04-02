# WFTPG 内存诊断工具
# 用于详细分析内存占用情况

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "WFTPG 内存占用分析" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# 获取进程
$process = Get-Process wftpg -ErrorAction SilentlyContinue
if (-not $process) {
    Write-Host "✗ WFTPG 未运行" -ForegroundColor Red
    exit 1
}

Write-Host "进程信息:" -ForegroundColor Yellow
Write-Host "  PID: $($process.Id)" -ForegroundColor Gray
Write-Host "  工作集内存 (WS): $([math]::Round($process.WorkingSet / 1MB, 2)) MB" -ForegroundColor Gray
Write-Host "  私有内存 (Private): $([math]::Round($process.PrivateMemorySize64 / 1MB, 2)) MB" -ForegroundColor Gray
Write-Host "  虚拟内存 (Virtual): $([math]::Round($process.VirtualMemorySize64 / 1MB, 2)) MB" -ForegroundColor Gray
Write-Host "  CPU 使用率：$([math]::Round($process.CPU, 2))%" -ForegroundColor Gray
Write-Host "  线程数：$($process.Threads.Count)" -ForegroundColor Gray
Write-Host "  句柄数：$($process.HandleCount)" -ForegroundColor Gray
Write-Host ""

# 线程详情
Write-Host "线程详情 (Top 10 by CPU):" -ForegroundColor Yellow
$process.Threads | 
    Sort-Object TotalProcessorTime -Descending | 
    Select-Object -First 10 | 
    ForEach-Object {
        $cpuTime = $_.TotalProcessorTime.TotalMilliseconds
        Write-Host "  Thread $($_.Id): $([math]::Round($cpuTime, 2)) ms" -ForegroundColor Gray
    }
Write-Host ""

# 模块大小
Write-Host "加载的主要模块:" -ForegroundColor Yellow
$process.Modules | 
    Where-Object { $_.FileName -match "(wftpg|egui|wgpu|windows)" } |
    Sort-Object ModuleMemorySize -Descending |
    Select-Object -First 15 |
    ForEach-Object {
        $size = [math]::Round($_.ModuleMemorySize / 1KB, 2)
        Write-Host "  $([System.IO.Path]::GetFileName($_.FileName)): $size KB" -ForegroundColor Gray
    }
Write-Host ""

# 检查日志文件
Write-Host "日志文件统计:" -ForegroundColor Yellow
$logDir = "C:\ProgramData\wftpg\logs"
if (Test-Path $logDir) {
    $logFiles = Get-ChildItem $logDir -Filter "*.log"
    $totalSize = ($logFiles | Measure-Object Length -Sum).Sum
    
    Write-Host "  日志文件数：$($logFiles.Count)" -ForegroundColor Gray
    Write-Host "  总大小：$([math]::Round($totalSize / 1KB, 2)) KB" -ForegroundColor Gray
    
    # 最新日志
    $latest = $logFiles | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latest) {
        Write-Host "  最新日志：$($latest.Name) ($([math]::Round($latest.Length / 1KB, 2)) KB)" -ForegroundColor Gray
        
        # 统计行数
        $lineCount = (Get-Content $latest.FullName | Measure-Object -Line).Lines
        Write-Host "  日志行数：$lineCount" -ForegroundColor Gray
    }
} else {
    Write-Host "  日志目录不存在" -ForegroundColor Red
}
Write-Host ""

# 配置文件
Write-Host "配置文件:" -ForegroundColor Yellow
$configPath = "C:\ProgramData\wftpg\config.toml"
if (Test-Path $configPath) {
    $config = Get-Item $configPath
    Write-Host "  大小：$([math]::Round($config.Length / 1KB, 2)) KB" -ForegroundColor Gray
    Write-Host "  修改时间：$($config.LastWriteTime)" -ForegroundColor Gray
} else {
    Write-Host "  配置文件不存在" -ForegroundColor Red
}
Write-Host ""

# 内存警告
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "内存健康检查" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

$workingSetMB = [math]::Round($process.WorkingSet / 1MB, 2)
$privateMB = [math]::Round($process.PrivateMemorySize64 / 1MB, 2)

Write-Host "关键指标解读:" -ForegroundColor Yellow
Write-Host "  • 工作集 (WS): OS 认为进程'最近使用'的物理内存" -ForegroundColor DarkGray
Write-Host "  • 私有内存：进程已分配的所有内存（含未使用）" -ForegroundColor DarkGray
Write-Host "  • 稳定期 WS 在 15-30 MB 属于正常优秀水平 ✅" -ForegroundColor DarkGray
Write-Host ""

# 判断标准：关注稳定期而非启动峰值
if ($workingSetMB -gt 100) {
    Write-Host "⚠️  注意：工作集偏高 (>100 MB)" -ForegroundColor Yellow
    Write-Host "   如果是刚启动 (<5 分钟)，属于正常现象" -ForegroundColor DarkGray
    Write-Host "   如果已运行 >10 分钟，建议持续监控" -ForegroundColor DarkGray
} elseif ($workingSetMB -gt 50) {
    Write-Host "✓ 良好：工作集合理 (50-100 MB)" -ForegroundColor Green
    Write-Host "   可能是启动初期，会自然下降" -ForegroundColor DarkGray
} elseif ($workingSetMB -gt 25) {
    Write-Host "✓✓ 优秀：工作集很低 (25-50 MB)" -ForegroundColor Green
    Write-Host "   程序运行状态健康" -ForegroundColor DarkGray
} else {
    Write-Host "✓✓✓ 极佳：工作集非常低 (<25 MB)" -ForegroundColor Green
    Write-Host "   这是 Rust + egui 应用的理想水平！🎉" -ForegroundColor DarkGray
}

Write-Host ""
Write-Host "重要提示:" -ForegroundColor Yellow
Write-Host "  • 启动时 150 MB 是内存预留，不是真实占用" -ForegroundColor DarkGray
Write-Host "  • 稳定期 20 MB 左右是完全正常的 ✅" -ForegroundColor DarkGray
Write-Host "  • 关键是看是否持续增长（泄漏）" -ForegroundColor DarkGray

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "建议的诊断命令" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

Write-Host "# 实时监控内存变化" -ForegroundColor White
Write-Host "Get-Process wftpg | Select-Object WorkingSet,CPU,Threads -Wait" -ForegroundColor DarkGray
Write-Host ""

Write-Host "# 长时间监控（检测泄漏）" -ForegroundColor White
Write-Host "1..60 | ForEach-Object {" -ForegroundColor DarkGray
Write-Host "  $p = Get-Process wftpg -ea SilentlyContinue" -ForegroundColor DarkGray
Write-Host "  if ($p) { \"$(Get-Date -Format 'HH:mm:ss'),$($p.WS/1MB),$($p.Private/1MB)\" }" -ForegroundColor DarkGray
Write-Host "  Start-Sleep -Seconds 60" -ForegroundColor DarkGray
Write-Host "} | Out-File memory_trend.csv" -ForegroundColor DarkGray
Write-Host ""

Write-Host "# 强制 GC（仅调试用）" -ForegroundColor White
Write-Host "[System.GC]::Collect()" -ForegroundColor DarkGray
Write-Host ""

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "详细分析请参考:" -ForegroundColor Cyan
Write-Host "MEMORY_ANALYSIS_REPORT.md" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
