@echo off
setlocal
cd /d "%~dp0"
set "SA_REPORT_DIR=%~dp0reports\audit-%RANDOM%-%RANDOM%"
py -3 --version >nul 2>&1
if not errorlevel 1 (
  py -3 tools\inspect_installation.py --game-dir "E:\GTA San Andreas\Grand Theft Auto San Andreas" --output "%SA_REPORT_DIR%"
) else (
  python --version >nul 2>&1
  if errorlevel 1 (
    echo Python 3.10 or newer is required. No game files have been changed.
    pause
    exit /b 1
  )
  python tools\inspect_installation.py --game-dir "E:\GTA San Andreas\Grand Theft Auto San Andreas" --output "%SA_REPORT_DIR%"
)
if errorlevel 1 (
  echo Inspection did not complete. See the error above.
) else (
  echo Upload sa-installation-report.zip from the report folder shown above.
)
pause
