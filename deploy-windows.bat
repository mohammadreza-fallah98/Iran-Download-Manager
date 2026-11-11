@echo off
setlocal EnableDelayedExpansion

:: Builds the Windows app (EXE) and packages the browser extensions into Release\.
::   Release\EXE\IranDownloadManager.exe
::   Release\EXE\IranDownloadManager-windows-<date>-v<version>.exe
::   Release\Extensions\Chrome\   (unpacked, for "Load unpacked")
::   Release\Extensions\Firefox\  (unpacked, for about:debugging)
::   Release\Extensions\IranDownloadManager-chrome-v<ext version>.zip
::   Release\Extensions\IranDownloadManager-firefox-v<ext version>.xpi
:: Pass "nopause" to skip the final pause (for scripts).

:: -- Config ----------------------------------------------------------------
set APP_NAME=IranDownloadManager
set SOURCE_DIR=%~dp0windows-form
set EXT_DIR=%SOURCE_DIR%\extensions
set OUT_DIR=%~dp0Release\EXE
set EXT_OUT=%~dp0Release\Extensions

:: -- Versions (single source of truth: Cargo.toml and the extension manifests) --
for /f "tokens=2 delims==" %%V in ('findstr /b /c:"version = " "%SOURCE_DIR%\Cargo.toml"') do set RAW=%%V
set APP_VERSION=!RAW: =!
set APP_VERSION=!APP_VERSION:"=!
set RAW=
for /f "tokens=2 delims=:" %%V in ('findstr /c:"\"version\"" "%EXT_DIR%\chrome\manifest.json"') do set RAW=%%V
set CHROME_VERSION=!RAW: =!
set CHROME_VERSION=!CHROME_VERSION:"=!
set CHROME_VERSION=!CHROME_VERSION:,=!
set RAW=
for /f "tokens=2 delims=:" %%V in ('findstr /c:"\"version\"" "%EXT_DIR%\firefox\manifest.json"') do set RAW=%%V
set FIREFOX_VERSION=!RAW: =!
set FIREFOX_VERSION=!FIREFOX_VERSION:"=!
set FIREFOX_VERSION=!FIREFOX_VERSION:,=!
for /f %%D in ('powershell -NoProfile -Command "Get-Date -Format yyyyMMdd"') do set DATE_STAMP=%%D

if "%APP_VERSION%"=="" (
    echo ERROR: could not read the version from Cargo.toml.
    goto :fail
)
if "%CHROME_VERSION%"=="" (
    echo ERROR: could not read the Chrome extension version.
    goto :fail
)
if "%FIREFOX_VERSION%"=="" (
    echo ERROR: could not read the Firefox extension version.
    goto :fail
)

:: -- Stop a running copy so the exe can be replaced ------------------------
taskkill /f /im %APP_NAME%.exe >nul 2>&1
tasklist /fi "imagename eq iran-download-manager.exe" 2>nul | find /i "iran-download-manager.exe" >nul
if not errorlevel 1 (
    echo A running Iran Download Manager was found and will be closed.
    taskkill /f /im iran-download-manager.exe >nul 2>&1
    timeout /t 2 /nobreak >nul
)

:: -- Build the EXE ---------------------------------------------------------
echo [1/5] Running tests...
cd /d "%SOURCE_DIR%"
cargo test --release
if errorlevel 1 (
    echo ERROR: tests failed.
    goto :fail
)

echo [2/5] Building release binary v%APP_VERSION%...
cargo build --release
if errorlevel 1 (
    echo ERROR: cargo build failed.
    goto :fail
)

:: -- Copy the EXE ----------------------------------------------------------
echo [3/5] Copying the EXE to Release\EXE\...
if not exist "%OUT_DIR%" mkdir "%OUT_DIR%"
copy /y "%SOURCE_DIR%\target\release\iran-download-manager.exe" "%OUT_DIR%\%APP_NAME%.exe" >nul
if errorlevel 1 (
    echo ERROR: copying the EXE failed.
    goto :fail
)
copy /y "%SOURCE_DIR%\target\release\iran-download-manager.exe" "%OUT_DIR%\%APP_NAME%-windows-%DATE_STAMP%-v%APP_VERSION%.exe" >nul
if errorlevel 1 (
    echo ERROR: copying the versioned EXE failed.
    goto :fail
)

