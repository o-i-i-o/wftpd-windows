@echo off
chcp 65001 >nul
setlocal enabledelayedexpansion

set "ERROR_COUNT=0"
set "TOTAL_CHECKS=0"

echo ========================================
echo   WFTPG 发布前检查
echo ========================================
echo.

goto :main

:: ========== 函数定义 ==========
:run_check
set "CHECK_NAME=%~1"
set "CHECK_CMD=%~2"
set /a TOTAL_CHECKS+=1

echo [!TOTAL_CHECKS!] 正在执行: !CHECK_NAME!...
call %CHECK_CMD%
if errorlevel 1 (
    echo   ❌ 失败: !CHECK_NAME!
    set /a ERROR_COUNT+=1
) else (
    echo   ✅ 通过: !CHECK_NAME!
)
echo.
exit /b 0

:main

:: ========== wftpd 项目检查 ==========
echo ────────────────────────────────────
echo   项目: wftpd (FTP/SFTP 服务端)
echo ────────────────────────────────────
cd /d "%~dp0wftpd" || goto :error_cd

if not exist "Cargo.toml" (
    echo   ❌ 错误: 未找到 Cargo.toml，当前目录: %CD%
    set /a ERROR_COUNT+=1
    set /a TOTAL_CHECKS+=1
    goto :check_wftpg
)

:: 先格式化代码
echo [准备] 正在格式化代码...
cargo fmt >nul 2>&1
if errorlevel 1 (
    echo   ❌ 代码格式化失败
    exit /b 1
) else (
    echo   ✅ 代码格式化完成
)
echo.

call :run_check "代码格式检查" "cargo fmt --check"
call :run_check "Clippy 静态分析" "cargo clippy --release -- -D warnings"
call :run_check "编译检查" "cargo check --release"
call :run_check "Release 构建" "cargo build --release"

:check_wftpg
:: ========== wftpg 项目检查 ==========
echo ────────────────────────────────────
echo   项目: wftpg (GUI 客户端)
echo ────────────────────────────────────
cd /d "%~dp0wftpg" || goto :error_cd

if not exist "Cargo.toml" (
    echo   ❌ 错误: 未找到 Cargo.toml，当前目录: %CD%
    set /a ERROR_COUNT+=1
    set /a TOTAL_CHECKS+=1
    goto :summary
)

:: 先格式化代码
echo [准备] 正在格式化代码...
cargo fmt >nul 2>&1
if errorlevel 1 (
    echo   ❌ 代码格式化失败
    exit /b 1
) else (
    echo   ✅ 代码格式化完成
)
echo.

call :run_check "代码格式检查" "cargo fmt --check"
call :run_check "Clippy 静态分析" "cargo clippy --release -- -D warnings"
call :run_check "编译检查" "cargo check --release"
call :run_check "Release 构建" "cargo build --release"

:summary
:: ========== 总结 ==========
echo ========================================
echo   检查结果汇总
echo ========================================
echo   总检查数: !TOTAL_CHECKS!
echo   通过: !TOTAL_CHECKS! - !ERROR_COUNT!
echo   失败: !ERROR_COUNT!
echo.

if !ERROR_COUNT! equ 0 (
    echo   🎉 所有检查通过！可以发布。
    echo ========================================
    endlocal
    exit /b 0
) else (
    echo   ⚠️  发现 !ERROR_COUNT! 个失败项，请修复后重试。
    echo ========================================
    endlocal
    exit /b 1
)

:error_cd
echo   ❌ 错误: 无法切换到项目目录
echo   预期目录: %~dp0wftpd 或 %~dp0wftpg
echo ========================================
endlocal
exit /b 1
