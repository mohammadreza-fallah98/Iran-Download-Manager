@echo off
setlocal EnableDelayedExpansion

:: ── Config ────────────────────────────────────────────────────────────────
set APP_NAME=IranDownloadManager
set APP_VERSION=0.1.0
set SOURCE_DIR=%~dp0Frontend
set OUT_DIR=%~dp0Release\EXE

:: ── Date stamp ────────────────────────────────────────────────────────────
for /f "tokens=2 delims==" %%I in ('wmic os get localdatetime /value') do set DT=%%I
set DATE_STAMP=%DT:~0,8%
set OUT_FILE=%OUT_DIR%\%APP_NAME%-windows-%DATE_STAMP%-v%APP_VERSION%.exe

:: ── Build ─────────────────────────────────────────────────────────────────
echo [1/3] Building release binary...
cd /d "%SOURCE_DIR%"
cargo build --release
if errorlevel 1 (
    echo ERROR: cargo build failed.
    pause
    exit /b 1
)

:: ── Copy ──────────────────────────────────────────────────────────────────
echo [2/3] Copying output to Release\EXE\...
if not exist "%OUT_DIR%" mkdir "%OUT_DIR%"
copy /y "%SOURCE_DIR%\target\release\iran-download-manager.exe" "%OUT_FILE%"
if errorlevel 1 (
    echo ERROR: Copy failed.
    pause
    exit /b 1
)

:: ── Done ──────────────────────────────────────────────────────────────────
echo [3/3] Done.
echo   Output: %OUT_FILE%
echo.
pause
