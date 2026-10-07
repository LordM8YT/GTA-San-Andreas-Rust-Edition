@echo off
setlocal
cd /d "%~dp0"
set "SA_SAMPLE_DIR=%~dp0private-samples\first-%RANDOM%-%RANDOM%"
py -3 --version >nul 2>&1
if not errorlevel 1 (
  py -3 tools\export_first_sample.py --game-dir "E:\GTA San Andreas\Grand Theft Auto San Andreas" --output "%SA_SAMPLE_DIR%"
) else (
  python --version >nul 2>&1
  if errorlevel 1 (
    echo Python 3.10 or newer is required. No game files have been changed.
    pause
    exit /b 1
  )
  python tools\export_first_sample.py --game-dir "E:\GTA San Andreas\Grand Theft Auto San Andreas" --output "%SA_SAMPLE_DIR%"
)
if errorlevel 1 (
  echo Export did not complete. See the error above.
) else (
  echo Attach sa-first-model-private.zip from the folder shown above.
)
pause
