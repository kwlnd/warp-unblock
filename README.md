# warp-unblock

Обход блокировок на Windows, полностью портативно: **zapret** пробивает DPI до серверов Cloudflare WARP, **mihomo** сам держит WireGuard-тоннель до WARP (TCP и UDP — голос Discord, звонки) и в режиме TUN отправляет заблокированное через WARP, остальное напрямую. Клиент WARP не нужен.

## Запуск

1. `test-strategies.bat` — перебрать стратегии zapret, рабочую сохранить (`zapret-strategy.txt`).
2. `start.bat` — запуск всего; `stop.bat` — остановка.

При первом запуске скрипты сами скачивают zapret ([bol-van/zapret](https://github.com/bol-van/zapret)), mihomo ([MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo)) и wgcf ([ViRb3/wgcf](https://github.com/ViRb3/wgcf)) из последних релизов (`setup.ps1`) и регистрируют аккаунт WARP. Права администратора запрашиваются сами. Панель mihomo: http://127.0.0.1:9090/ui

## Файлы

| | |
|---|---|
| `config.yaml` | конфиг mihomo, правила взяты из `_mihomo-routing` |
| `warp-test.yaml` | конфиг mihomo для проверки стратегий: только WARP, без TUN |
| `_common.cmd`, `_strategies.cmd`, `_warp.ps1` | пути, стратегии zapret, регистрация и проверка WARP |
| `warp\` | wgcf и аккаунт WARP (`wgcf-account.toml` — не раздавать) |
| `mihomo\warp.yaml` | WireGuard-профиль для mihomo, пересобирается при каждом запуске — параметры в `_warp.ps1` |
| `lists/proxy-domains.txt`, `lists/direct-domains.txt` | свои домены через WARP / напрямую |
| `lists/warp-hosts.txt`, `lists/warp-ips.txt` | на что действует zapret |

Скрипты .bat/.cmd хранятся в CP866 с CRLF и сами выбирают chcp 866: кириллица и рамки работают в обычной консоли Windows с растровым шрифтом. При редактировании сохраняйте эту кодировку; .ps1 — UTF-8 с BOM.
