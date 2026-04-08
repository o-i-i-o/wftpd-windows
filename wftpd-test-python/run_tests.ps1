# WFTPD Python 测试套件 - PowerShell版本
# Requires: PowerShell 5.1+

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "WFTPD FTP/SFTP Python 测试套件" -ForegroundColor Cyan  
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# 检查Python是否安装
try {
    $pythonVersion = python --version 2>&1
    Write-Host "检测到Python环境: $pythonVersion" -ForegroundColor Green
} catch {
    Write-Host "错误: 未找到Python，请先安装Python 3.6+" -ForegroundColor Red
    Read-Host "按回车键退出"
    exit 1
}

Write-Host ""

# 检查依赖
Write-Host "检查依赖包..." -ForegroundColor Yellow
try {
    pip show paramiko | Out-Null
    Write-Host "依赖包已安装" -ForegroundColor Green
} catch {
    Write-Host "安装依赖包..." -ForegroundColor Yellow
    try {
        pip install -r requirements.txt
        Write-Host "依赖包安装成功" -ForegroundColor Green
    } catch {
        Write-Host "错误: 依赖安装失败" -ForegroundColor Red
        Read-Host "按回车键退出"
        exit 1
    }
}

Write-Host ""

# 创建测试数据目录
if (!(Test-Path "testdata")) {
    New-Item -ItemType Directory -Path "testdata" | Out-Null
    Write-Host "创建测试数据目录" -ForegroundColor Green
}

# 运行测试
Write-Host "开始执行测试..." -ForegroundColor Yellow
Write-Host ""

$startTime = Get-Date

try {
    python wftpd_test.py $args
    $testResult = $LASTEXITCODE
} catch {
    Write-Host "测试执行出错: $_" -ForegroundColor Red
    $testResult = 1
}

$endTime = Get-Date
$duration = ($endTime - $startTime).TotalSeconds

Write-Host ""
if ($testResult -eq 0) {
    Write-Host "测试完成: 所有测试通过 ✓" -ForegroundColor Green
} else {
    Write-Host "测试完成: 存在失败的测试 ✗" -ForegroundColor Red
}

Write-Host "总耗时: $([math]::Round($duration, 2)) 秒" -ForegroundColor Cyan
Write-Host ""
Write-Host "查看详细报告: test_report.json" -ForegroundColor Blue
Write-Host "查看日志文件: test_results.log" -ForegroundColor Blue
Write-Host ""

Read-Host "按回车键退出"
exit $testResult