@echo off
chcp 866 >nul
setlocal

:: winws запущен от администратора, без прав его не убить
net session >nul 2>&1
if errorlevel 1 (
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

echo Останавливаю все компоненты WARP-UNBLOCK...
echo.

taskkill /f /im mihomo.exe >nul 2>&1
echo [OK] mihomo и тоннель WARP остановлены

taskkill /f /im winws.exe >nul 2>&1
echo [OK] Zapret остановлен

echo.
echo Все компоненты остановлены.
if /i not "%~1"=="nopause" timeout /t 2
