@echo off
:: Стратегии zapret для WARP. Не запускать напрямую: call "%~dp0_strategies.cmd" <номер>
:: На выходе ZAPRET_ARGS (аргументы winws) и ZAPRET_NAME. Пустой ZAPRET_ARGS - стратегии нет.
:: TCP 443 - API WARP (регистрация), UDP - WireGuard-эндпоинты.
set "ZAPRET_ARGS="
set "ZAPRET_NAME="
set "HOSTS=%~dp0lists\warp-hosts.txt"
set "IPS=%~dp0lists\warp-ips.txt"
set "FAKE_TLS=%~dp0zapret\tls_clienthello_www_google_com.bin"
set "UDP_PORTS=500,1701,2408,4500"
set "WF=--wf-tcp=443 --wf-udp=%UDP_PORTS%"
set "TCP=--filter-tcp=443 --hostlist="%HOSTS%" --dpi-desync-fake-tls="%FAKE_TLS%""
set "UDP=--filter-udp=%UDP_PORTS% --ipset="%IPS%" --dpi-desync=fake --dpi-desync-any-protocol"

for %%n in (1 2 3 4 5 6 7) do if "%~1"=="%%n" goto s%%n
exit /b 1

:s1
set "ZAPRET_NAME=fake,disorder2 + TTL=4 + md5sig,badseq (универсальная)"
set "ZAPRET_ARGS=%WF% %TCP% --dpi-desync=fake,disorder2 --dpi-desync-split-pos=1 --dpi-desync-ttl=4 --dpi-desync-fooling=md5sig,badseq --new %UDP% --dpi-desync-repeats=6"
exit /b 0

:s2
set "ZAPRET_NAME=fake,split2 + TTL=3 + md5sig (Ростелеком)"
set "ZAPRET_ARGS=%WF% %TCP% --dpi-desync=fake,split2 --dpi-desync-split-pos=3 --dpi-desync-ttl=3 --dpi-desync-fooling=md5sig --new %UDP% --dpi-desync-repeats=8"
exit /b 0

:s3
set "ZAPRET_NAME=fake,disorder2 + TTL=6 + badsum (МГТС)"
set "ZAPRET_ARGS=%WF% %TCP% --dpi-desync=fake,disorder2 --dpi-desync-split-pos=1 --dpi-desync-ttl=6 --dpi-desync-fooling=badsum --new %UDP% --dpi-desync-repeats=6"
exit /b 0

:s4
set "ZAPRET_NAME=fake + TTL=4 + md5sig (Билайн)"
set "ZAPRET_ARGS=%WF% %TCP% --dpi-desync=fake --dpi-desync-ttl=4 --dpi-desync-fooling=md5sig --new %UDP% --dpi-desync-repeats=4"
exit /b 0

:s5
set "ZAPRET_NAME=fake,multisplit + midsld (агрессивная)"
set "ZAPRET_ARGS=%WF% %TCP% --dpi-desync=fake,multisplit --dpi-desync-split-pos=1,midsld --dpi-desync-ttl=4 --dpi-desync-fooling=md5sig,badseq --new %UDP% --dpi-desync-repeats=11"
exit /b 0

:s6
set "ZAPRET_NAME=только UDP fake (если TCP и так работает)"
set "ZAPRET_ARGS=--wf-udp=%UDP_PORTS% %UDP% --dpi-desync-repeats=6"
exit /b 0

:s7
set "ZAPRET_NAME=своя (из zapret-custom.txt)"
if exist "%~dp0zapret-custom.txt" set /p ZAPRET_ARGS=<"%~dp0zapret-custom.txt"
exit /b 0
