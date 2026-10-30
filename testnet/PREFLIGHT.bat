@echo off
rem PREFLIGHT.bat - read-only check of tools, WASM files and network. Creates nothing, signs nothing.
setlocal
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0\0-check-tools.ps1"
set RC=%ERRORLEVEL%
pause
exit /b %RC%
