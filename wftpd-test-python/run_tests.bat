@echo off
chcp 65001 >nul
title WFTPD Python 测试套件

echo ========================================
echo WFTPD FTP/SFTP Python 测试套件
echo ========================================
echo.

REM 检查Python是否安装
python --version >nul 2>&1
if %errorlevel% neq 0 (
    echo 错误: 未找到Python，请先安装Python 3.6+
    pause
    exit /b 1
)

echo 检测到Python环境
python --version
echo.

REM 检查依赖
echo 检查依赖包...
pip show paramiko >nul 2>&1
if %errorlevel% neq 0 (
    echo 安装依赖包...
    pip install -r requirements.txt
    if %errorlevel% neq 0 (
        echo 错误: 依赖安装失败
        pause
        exit /b 1
    )
) else (
    echo 依赖包已安装
)
echo.

REM 创建测试数据目录
if not exist testdata mkdir testdata

REM 运行测试
echo 开始执行测试...
echo.
python wftpd_test.py %*

set TEST_RESULT=%errorlevel%

echo.
if %TEST_RESULT% equ 0 (
    echo 测试完成: 所有测试通过 ✓
) else (
    echo 测试完成: 存在失败的测试 ✗
)

echo.
echo 查看详细报告: test_report.json
echo 查看日志文件: test_results.log
echo.

pause
exit /b %TEST_RESULT%