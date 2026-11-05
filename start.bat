@echo off
chcp 866 >nul
setlocal

:: winws и TUN требуют прав администратора
net session >nul 2>&1
if errorlevel 1 (
    echo Нужны права администратора, перезапускаю...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

title WARP-UNBLOCK: Zapret -> WARP -> mihomo
color 0A

echo ╔══════════════════════════════════════════════════════════════╗
echo ║          WARP-UNBLOCK - Обход блокировок через WARP          ║
echo ║  Zapret пробивает DPI до WARP, mihomo держит тоннель         ║
echo ╚══════════════════════════════════════════════════════════════╝
echo.

:: ============================================================================
::  ЗАВИСИМОСТИ - при нехватке _common.cmd сам запустит setup.ps1
:: ============================================================================
echo [1/4] Проверяю зависимости...
call "%~dp0_common.cmd"
if errorlevel 1 (
    echo [!] Не удалось скачать зависимости, см. вывод setup.ps1 выше
    goto fail
)
echo     OK: zapret, mihomo, wgcf на месте
echo.

:: ============================================================================
::  ZAPRET - DPI-обход только для WARP-эндпоинтов
:: ============================================================================
set "STRATEGY=1"
if exist "%STRATEGY_FILE%" set /p STRATEGY=<"%STRATEGY_FILE%"
call "%ROOT%_strategies.cmd" %STRATEGY%
if not defined ZAPRET_ARGS (
    echo [!] Стратегия %STRATEGY% не найдена, беру 1
    set "STRATEGY=1"
    call "%ROOT%_strategies.cmd" 1
)
echo [2/4] Запускаю Zapret, стратегия %STRATEGY%: %ZAPRET_NAME%

taskkill /f /im mihomo.exe >nul 2>&1
taskkill /f /im winws.exe >nul 2>&1
timeout /t 1 /nobreak >nul
start "zapret" /min "%WINWS%" %ZAPRET_ARGS%
timeout /t 2 /nobreak >nul
tasklist /fi "imagename eq winws.exe" | findstr /i "winws" >nul
if errorlevel 1 (
    echo [!] winws не запустился. Проверьте стратегию через test-strategies.bat
    goto fail
)
echo     OK: Zapret работает
echo.

:: ============================================================================
::  WARP - аккаунт и WireGuard-профиль для mihomo, один раз
:: ============================================================================
echo [3/4] Проверяю профиль WARP...
%PS% "%ROOT%_warp.ps1" -Action register
if errorlevel 1 goto fail
echo.

:: ============================================================================
::  MIHOMO - маршрутизация и тоннель до WARP
:: ============================================================================
echo [4/4] Запускаю mihomo...
if not exist "%MIHOMO_DIR%\rule-sets" mkdir "%MIHOMO_DIR%\rule-sets"

:: mihomo читает файлы только внутри своей папки, lists\ разрешаем отдельно
set "SAFE_PATHS=%ROOT%lists"

"%MIHOMO_EXE%" -t -d "%MIHOMO_DIR%" -f "%MIHOMO_CONFIG%" >nul 2>&1
if errorlevel 1 (
    echo [!] Ошибка в config.yaml:
    "%MIHOMO_EXE%" -t -d "%MIHOMO_DIR%" -f "%MIHOMO_CONFIG%"
    goto fail
)

start "mihomo" /min cmd /d /c ""%MIHOMO_EXE%" -d "%MIHOMO_DIR%" -f "%MIHOMO_CONFIG%" >"%MIHOMO_DIR%\mihomo.log" 2>&1"
timeout /t 3 /nobreak >nul
tasklist /fi "imagename eq mihomo.exe" | findstr /i "mihomo" >nul
if errorlevel 1 (
    echo [!] mihomo не запустился. Лог: %MIHOMO_DIR%\mihomo.log
    goto fail
)
echo     OK: mihomo работает, проверяю тоннель WARP...
%PS% "%ROOT%_warp.ps1" -Action check -Port 9090
if errorlevel 1 (
    echo [!] WARP не отвечает. Подберите стратегию zapret через test-strategies.bat
) else (
    echo     OK: тоннель WARP поднят
)
echo.

echo ══════════════════════════════════════════════════════════════
echo     Zapret        : стратегия %STRATEGY%
echo     mihomo TUN    : весь трафик системы идёт через правила
echo     mihomo proxy  : 127.0.0.1:7890 - HTTP/SOCKS5
echo     mihomo панель : http://127.0.0.1:9090/ui
echo ══════════════════════════════════════════════════════════════
echo  Нажмите любую клавишу, чтобы остановить все компоненты
pause >nul
call "%ROOT%stop.bat" nopause
exit /b 0

:fail
echo.
pause
exit /b 1
