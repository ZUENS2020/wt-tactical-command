@echo off
cd /d "%~dp0"
wt-collector.exe diag
echo.
echo (diag output is shown above only; nothing is saved. Select + right-click to copy it.)
pause
