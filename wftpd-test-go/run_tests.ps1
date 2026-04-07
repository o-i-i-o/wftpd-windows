# WFTPD FTP/SFTP Test Runner
# This script sets UTF-8 encoding before running tests

# Set console output encoding to UTF-8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# Change code page to UTF-8
chcp 65001 | Out-Null

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "WFTPD FTP/SFTP Test Suite" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# Run the test executable with all passed arguments
$exitCode = 0
try {
    & .\wftpd_test.exe $args
    $exitCode = $LASTEXITCODE
} catch {
    Write-Host "Error running tests: $_" -ForegroundColor Red
    $exitCode = 1
}

Write-Host ""
Write-Host "Tests completed with exit code: $exitCode" -ForegroundColor $(if ($exitCode -eq 0) { "Green" } else { "Yellow" })

exit $exitCode
