@echo off
setlocal
cd /d "%~dp0native"
cargo build --release -p sa-runtime
if errorlevel 1 (
  echo Native bygging feilet. Se feilmeldingen over.
  pause
  exit /b 1
)
echo Ferdig: %~dp0native\target\release\sa-runtime.exe
pause
