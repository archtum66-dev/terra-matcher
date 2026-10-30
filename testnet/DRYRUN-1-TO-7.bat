@echo off
rem DRYRUN-1-TO-7.bat - shows all seven steps as commands without sending anything (-DryRun). Uses the prefix "dryrun" and its own state file.
setlocal
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-1-to-7.ps1" -Prefix dryrun -DryRun
set RC=%ERRORLEVEL%
pause
exit /b %RC%
