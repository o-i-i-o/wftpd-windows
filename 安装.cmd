@echo off
chcp 65001 >nul 2>&1
setlocal EnableDelayedExpansion

:: ========================================
:: WFTPG 安装脚本
:: 功能：请求管理员权限、创建目录、复制文件、创建快捷方式、启动程序
:: ========================================

:: 检查是否以管理员权限运行
net session >nul 2>&1
if %errorLevel% neq 0 (
    echo ========================================
    echo   请求管理员权限...
    echo ========================================
    echo.
    
    :: 使用 PowerShell 请求提升权限
    powershell -Command "Start-Process cmd -ArgumentList '/c', '%~dpnx0' -Verb RunAs"
    exit /b
)

echo ========================================
echo   WFTPG 安装程序
echo ========================================
echo.

:: 获取脚本所在目录
set SCRIPT_DIR=%~dp0

:: 定义目标路径
set INSTALL_DIR=C:\Program Files\wftpg\
set WFTPG_EXE=wftpg.exe
set WFTPD_EXE=wftpd.exe
set DESKTOP_SHORTCUT=%USERPROFILE%\Desktop\WFTPG.lnk

:: 检查源文件是否存在
echo [1/5] 检查安装文件...
if not exist "%SCRIPT_DIR%%WFTPG_EXE%" (
    echo  错误：找不到 %WFTPG_EXE%
    echo   路径：%SCRIPT_DIR%%WFTPG_EXE%
    pause
    exit /b 1
)
if not exist "%SCRIPT_DIR%%WFTPD_EXE%" (
    echo  错误：找不到 %WFTPD_EXE%
    echo   路径：%SCRIPT_DIR%%WFTPD_EXE%
    pause
    exit /b 1
)
echo  安装文件检查通过
echo.

:: 创建安装目录
echo [2/5] 创建安装目录...
if not exist "%INSTALL_DIR%" (
    mkdir "%INSTALL_DIR%"
    if errorlevel 1 (
        echo  错误：无法创建目录 %INSTALL_DIR%
        pause
        exit /b 1
    )
    echo  已创建目录：%INSTALL_DIR%
) else (
    echo  目录已存在：%INSTALL_DIR%
)
echo.

:: 复制 wftpg.exe
echo [3/5] 复制程序文件...
echo   正在复制 %WFTPG_EXE%...
copy /Y "%SCRIPT_DIR%%WFTPG_EXE%" "%INSTALL_DIR%%WFTPG_EXE%"
if errorlevel 1 (
    echo  错误：无法复制 %WFTPG_EXE%
    pause
    exit /b 1
)
echo  已复制：%WFTPG_EXE%

:: 复制 wftpd.exe
echo   正在复制 %WFTPD_EXE%...
copy /Y "%SCRIPT_DIR%%WFTPD_EXE%" "%INSTALL_DIR%%WFTPD_EXE%"
if errorlevel 1 (
    echo  错误：无法复制 %WFTPD_EXE%
    pause
    exit /b 1
)
echo  已复制：%WFTPD_EXE%
echo.

:: 创建桌面快捷方式
echo [4/5] 创建桌面快捷方式...
powershell -Command "$WshShell = New-Object -ComObject WScript.Shell; $Shortcut = $WshShell.CreateShortcut('%DESKTOP_SHORTCUT%'); $Shortcut.TargetPath = '%INSTALL_DIR%\%WFTPG_EXE%'; $Shortcut.WorkingDirectory = '%INSTALL_DIR%'; $Shortcut.IconLocation = '%INSTALL_DIR%\%WFTPG_EXE%,0'; $Shortcut.Description = 'WFTPG FTP/SFTP Server Manager'; $Shortcut.Save()"
if errorlevel 1 (
    echo   警告：无法创建桌面快捷方式
    echo   可以手动创建快捷方式指向：%INSTALL_DIR%%WFTPG_EXE%
) else (
    echo  已创建桌面快捷方式：%DESKTOP_SHORTCUT%
)
echo.

:: 启动程序
echo [5/5] 启动WFTPG-管理员权限...
:: powershell -Command "Start-Process '%INSTALL_DIR%%WFTPG_EXE%' -Verb RunAs"
start "" "%INSTALL_DIR%\%WFTPG_EXE%"
if errorlevel 1 (
    echo   警告：无法自动启动程序
    echo   可以手动运行：%INSTALL_DIR%%WFTPG_EXE%
    echo   提示：右键选择"以管理员身份运行"
) else (
    echo   程序已启动
)
echo.

:: 完成
echo ========================================
echo   安装完成！
echo ========================================
echo.
echo 安装位置：%INSTALL_DIR%
echo 桌面快捷方式：%DESKTOP_SHORTCUT%
echo.
echo WFTPG已经安装并启动
echo.
exit
