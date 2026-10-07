@echo off
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if not errorlevel 1 (
  cargo build --manifest-path "%~dp0native\Cargo.toml" --release -p sa-server
  if errorlevel 1 exit /b 1
) else if not exist "%~dp0native\target\release\sa-server.exe" (
  echo Installer Rust/Cargo for aa bygge serveren.
  pause
  exit /b 1
)
"%~dp0native\target\release\sa-server.exe" %*
