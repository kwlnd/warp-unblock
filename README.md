<div align="center">

<img src="branding/warp-w-orange.svg" width="112" alt="WARP Unblock">

# WARP Unblock

**YouTube, Discord, Telegram и другие заблокированные сервисы — через Cloudflare WARP.**<br>
Остальной интернет работает напрямую, как обычно.

[![Последняя версия](https://img.shields.io/github/v/release/kwlnd/warp-unblock?style=flat&color=fd8119&label=версия)](https://github.com/kwlnd/warp-unblock/releases/latest)
[![Windows](https://img.shields.io/badge/Windows-10%20%2F%2011-0078D4?style=flat&logo=windows&logoColor=white)](windows/README.md)
[![Linux](https://img.shields.io/badge/Linux-x86__64%20%2F%20aarch64-FCC624?style=flat&logo=linux&logoColor=black)](linux/README.md)
[![Android](https://img.shields.io/badge/Android-7.0%2B-3DDC84?style=flat&logo=android&logoColor=white)](android/README.md)
[![Лицензия MIT](https://img.shields.io/badge/лицензия-MIT-171b23?style=flat)](LICENSE)

[![Windows build](https://img.shields.io/github/actions/workflow/status/kwlnd/warp-unblock/windows.yml?branch=main&style=flat&logo=github&label=Windows%20build)](https://github.com/kwlnd/warp-unblock/actions/workflows/windows.yml)
[![Linux build](https://img.shields.io/github/actions/workflow/status/kwlnd/warp-unblock/linux.yml?branch=main&style=flat&logo=github&label=Linux%20build)](https://github.com/kwlnd/warp-unblock/actions/workflows/linux.yml)
[![Android build](https://img.shields.io/github/actions/workflow/status/kwlnd/warp-unblock/android.yml?branch=main&style=flat&logo=github&label=Android%20build)](https://github.com/kwlnd/warp-unblock/actions/workflows/android.yml)
[![Rust](https://img.shields.io/badge/Rust-GPUI-000000?style=flat&logo=rust&logoColor=white)](docs/DEVELOPMENT.md)
[![Kotlin](https://img.shields.io/badge/Kotlin-Compose-7F52FF?style=flat&logo=kotlin&logoColor=white)](docs/DEVELOPMENT.md)
[![mihomo](https://img.shields.io/badge/ядро-mihomo-fd8119?style=flat)](https://github.com/MetaCubeX/mihomo)

### Скачать

[![Скачать для Windows](https://img.shields.io/badge/Скачать-Windows%20x64%20·%20ZIP-0078D4?style=flat&logo=windows&logoColor=white)](https://github.com/kwlnd/warp-unblock/releases/latest)
[![Скачать для Linux](https://img.shields.io/badge/Скачать-Linux%20·%20tar.gz%20%2F%20deb%20%2F%20rpm-FCC624?style=flat&logo=linux&logoColor=black)](https://github.com/kwlnd/warp-unblock/releases/latest)
[![Скачать для Android](https://img.shields.io/badge/Скачать-Android%20·%20APK-3DDC84?style=flat&logo=android&logoColor=white)](https://github.com/kwlnd/warp-unblock/releases/latest)

</div>

На странице релиза:

| Устройство | Файл |
| --- | --- |
| Windows 10/11, x64 | `WarpUnblock-…-win-x64.zip` |
| Linux x86_64 или aarch64 | `WarpUnblock-…-linux-<arch>.tar.gz`, `.deb` (Debian, Ubuntu), `.rpm` (Fedora, openSUSE) |
| Android 7.0+, почти все телефоны | `WarpUnblock-android-…-arm64-v8a.apk` |
| Android: ТВ-приставки, 32-битные системы, или если не уверены | `WarpUnblock-android-…-universal.apk` |

Контрольные суммы всех файлов — `SHA256SUMS.txt` там же. Код через **Code → Download ZIP** скачивать не нужно.

## Что умеет

- **Одна кнопка.** Нажал — подключилось. Аккаунт WARP создаётся сам.
- **Только нужное через WARP.** YouTube, Discord, Telegram, мессенджеры и списки заблокированных сайтов — через туннель, остальное — напрямую, без лишней нагрузки и потери скорости.
- **Маскировка от DPI.** Мусорные пакеты перед рукопожатием (AmneziaWG), на Windows и Linux — по желанию приёмы [zapret](https://github.com/bol-van/zapret).
- **Сам подбирает адрес и профиль.** Провайдер блокирует конкретные адреса и порты — программа перебирает пул адресов Cloudflare и способы маскировки и находит рабочую пару.
- **Сам переключается.** Если WARP перестал отвечать, программа найдёт другой адрес или профиль и переподключится без вашего участия.
- **Нейросети.** На Windows и Linux ChatGPT, Gemini, Claude и другие идут через Opera EU поверх WARP.
- **Свои списки.** «Через WARP» и «Напрямую» редактируются прямо в программе. На Android — ещё и выбор приложений для VPN.
- **Обновления из программы** с проверкой SHA-256.
- **Лёгкая.** Интерфейс на Rust (Windows, Linux) и Compose (Android): в простое около 1% одного ядра.

## Как начать

**Windows.** Распакуйте архив целиком в любую папку, запустите `WarpUnblock.exe` и разрешите права администратора. Нажмите большую кнопку — при первом запуске программа сама создаст аккаунт WARP. [Подробная инструкция →](windows/README.md)

**Linux.** Распакуйте `tar.gz` и запустите `WarpUnblock/WarpUnblock` (или поставьте `.deb`/`.rpm`). Права для TUN запросит polkit. [Подробная инструкция →](linux/README.md)

**Android.** Установите APK, нажмите кнопку на главном экране и разрешите VPN. После первого подключения разрешите работу в фоне — иначе Xiaomi, Samsung и другие оболочки могут закрыть VPN при блокировке экрана. [Подробная инструкция →](android/README.md)

> [!TIP]
> Не подключается — нажмите **Подобрать** на странице «Профили»: программа сама найдёт рабочий вариант.

## Куда идёт трафик

| Трафик | Куда |
| --- | --- |
| YouTube, Discord, Telegram, мессенджеры, списки заблокированных сайтов, ваш список «Через WARP» | → WARP |
| Нейросети (ChatGPT, Gemini, Claude и др.) | → Windows и Linux: Opera EU поверх WARP · Android: WARP |
| Всё остальное и ваш список «Напрямую» | → напрямую |
| Реклама | ✕ блокируется |

Цвет значка «!W» показывает состояние: серый — выключено · жёлтый — подключение · оранжевый — работает · красный — ошибка.

## Если что-то не работает

| Что видно | Что сделать |
| --- | --- |
| WARP не отвечает | Отключите другой VPN, нажмите **Подобрать** |
| «Подключено», но ничего не грузится / «Порт занят» | Закройте другие клиенты на ядре Clash (FlClash, Clash Verge и т.п.) |
| На Android VPN пропадает, когда гаснет экран | **Настройки → Работа в фоне**: снимите ограничение и включите автозапуск; ещё надёжнее — «Постоянная VPN» в системных настройках |
| Не проходит регистрация аккаунта WARP | Перенесите профиль с другого устройства: Windows — **QR для телефона**, Android — **Профили → QR** |
| Конкретный сайт не открывается | Добавьте его домен в список «Через WARP» |

Журнал есть в обеих версиях. В [issue](https://github.com/kwlnd/warp-unblock/issues/new) укажите платформу, версию, профиль и провайдера.

> [!WARNING]
> Профиль WARP, резервная копия и QR содержат приватный ключ — не публикуйте их. Один аккаунт WARP не стоит использовать одновременно на компьютере и телефоне.

## Для разработки

- [Устройство и сборка](docs/DEVELOPMENT.md)
- [Формат списков доменов](docs/DOMAIN-LISTS.md)
- [Сторонние компоненты и лицензии](THIRD_PARTY.md)

<div align="center"><sub>Код — <a href="LICENSE">MIT</a>. Сторонние компоненты — под своими лицензиями.</sub></div>
