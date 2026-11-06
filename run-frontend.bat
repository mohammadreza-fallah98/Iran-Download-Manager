@echo off
setlocal

set SOURCE_DIR=%~dp0windows-form

echo Starting Iran Download Manager (dev mode)...
echo.
cd /d "%SOURCE_DIR%"
cargo run
if errorlevel 1 (
    echo ERROR: cargo run failed.
    pause
    exit /b 1
)
