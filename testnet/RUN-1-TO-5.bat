@echo off
setlocal
set /p PREFIX=Enter account prefix (e.g. terra1): 
if "%PREFIX%"=="" (
  echo Prefix must not be empty.
  pause
  exit /b 1
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-1-to-5.ps1" -Prefix %PREFIX%
pause