:: -- Extensions: unpacked copies -------------------------------------------
echo [4/5] Copying the extensions to Release\Extensions\...
for %%B in (Chrome Firefox) do (
    if exist "%EXT_OUT%\%%B" rmdir /s /q "%EXT_OUT%\%%B"
    mkdir "%EXT_OUT%\%%B"
)
xcopy /e /i /y /q "%EXT_DIR%\chrome\*" "%EXT_OUT%\Chrome\" >nul
if errorlevel 1 (
    echo ERROR: copying the Chrome extension failed.
    goto :fail
)
xcopy /e /i /y /q "%EXT_DIR%\firefox\*" "%EXT_OUT%\Firefox\" >nul
if errorlevel 1 (
    echo ERROR: copying the Firefox extension failed.
    goto :fail
)

:: -- Extensions: store packages (zip / xpi with manifest.json at the root) --
echo [5/5] Packaging the extensions...
set CHROME_PKG=%EXT_OUT%\%APP_NAME%-chrome-v%CHROME_VERSION%.zip
set FIREFOX_PKG=%EXT_OUT%\%APP_NAME%-firefox-v%FIREFOX_VERSION%.xpi
if exist "%CHROME_PKG%" del /q "%CHROME_PKG%"
if exist "%FIREFOX_PKG%" del /q "%FIREFOX_PKG%"
powershell -NoProfile -Command "$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.IO.Compression.FileSystem; foreach ($p in @(@('%EXT_DIR%\chrome','%CHROME_PKG%'), @('%EXT_DIR%\firefox','%FIREFOX_PKG%'))) { $zip=[IO.Compression.ZipFile]::Open($p[1],'Create'); try { Get-ChildItem -Path $p[0] -Recurse -File | ForEach-Object { $rel=$_.FullName.Substring($p[0].Length+1).Replace('\','/'); [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,$_.FullName,$rel,'Optimal') } } finally { $zip.Dispose() } }"
if errorlevel 1 (
    echo ERROR: packaging the extensions failed.
    goto :fail
)

:: -- Website downloads (used by deploy-linux-website.bat) --
set SITE_DL=%~dp0Website\idm\downloads
if exist "%~dp0Website\idm" (
    if not exist "%SITE_DL%" mkdir "%SITE_DL%"
    del /q "%SITE_DL%\*.zip" "%SITE_DL%\*.xpi" 2>nul
    powershell -NoProfile -Command "Compress-Archive -Force -Path '%OUT_DIR%\%APP_NAME%.exe' -DestinationPath '%OUT_DIR%\%APP_NAME%.zip'"
    copy /y "%OUT_DIR%\%APP_NAME%.zip" "%SITE_DL%\" >nul
    copy /y "%CHROME_PKG%" "%SITE_DL%\" >nul
    copy /y "%FIREFOX_PKG%" "%SITE_DL%\" >nul
)

:: -- Done ------------------------------------------------------------------
echo.
echo Done.
echo   EXE:               %OUT_DIR%\%APP_NAME%.exe
echo   EXE (versioned):   %OUT_DIR%\%APP_NAME%-windows-%DATE_STAMP%-v%APP_VERSION%.exe
echo   Chrome (unpacked): %EXT_OUT%\Chrome
echo   Firefox (unpacked):%EXT_OUT%\Firefox
echo   Chrome package:    %CHROME_PKG%
echo   Firefox package:   %FIREFOX_PKG%
echo.
if /i not "%~1"=="nopause" pause
exit /b 0

:fail
echo.
if /i not "%~1"=="nopause" pause
exit /b 1
