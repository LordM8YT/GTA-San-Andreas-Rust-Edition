@echo off
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if not errorlevel 1 (
  echo Bygger native Rust-runtime...
  cargo build --manifest-path "%~dp0native\Cargo.toml" --release -p sa-runtime
  if errorlevel 1 (
    echo Bygging feilet. Installer Rust/Cargo og prov igjen.
    pause
    exit /b 1
  )
) else if not exist "%~dp0native\target\release\sa-runtime.exe" (
  echo Installer Rust/Cargo for aa bygge native runtime.
  pause
  exit /b 1
)
"%~dp0native\target\release\sa-runtime.exe" %*
if errorlevel 1 (
  echo Rust-runtime avsluttet med en feil.
  pause
  exit /b 1
)
