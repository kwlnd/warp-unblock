@echo off
chcp 866 >nul
setlocal

net session >nul 2>&1
if errorlevel 1 (
    echo Нужны права администратора, перезапускаю...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

title WARP-UNBLOCK: подбор стратегии Zapret
color 0E

echo ╔══════════════════════════════════════════════════════════════╗
echo ║     Подбор стратегии Zapret для WARP endpoints               ║
echo ║     Стратегия запускается, затем проверяется тоннель WARP    ║
echo ╚══════════════════════════════════════════════════════════════╝

call "%~dp0_common.cmd"
if errorlevel 1 (
    echo [!] Не удалось скачать зависимости, см. вывод setup.ps1 выше
    pause
    exit /b 1
)

:menu
set "SAVED=1"
if exist "%STRATEGY_FILE%" set /p SAVED=<"%STRATEGY_FILE%"
echo.
echo  Выберите стратегию, сейчас сохранена %SAVED%:
echo.
for %%n in (1 2 3 4 5 6) do (
    call "%ROOT%_strategies.cmd" %%n
    call echo  [%%n] %%ZAPRET_NAME%%
)
echo  [7] Своя стратегия - ввести аргументы winws вручную
echo  [0] Выход
echo.
set "choice="
set /p choice="Номер стратегии: "
if "%choice%"=="0" goto quit

if "%choice%"=="7" (
    set "custom_args="
    set /p custom_args="Аргументы winws: "
    call :save_custom
)
call "%ROOT%_strategies.cmd" %choice%
if not defined ZAPRET_ARGS (
    echo [!] Нет такой стратегии
    goto menu
)

echo.
echo [*] Стратегия %choice%: %ZAPRET_NAME%
taskkill /f /im mihomo.exe >nul 2>&1
taskkill /f /im winws.exe >nul 2>&1
timeout /t 1 /nobreak >nul
start "zapret" /min "%WINWS%" %ZAPRET_ARGS%
timeout /t 2 /nobreak >nul
tasklist /fi "imagename eq winws.exe" | findstr /i "winws" >nul
if errorlevel 1 (
    echo [FAIL] winws не запустился - аргументы неверные
    goto menu
)

:: Первый раз заодно регистрирует аккаунт WARP - API Cloudflare тоже может резаться DPI
%PS% "%ROOT%_warp.ps1" -Action register
if errorlevel 1 (
    taskkill /f /im winws.exe >nul 2>&1
    echo [FAIL] Не удалось получить профиль WARP, пробуйте другую стратегию
    goto menu
)

echo [*] Поднимаю тестовый тоннель WARP без TUN, жду до 30 секунд...
start "mihomo-test" /min "%MIHOMO_EXE%" -d "%MIHOMO_DIR%" -f "%MIHOMO_TEST_CONFIG%"
%PS% "%ROOT%_warp.ps1" -Action check -Port 9091
set "RESULT=%errorlevel%"
taskkill /f /im mihomo.exe >nul 2>&1
taskkill /f /im winws.exe >nul 2>&1
if not "%RESULT%"=="0" (
    echo [FAIL] WARP не отвечает, пробуйте другую стратегию
    goto menu
)

echo [OK] WARP работает на стратегии %choice%
set "save="
set /p save="Сохранить её для start.bat? [y/N]: "
if /i "%save%"=="y" (
    >"%STRATEGY_FILE%" echo %choice%
    echo [OK] Сохранено в zapret-strategy.txt
)
goto menu

:save_custom
>"%CUSTOM_FILE%" echo %custom_args%
exit /b 0

:quit
taskkill /f /im mihomo.exe >nul 2>&1
taskkill /f /im winws.exe >nul 2>&1
exit /b 0
