@echo off
chcp 65001 >nul
echo ========================================
echo WFTPD FTP/SFTP Test Suite
echo ========================================
echo.
echo Starting tests...
echo.

wftpd_test.exe %*

echo.
echo Tests completed.
pause
