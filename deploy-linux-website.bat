@echo off
setlocal

:: Publishes the single-page site in Website\idm to the Linux server (idm.asratech.ir).
:: Needs the OpenSSH client (ssh, scp) that ships with Windows 10/11. The server password is typed
:: by you at the prompts; nothing secret is stored in this file.
:: Optional: set IDM_SERVER_HOST / IDM_SERVER_USER first. Pass "nopause" to skip the final pause.

if "%IDM_SERVER_HOST%"=="" set IDM_SERVER_HOST=185.18.215.72
if "%IDM_SERVER_USER%"=="" set IDM_SERVER_USER=root
set REMOTE=%IDM_SERVER_USER%@%IDM_SERVER_HOST%
set SITE_DIR=%~dp0Website\idm
set CONF=%~dp0Website\idm.asratech.ir.conf
set WEB_ROOT=/var/www/idm
:: Use the deploy key (made once with ssh-keygen) when it exists; otherwise ssh asks for the password
set SSH_OPTS=-o StrictHostKeyChecking=accept-new
if exist "%USERPROFILE%\.ssh\idm_deploy_ed25519" set SSH_OPTS=%SSH_OPTS% -i "%USERPROFILE%\.ssh\idm_deploy_ed25519"

where ssh >nul 2>&1
if errorlevel 1 (
    echo ERROR: the OpenSSH client was not found. Install it from Windows Optional Features.
    goto :fail
)
if not exist "%SITE_DIR%\index.html" (
    echo ERROR: %SITE_DIR%\index.html was not found.
    goto :fail
)

echo [1/4] Creating %WEB_ROOT% on the server...
ssh %SSH_OPTS% %REMOTE% "mkdir -p %WEB_ROOT%/downloads %WEB_ROOT%/fonts"
if errorlevel 1 goto :fail

echo [2/4] Uploading the site files...
scp %SSH_OPTS% -r "%SITE_DIR%\index.html" "%SITE_DIR%\logo.png" "%REMOTE%:%WEB_ROOT%/"
if errorlevel 1 goto :fail
scp %SSH_OPTS% "%SITE_DIR%\fonts\Vazirmatn-Regular.ttf" "%REMOTE%:%WEB_ROOT%/fonts/"
if errorlevel 1 goto :fail
scp %SSH_OPTS% "%SITE_DIR%\downloads\*" "%REMOTE%:%WEB_ROOT%/downloads/"
if errorlevel 1 goto :fail

echo [3/4] Installing the nginx site...
scp %SSH_OPTS% "%CONF%" "%REMOTE%:/etc/nginx/conf.d/idm.asratech.ir.conf"
if errorlevel 1 goto :fail

echo [4/4] Testing and reloading nginx...
ssh %SSH_OPTS% %REMOTE% "chown -R www-data:www-data %WEB_ROOT% 2>/dev/null || true; nginx -t && systemctl reload nginx"
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
