@echo off
:: Общие пути. Не запускать напрямую: call "%~dp0_common.cmd"
set "ROOT=%~dp0"
set "ZAPRET_DIR=%ROOT%zapret"
set "WINWS=%ZAPRET_DIR%\winws.exe"
set "MIHOMO_DIR=%ROOT%mihomo"
set "MIHOMO_EXE=%MIHOMO_DIR%\mihomo.exe"
set "MIHOMO_CONFIG=%ROOT%config.yaml"
set "MIHOMO_TEST_CONFIG=%ROOT%warp-test.yaml"
set "WGCF=%ROOT%warp\wgcf.exe"
set "STRATEGY_FILE=%ROOT%zapret-strategy.txt"
set "CUSTOM_FILE=%ROOT%zapret-custom.txt"
set "PS=powershell -NoProfile -ExecutionPolicy Bypass -File"

:: Нет бинарей - качаем из официальных релизов
if exist "%WINWS%" if exist "%MIHOMO_EXE%" if exist "%WGCF%" exit /b 0
echo Не хватает zapret / mihomo / wgcf, запускаю setup.ps1...
%PS% "%ROOT%setup.ps1" -NoPause
if not exist "%WINWS%" exit /b 1
if not exist "%MIHOMO_EXE%" exit /b 1
if not exist "%WGCF%" exit /b 1
exit /b 0
