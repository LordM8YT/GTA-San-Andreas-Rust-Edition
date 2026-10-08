@echo off
setlocal
cd /d "%~dp0"
if exist "sa-launcher.exe" (
  start "" "sa-launcher.exe"
  exit /b 0
)
if exist "native\target\release\sa-launcher.exe" (
  start "" "native\target\release\sa-launcher.exe"
  exit /b 0
)
echo SARE launcher is missing. Extract the complete client package.
echo Developers: cargo build --release --workspace --manifest-path native/Cargo.toml
pause
exit /b 1
