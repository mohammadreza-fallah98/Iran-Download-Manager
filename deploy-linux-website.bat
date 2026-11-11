@echo off
setlocal

:: Publishes https://idm.asratech.ir: the landing page (Website\idm\p\idm), the download files and the nginx site.
:: The page lives in the shared asratech.ir tree like the other product pages (/var/www/asratech.ir/p/idm);
:: the logo and the downloads live in /var/www/idm.
:: Needs the OpenSSH client (ssh, scp). With the deploy key (%USERPROFILE%\.ssh\idm_deploy_ed25519) no password
:: is asked; otherwise ssh asks for the server password. Nothing secret is stored in this file.
:: Optional: set IDM_SERVER_HOST / IDM_SERVER_USER first. Pass "nopause" to skip the final pause.

if "%IDM_SERVER_HOST%"=="" set IDM_SERVER_HOST=185.18.215.72
if "%IDM_SERVER_USER%"=="" set IDM_SERVER_USER=root
set REMOTE=%IDM_SERVER_USER%@%IDM_SERVER_HOST%
set SITE_DIR=%~dp0Website\idm
set CONF=%~dp0Website\idm.asratech.ir.conf
set DL_ROOT=/var/www/idm
set PAGE_ROOT=/var/www/asratech.ir/p/idm

set SSH_OPTS=-o StrictHostKeyChecking=accept-new
if exist "%USERPROFILE%\.ssh\idm_deploy_ed25519" set SSH_OPTS=%SSH_OPTS% -i "%USERPROFILE%\.ssh\idm_deploy_ed25519"

where ssh >nul 2>&1
if errorlevel 1 (
    echo ERROR: the OpenSSH client was not found. Install it from Windows Optional Features.
    goto :fail
)
if not exist "%SITE_DIR%\p\idm\index.html" (
    echo ERROR: %SITE_DIR%\p\idm\index.html was not found.
    goto :fail
)

echo [1/4] Creating the folders on the server...
ssh %SSH_OPTS% %REMOTE% "mkdir -p %DL_ROOT%/downloads %PAGE_ROOT%/assets"
if errorlevel 1 goto :fail

echo [2/4] Uploading the page, logo and downloads...
scp %SSH_OPTS% "%SITE_DIR%\p\idm\index.html" "%REMOTE%:%PAGE_ROOT%/"
if errorlevel 1 goto :fail
scp %SSH_OPTS% "%SITE_DIR%\p\idm\assets\*" "%REMOTE%:%PAGE_ROOT%/assets/"
if errorlevel 1 goto :fail
scp %SSH_OPTS% "%SITE_DIR%\logo.png" "%REMOTE%:%DL_ROOT%/"
if errorlevel 1 goto :fail
scp %SSH_OPTS% "%SITE_DIR%\downloads\*" "%REMOTE%:%DL_ROOT%/downloads/"
if errorlevel 1 goto :fail

echo [3/4] Installing the nginx site...
scp %SSH_OPTS% "%CONF%" "%REMOTE%:/etc/nginx/conf.d/idm.conf"
if errorlevel 1 goto :fail

echo [4/4] Testing and reloading nginx...
ssh %SSH_OPTS% %REMOTE% "chown -R www-data:www-data %DL_ROOT% %PAGE_ROOT% 2>/dev/null || true; nginx -t && systemctl reload nginx"
if errorlevel 1 goto :fail

echo.
echo Done. Open https://idm.asratech.ir
echo.
if /i not "%~1"=="nopause" pause
exit /b 0

:fail
echo.
echo ERROR: the website was not published. Read the message above.
echo.
if /i not "%~1"=="nopause" pause
exit /b 1
