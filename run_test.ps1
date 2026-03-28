# WFTPG FTP/SFTP 测试脚本启动器 (PowerShell 版本)

Write-Host "================================================" -ForegroundColor Cyan
Write-Host "WFTPG FTP/SFTP 测试脚本启动器" -ForegroundColor Cyan
Write-Host "================================================" -ForegroundColor Cyan
Write-Host ""

# 检查 Python 是否安装
try {
    $pythonVersion = python --version 2>&1
    Write-Host "[信息] Python 已安装：$pythonVersion" -ForegroundColor Green
} catch {
    Write-Host "[错误] 未检测到 Python，请先安装 Python 3.8+" -ForegroundColor Red
    Read-Host "按回车键退出"
    exit 1
}

Write-Host ""

# 检查并安装依赖
Write-Host "[信息] 检查 Python 依赖..." -ForegroundColor Yellow
try {
    Import-Module paramiko -ErrorAction Stop
    Write-Host "[信息] Python 依赖已安装" -ForegroundColor Green
} catch {
    Write-Host "[信息] 正在安装 paramiko..." -ForegroundColor Yellow
    try {
        pip install -r requirements-test.txt
        Write-Host "[信息] 依赖安装成功" -ForegroundColor Green
    } catch {
        Write-Host "[错误] 依赖安装失败" -ForegroundColor Red
        Read-Host "按回车键退出"
        exit 1
    }
}

Write-Host ""

# 检查 wftpd.exe 是否存在
$wftpdRelease = "target\release\wftpd.exe"
$wftpdDebug = "target\debug\wftpd.exe"

if (Test-Path $wftpdRelease) {
    Write-Host "[信息] 找到 wftpd.exe (release 版本): $wftpdRelease" -ForegroundColor Green
} elseif (Test-Path $wftpdDebug) {
    Write-Host "[信息] 找到 wftpd.exe (debug 版本): $wftpdDebug" -ForegroundColor Yellow
} else {
    Write-Host "[警告] 未找到 wftpd.exe" -ForegroundColor Red
    Write-Host "[提示] 建议先运行：cargo build --release" -ForegroundColor Yellow
    Write-Host ""
}

Write-Host ""
Write-Host "================================================" -ForegroundColor Cyan
Write-Host "开始运行测试" -ForegroundColor Cyan
Write-Host "================================================" -ForegroundColor Cyan
Write-Host ""

# 运行测试脚本
try {
    python test_ftp_sftp.py
} catch {
    Write-Host ""
    Write-Host "[错误] 测试脚本执行失败：$_" -ForegroundColor Red
}

Write-Host ""
Write-Host "================================================" -ForegroundColor Cyan
Write-Host "测试完成" -ForegroundColor Cyan
Write-Host "================================================" -ForegroundColor Cyan
Write-Host ""

# 显示测试结果文件（如果存在）
$resultFile = "test_result.json"
if (Test-Path $resultFile) {
    Write-Host "[信息] 测试结果已保存到：$resultFile" -ForegroundColor Green
    Write-Host ""
    
    # 询问是否查看结果
    $viewResult = Read-Host "是否查看测试结果摘要？(Y/N)"
    if ($viewResult -eq 'Y' -or $viewResult -eq 'y') {
        try {
            $result = Get-Content $resultFile | ConvertFrom-Json
            
            Write-Host ""
            Write-Host "=== 测试结果摘要 ===" -ForegroundColor Cyan
            Write-Host "总测试数： $($result.summary.total_tests)" -ForegroundColor White
            Write-Host "通过：     $($result.summary.total_passed)" -ForegroundColor Green
            Write-Host "失败：     $($result.summary.total_failed)" -ForegroundColor $(if ($result.summary.total_failed -gt 0) { "Red" } else { "Green" })
            Write-Host "成功率：   $($result.summary.success_rate)" -ForegroundColor White
            
            if ($result.ftp) {
                Write-Host ""
                Write-Host "FTP 测试:" -ForegroundColor Cyan
                Write-Host "  测试数：$($result.ftp.total)" -ForegroundColor White
                Write-Host "  通过：  $($result.ftp.passed)" -ForegroundColor Green
                Write-Host "  失败：  $($result.ftp.failed)" -ForegroundColor $(if ($result.ftp.failed -gt 0) { "Red" } else { "Green" })
                Write-Host "  成功率：$($result.ftp.success_rate)" -ForegroundColor White
            }
            
            if ($result.sftp) {
                Write-Host ""
                Write-Host "SFTP 测试:" -ForegroundColor Cyan
                Write-Host "  测试数：$($result.sftp.total)" -ForegroundColor White
                Write-Host "  通过：  $($result.sftp.passed)" -ForegroundColor Green
                Write-Host "  失败：  $($result.sftp.failed)" -ForegroundColor $(if ($result.sftp.failed -gt 0) { "Red" } else { "Green" })
                Write-Host "  成功率：$($result.sftp.success_rate)" -ForegroundColor White
            }
            
            if ($result.ftp.errors -or $result.sftp.errors) {
                Write-Host ""
                Write-Host "错误详情:" -ForegroundColor Yellow
                $allErrors = $result.ftp.errors + $result.sftp.errors
                foreach ($error in $allErrors) {
                    Write-Host "  ✗ $($error.test): $($error.reason)" -ForegroundColor Red
                }
            }
        } catch {
            Write-Host "[警告] 无法解析测试结果文件" -ForegroundColor Yellow
        }
    }
} else {
    Write-Host "[警告] 未找到测试结果文件" -ForegroundColor Yellow
}

Write-Host ""
Read-Host "按回车键退出"
