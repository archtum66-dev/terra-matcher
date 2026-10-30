@echo off
rem RUN-1-TO-7.bat - double-click: preflight, then the seven testnet steps (accounts, WASM, deploy, keys, deposit, settle, withdraw).
rem Testnet only. The scripts refuse mainnet.
setlocal
cd /d "%~dp0"
echo.
echo Step 0: preflight (read-only: tools, WASM files, network)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0\0-check-tools.ps1"
if errorlevel 1 (
  echo.
  echo Preflight failed. Fix the lines marked FAIL above, then start this file again.
  pause
  exit /b 1
)
echo.
set /p PREFIX=Enter account prefix (lowercase letters, digits, hyphen; e.g. ag2610): 
if "%PREFIX%"=="" (
  echo Prefix must not be empty.
  pause
  exit /b 1
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-1-to-7.ps1" -Prefix %PREFIX%
set RC=%ERRORLEVEL%
echo.
if "%RC%"=="0" (
  echo Finished. Results: testnet\results\summary.md   Logs: testnet\logs
) else (
  echo Stopped with exit code %RC%. See the messages above and testnet\logs. A second start skips what is already done.
)
pause
exit /b %RC%
