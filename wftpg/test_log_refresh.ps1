# 测试日志自动刷新功能
# 用于验证前端日志页面是否能够根据日志文件变化自动刷新

$ErrorActionPreference = "Stop"

# 日志目录
$logDir = "C:\ProgramData\wftpg\logs"

# 确保日志目录存在
Write-Host "创建日志目录：$logDir" -ForegroundColor Cyan
New-Item -ItemType Directory -Path $logDir -Force | Out-Null

# 生成测试日志文件
$logFile = Join-Path $logDir "wftpg-$(Get-Date -Format 'yyyy-MM-dd').log"
Write-Host "生成测试日志文件：$logFile" -ForegroundColor Cyan

# 初始日志内容
$initialLogs = @(
    '{"timestamp":"2026-04-01T10:00:00+08:00","level":"INFO","fields":{"message":"服务器启动","protocol":"FTP"}}',
    '{"timestamp":"2026-04-01T10:01:00+08:00","level":"INFO","fields":{"message":"用户登录","username":"test","client_ip":"192.168.1.100","protocol":"SFTP"}}'
)

Write-Host "写入初始日志..." -ForegroundColor Yellow
$initialLogs | Set-Content -Path $logFile -Encoding UTF8

Write-Host "`n=== 测试步骤 ===" -ForegroundColor Green
Write-Host "1. 启动 WFTPG GUI 程序" -ForegroundColor White
Write-Host "2. 切换到【运行日志】标签页" -ForegroundColor White
Write-Host "3. 等待 3 秒，让程序加载初始日志" -ForegroundColor White
Write-Host "4. 本脚本将在 5 秒后追加新日志..." -ForegroundColor White
Write-Host "5. 观察日志页面是否自动刷新显示新日志" -ForegroundColor White

Start-Sleep -Seconds 5

# 追加新日志
$newLogs = @(
    '{"timestamp":"2026-04-01T10:02:00+08:00","level":"INFO","fields":{"message":"文件上传成功","username":"test","client_ip":"192.168.1.100","protocol":"FTP"}}',
    '{"timestamp":"2026-04-01T10:03:00+08:00","level":"WARNING","fields":{"message":"连接超时","client_ip":"192.168.1.101","protocol":"SFTP"}}',
    '{"timestamp":"2026-04-01T10:04:00+08:00","level":"ERROR","fields":{"message":"认证失败","client_ip":"192.168.1.102","protocol":"FTP"}}'
)

Write-Host "`n>>> 追加新日志！" -ForegroundColor Green
$newLogs | Add-Content -Path $logFile -Encoding UTF8

Write-Host "新日志已写入，请检查 GUI 是否自动刷新显示" -ForegroundColor Yellow
Write-Host "如果日志页面没有自动刷新，请按【刷新】按钮手动刷新" -ForegroundColor Yellow

Start-Sleep -Seconds 2

# 再次追加日志
$moreLogs = @(
    '{"timestamp":"2026-04-01T10:05:00+08:00","level":"INFO","fields":{"message":"用户登出","username":"test","client_ip":"192.168.1.100","protocol":"FTP"}}'
)

Write-Host "`n>>> 再次追加日志！" -ForegroundColor Green
$moreLogs | Add-Content -Path $logFile -Encoding UTF8

Write-Host "`n=== 测试完成 ===" -ForegroundColor Green
Write-Host "请确认：" -ForegroundColor Yellow
Write-Host "✓ 新日志是否自动出现在日志列表中" -ForegroundColor White
Write-Host "✓ 日志级别颜色是否正确（INFO=绿色，WARN=橙色，ERROR=红色）" -ForegroundColor White
Write-Host "✓ 时间、协议、客户端 IP 等信息是否正确显示" -ForegroundColor White
