# SESSION

## Состояние (2026-11-05)
Портативная схема без клиента WARP: zapret → WireGuard-тоннель WARP прямо в mihomo (UDP работает) → TUN. setup скачал бинари, аккаунт WARP зарегистрирован. Тоннель проверен вживую (delay ~350–600 мс) через warp-test.yaml; start.bat с TUN ещё не запускался.

Проверено:
- `_strategies.cmd` в cmd собирает аргументы winws для всех стратегий.
- `setup.ps1` и `_warp.ps1` без ошибок разбираются в pwsh и PS 5.1.
- `_warp.ps1`: register на реальном аккаунте пишет `mihomo/warp.yaml` и получает reserved; check — 3 из 3 холодных стартов OK, на мёртвом порту возвращает 1.
- `mihomo -t` проходит и на config.yaml, и на warp-test.yaml.
- Голый WireGuard до WARP у этого провайдера режется (таймаут на всех эндпоинтах, со стратегией 1 тоже). С `amnezia-wg-option` jc=4/jmin=40/jmax=70 поднимается на всех эндпоинтах, даже без zapret.

## Решения
- WARP через wgcf + outbound `wireguard` в mihomo (выбрал пользователь), warp-cli выпилен. wgcf — сторонний, не от Cloudflare.
- Профиль WARP — file-провайдер `mihomo/warp.yaml`, `config.yaml` статичный. Эндпоинт задаётся IP 162.159.192.1, не доменом.
- `reserved` берётся из API Cloudflare (`/v0a1922/reg/{device_id}`) best-effort и кешируется в `warp/reserved.txt`.
- `mihomo/warp.yaml` пересобирается из профиля при каждом register — параметры менять в `_warp.ps1`.
- Регистрация делается после старта zapret: API Cloudflare тоже может резаться DPI.
- Стратегия zapret одна на все скрипты (`_strategies.cmd`), номер в `zapret-strategy.txt`.
- test-strategies проверяет стратегию delay-тестом группы `WARP` через API тестового mihomo (`warp-test.yaml`, порт 9091, без TUN), после теста убивает winws и mihomo. Прокси из провайдеров в `/proxies` не видны, только через группу.
- `allow-lan: false`, потому что в authentication лежит пароль-заглушка.
- Локальные `lists/*-domains.txt` подключены как file-провайдеры через `../lists` + `SAFE_PATHS` в start.bat.

## Подводные камни
- bat/cmd — CP866 без BOM, только CRLF, в точках входа `chcp 866`; ps1 — UTF-8 с BOM (иначе PS 5.1 портит кириллицу). Не пересохранять bat/cmd в UTF-8.
- В bat нельзя писать `)` в echo внутри блоков `if (...)`.
- 2026-11-06: UTF-8 + переключение шрифта не устранили пропадание кириллицы в start.bat. Наши bat/cmd переведены в CP866; рамки сохранены, тире и стрелки заменены ASCII. Зависимость от `_font.ps1` убрана, setup тоже выбирает CP866. Шрифт и реестр не меняются.
- Не запускать несколько mihomo с одним ключом WARP одновременно — потом пару минут не проходит handshake.
- Первый handshake после паузы дольше 5 с, поэтому в check таймаут 10 с на попытку и 30 с всего.
- Свой `DIRECT` в `proxies` mihomo не принимает (duplicate name).
- Heredoc в Git Bash портит обратные слеши в python-строках (backslash+r становится CR), sed -i снимает CRLF — после правок возвращать CRLF.
- В _warp.ps1 нельзя называть переменную `$profile` — это автоматическая переменная PS.
- `lists/telegram-cidrs.txt` и `lists/discord-voice-ips.txt` не используются, берутся удалённые mrs.

## Дальше
- Живой прогон start.bat с TUN: сайты из списков, Discord-голос.
- Решить, нужен ли zapret вообще: тоннель поднимается одними мусорными пакетами. Пока zapret оставлен — нужен для регистрации через API и как запасной вариант.

## Discord: диагностика 2026-11-06
- Пользователь: с одним warp-unblock Discord не загружается, YouTube работает. Затем вернулся на FlClashX.
- Проверка отдельным mihomo без TUN, с привязкой к Ethernet: discord.com/app, API gateway и сервер обновлений отвечают HTTP 200 через наш WARP; WebSocket gateway.discord.gg открывается и присылает opcode 10 HELLO. Это не проверка работы приложения через TUN.
- На момент диагностики штатные mihomo/winws и Discord были закрыты; активен TUN FlClashX. Его не меняли.
- start.bat теперь сохраняет stdout/stderr mihomo в mihomo/mihomo.log (UTF-8). Нужен лог проблемного запуска с TUN; причина пока не установлена, сетевые правила наугад не менялись.
- 2026-11-06: по просьбе пользователя группа «По умолчанию» содержит только DIRECT; MATCH идёт через неё. Сохранённый ранее выбор WARP больше не применим к этой группе. WARP остаётся для явных правил обхода.
