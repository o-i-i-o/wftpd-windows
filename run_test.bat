@echo off
chcp 65001 >nul
echo ================================================
echo WFTPG FTP/SFTP 测试脚本启动器
echo ================================================
echo.

REM 检查 Python 是否安装
python --version >nul 2>&1
if %errorlevel% neq 0 (
    echo [错误] 未检测到 Python，请先安装 Python 3.8+
    pause
    exit /b 1
)

echo [信息] Python 已安装
echo.

REM 检查依赖是否安装
echo [信息] 检查 Python 依赖...
python -c "import paramiko" >nul 2>&1
if %errorlevel% neq 0 (
    echo [信息] 正在安装 paramiko...
    pip install -r requirements-test.txt
    if %errorlevel% neq 0 (
        echo [错误] 依赖安装失败
        pause
        exit /b 1
    )
) else (
    echo [信息] Python 依赖已安装
)
echo.

REM 检查 wftpd.exe 是否存在
if exist "target\release\wftpd.exe" (
    echo [信息] 找到 wftpd.exe (release 版本)
) else if exist "target\debug\wftpd.exe" (
    echo [信息] 找到 wftpd.exe (debug 版本)
) else (
    echo [警告] 未找到 wftpd.exe
    echo [提示] 建议先运行：cargo build --release
    echo.
)

echo ================================================
echo 开始运行测试
echo ================================================
echo.

REM 运行测试脚本
python test_ftp_sftp.py

echo.
echo ================================================
echo 测试完成
echo ================================================
pause
