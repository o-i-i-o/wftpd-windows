@echo off
setlocal EnableDelayedExpansion

:: ========================================
:: WFTPG 卸载脚本
:: 功能：请求管理员权限、停止服务、卸载服务、停止程序、删除文件、删除快捷方式
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
echo   WFTPG 卸载程序
echo ========================================
echo.

:: 定义路径
set INSTALL_DIR=C:\Program Files\wftpg
set WFTPG_EXE=wftpg.exe
set WFTPD_EXE=wftpd.exe
set SERVICE_NAME=wftpd
set DESKTOP_SHORTCUT=%USERPROFILE%\Desktop\WFTPG.lnk

:: 步骤 1: 检查安装
echo [1/6] 检查安装状态...
if not exist "%INSTALL_DIR%" (
    echo   路径：%INSTALL_DIR%
    pause
    exit /b 1
)
echo  检测到 WFTPG 安装
echo.

:: 步骤 2: 停止 WFTPD 服务
echo [2/6] 停止 WFTPD 服务...
sc query %SERVICE_NAME% >nul 2>&1
if %errorLevel% equ 0 (
    sc query %SERVICE_NAME% | find "RUNNING" >nul 2>&1
    if %errorLevel% equ 0 (
        echo   正在停止服务 %SERVICE_NAME%...
        net stop %SERVICE_NAME% /y >nul 2>&1
        if errorlevel 1 (
            echo  警告：停止服务失败，尝试强制停止
            taskkill /F /IM %WFTPD_EXE% >nul 2>&1
        )
        
        :: 等待服务完全停止
        timeout /t 2 /nobreak >nul
        
        :: 验证服务已停止
        sc query %SERVICE_NAME% | find "STOPPED" >nul 2>&1
        if %errorLevel% equ 0 (
            echo  服务已停止
        ) else (
            echo  警告：服务可能仍在运行
        )
    ) else (
        echo  服务未运行，跳过停止
    )
) else (
    echo  服务未安装，跳过停止
)
echo.

:: 步骤 3: 卸载 WFTPD 服务
echo [3/6] 卸载 WFTPD 服务...
sc query %SERVICE_NAME% >nul 2>&1
if %errorLevel% equ 0 (
    echo   正在删除服务 %SERVICE_NAME%...
    sc delete %SERVICE_NAME% >nul 2>&1
    if errorlevel 1 (
        echo  警告：删除服务失败
        pause
    ) else (
        echo  服务已删除
    )
) else (
    echo  服务不存在，跳过卸载
)
echo.

:: 步骤 4: 停止 WFTPG 程序
echo [4/6] 停止 WFTPG 程序...
tasklist | find "%WFTPG_EXE%" >nul 2>&1
if %errorLevel% equ 0 (
    echo   正在关闭 WFTPG 程序...
    taskkill /F /IM %WFTPG_EXE% >nul 2>&1
    if errorlevel 1 (
        echo  警告：无法关闭 WFTPG 程序
    ) else (
        echo  WFTPG 程序已关闭
    )
) else (
    echo  WFTPG 程序未运行，跳过停止
)
echo.

:: 步骤 5: 删除桌面快捷方式
echo [5/6] 删除桌面快捷方式...
if exist "%DESKTOP_SHORTCUT%" (
    del /Q "%DESKTOP_SHORTCUT%" >nul 2>&1
    if errorlevel 1 (
        echo  警告：无法删除桌面快捷方式
    ) else (
        echo  已删除桌面快捷方式
    )
) else (
    echo  桌面快捷方式不存在，跳过删除
)
echo.

:: 步骤 6: 删除安装目录
echo [6/6] 删除安装目录...
if exist "%INSTALL_DIR%" (
    echo   正在删除 %INSTALL_DIR%...
    
    :: 先尝试删除 wftpg.exe
    if exist "%INSTALL_DIR%\%WFTPG_EXE%" (
        del /Q "%INSTALL_DIR%\%WFTPG_EXE%" >nul 2>&1
    )
    
    :: 再尝试删除 wftpd.exe
    if exist "%INSTALL_DIR%\%WFTPD_EXE%" (
        del /Q "%INSTALL_DIR%\%WFTPD_EXE%" >nul 2>&1
    )
    
    :: 删除整个目录
    rmdir /S /Q "%INSTALL_DIR%"
    if errorlevel 1 (
        echo  警告：无法完全删除安装目录
        echo   可能有文件正在使用，请手动删除：%INSTALL_DIR%
    ) else (
        echo  已删除安装目录
    )
) else (
    echo  安装目录不存在，跳过删除
)
echo.

:: 完成
echo ========================================
echo   卸载完成！
echo ========================================
echo.
echo  WFTPG 已从系统中移除
echo.
echo 以下项目已被删除:
echo   ? WFTPD Windows 服务
echo   ? WFTPG 程序文件
echo   ? 桌面快捷方式
echo   ? 安装目录：%INSTALL_DIR%
echo.
echo 注意：配置文件保留在 C:\ProgramData\wftpg\
echo      如需完全清理，请手动删除该目录
echo.
exit