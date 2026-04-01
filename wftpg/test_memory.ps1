# WFTPG 内存优化验证脚本
# 用于快速测试内存占用和性能表现

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "WFTPG 内存优化验证脚本" -ForegroundColor Cyan
Write-Host "版本：v3.2.6 (优化版)" -ForegroundColor Cyan
Write-Host "日期：$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# 检查 Release 版本是否存在
$releasePath = ".\target\release\wftpg.exe"
if (-not (Test-Path $releasePath)) {
    Write-Host "错误：未找到 Release 版本，请先编译" -ForegroundColor Red
    Write-Host "运行：cargo build --release" -ForegroundColor Yellow
    exit 1
}

Write-Host "✓ 找到 Release 版本" -ForegroundColor Green
Write-Host ""

# 显示优化参数
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "优化参数配置" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "日志文件：" -NoNewline
Write-Host " src\gui_egui\log_tab.rs" -ForegroundColor Gray
Write-Host "文件日志：" -NoNewline
Write-Host " src\gui_egui\file_log_tab.rs" -ForegroundColor Gray
Write-Host ""

Write-Host "常量配置:" -ForegroundColor Yellow
Write-Host "  MAX_DISPLAY_LOGS:     500 条 (优化前：2000) ⬇ 75%" -ForegroundColor Green
Write-Host "  INITIAL_FETCH_COUNT:  100 条 (优化前：200)  ⬇ 50%" -ForegroundColor Green
Write-Host "  INCREMENTAL_READ_SIZE: 20 条 (优化前：50)   ⬇ 60%" -ForegroundColor Green
Write-Host "  MAX_EVENTS_PER_FRAME:  5 个/帧 (新增限制)" -ForegroundColor Green
Write-Host ""

# 预期效果
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "预期优化效果" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "内存占用：⬇ 40-50%  (150-200MB → 80-120MB)" -ForegroundColor Green
Write-Host "启动速度：⬆ 50%     (200ms → 100ms)" -ForegroundColor Green
Write-Host "Tab 切换：  ⬆ 60%     (50ms → 20ms)" -ForegroundColor Green
Write-Host "长时间运行：内存增长可控 (< 150MB/小时)" -ForegroundColor Green
Write-Host ""

# 提示用户
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "测试步骤" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "1. 手动启动应用程序:" -ForegroundColor Yellow
Write-Host "   .\target\release\wftpg.exe" -ForegroundColor Gray
Write-Host ""
Write-Host "2. 打开任务管理器 (Ctrl+Shift+Esc)" -ForegroundColor Yellow
Write-Host "3. 找到 'WFTPG - SFTP/FTP 管理工具' 进程" -ForegroundColor Yellow
Write-Host "4. 记录内存占用（工作集）" -ForegroundColor Yellow
Write-Host ""

Write-Host "预期结果:" -ForegroundColor Green
Write-Host "  ✓ 初始内存：80-120 MB" -ForegroundColor White
Write-Host "  ✓ Tab 切换流畅，无明显卡顿" -ForegroundColor White
Write-Host "  ✓ 日志加载迅速（<100ms）" -ForegroundColor White
Write-Host "  ✓ 运行 1 小时后内存 < 150 MB" -ForegroundColor White
Write-Host ""

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "功能验证清单" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

$checks = @(
    "应用程序正常启动",
    "所有标签页正常显示",
    "运行日志实时滚动",
    "文件日志实时更新",
    "新日志提示功能正常",
    "自动滚动到底部正常",
    "刷新按钮响应正常",
    "配置保存功能正常"
)

foreach ($check in $checks) {
    Write-Host "[ ] $check" -ForegroundColor White
}

Write-Host ""
Write-Host "请填写测试结果并记录内存数据" -ForegroundColor Yellow
Write-Host ""

# 询问用户是否开始测试
$response = Read-Host "是否现在启动应用程序进行测试？(y/n)"
if ($response -eq 'y' -or $response -eq 'Y') {
    Write-Host ""
    Write-Host "正在启动应用程序..." -ForegroundColor Green
    Start-Process $releasePath
    
    Write-Host ""
    Write-Host "提示:" -ForegroundColor Yellow
    Write-Host "1. 请在任务管理器中观察内存占用" -ForegroundColor White
    Write-Host "2. 测试各个功能是否正常" -ForegroundColor White
    Write-Host "3. 建议运行至少 30 分钟以验证稳定性" -ForegroundColor White
    Write-Host ""
    Write-Host "测试完成后，请关闭应用程序并查看内存释放情况" -ForegroundColor Yellow
} else {
    Write-Host ""
    Write-Host "稍后可以手动运行：.\target\release\wftpg.exe" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "详细测试指南请参考：" -ForegroundColor Cyan
Write-Host "MEMORY_OPTIMIZATION.md" -ForegroundColor Green
Write-Host "test_memory.sh (Bash 版本)" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
