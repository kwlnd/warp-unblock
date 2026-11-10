# SESSION — состояние и планы

Рабочая заметка: что сделано, что не проверено, что дальше. Устройство и сборка — [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Структура и релизы

- Монорепо: только ветка `main`, папки `windows/` и `android/` (ветки `android`, `monorepo`, `ccr-*` удалены, PR #18 смержен).
- Релиз один на обе платформы: тег `vX.Y.Z` → `.github/workflows/release.yml` вызывает `windows.yml` и `android.yml`, публикует ZIP, два APK, `SHA256SUMS.txt` (и `.sha256` для ZIP — его читает апдейтер Windows). Версия общая; Android `versionCode` = major·10000+minor·100+patch (у android-v0.3.1 было 10).
- Выпущены: Windows v0.6.0 (отдельно, до объединения), общие v0.7.0 и v0.7.1 (светящаяся сетка, AI-домены только через Opera). Старые релизы не удалять — решение пользователя.
- Архив Windows: в корне `README.md` и папка `WarpUnblock/` (exe + `app/`, лицензии в `app/licenses/*.txt`). Пользователь просил не класть кучу .md.

## Windows (GPUI)

- `gpui-kit` 0.9.1, Rust ≥ 1.95 (`cargo +1.95.0`, `windows/tools/build.ps1` пинит). Своя плашка окна со статусом по центру (`WindowControlArea`), узкое меню слева с иконками Lucide, шрифт Manrope (вшит), светлая/тёмная тема (system/light/dark в настройках), круглая кнопка питания с метриками, всплывашки, подсказки на неактивных кнопках, анимации, иконка (`windows/tools/make-icons.py`).
- Обновление из программы (`update.rs`): окно → загрузка, SHA-256, замена через `*.old` с откатом, перезапуск `--after-update`. Живьём проверены загрузка+SHA+распаковка; полная установка с перезапуском — нет (нужен UAC).
- `--preview` пишет настройки в `windows/data/` (как обычный запуск); `WARP_PREVIEW_UPDATE=1` включает проверку обновлений в preview.

## Android

- Общий стиль с Windows: `Theme.kt` (палитра, Manrope), `Components.kt`, иконки Lucide (`android/tools/lucide-to-vector.py`), вкладки Связь/Профили/Списки/Настройки/Журнал, кнопка питания, иконка-щит.
- Апдейтер (`Updater.kt`, `UpdateUi.kt`) понимает общие `v*` и старые `android-v*`.
- Локальная сборка: `android/tools/build.ps1` (JDK 17, Gradle 8.9, SDK 34 в `android/.toolchain/`, в .gitignore; лицензия SDK принята с согласия пользователя). Снимки экранов: `-Tasks testDebugUnitTest,-Pscreenshots=1` → `android/app/build/screenshots/`. Нативных библиотек локально нет — рабочий APK собирает только CI.
- Проверено: 8 unit-тестов, lint, снимки экранов. На телефоне новый интерфейс и апдейтер не проверены.

## Не проверено вживую

- Windows: трей/скрытый старт и подключение через новый UI, полный TUN с Discord/YouTube, автозапуск, Opera → AI.
- Android: новый интерфейс на телефоне, выбор приложений, автозапуск, Always-on, QR-камера, отправка журнала, установка обновления.
- Автообновление обеих платформ заработает со следующего релиза после v0.7.0 (на v0.6.0/v0.7.0 переход с более старых — вручную).

## Гочи

- Зависший процесс preview `WarpUnblock.exe` (PID 4064) держал `windows/WarpUnblock.exe`; для тестов использовалось `windows/WarpUnblock-dev.exe` (все `windows/*.exe` в .gitignore).
- `sdkmanager.bat` режет аргументы по `;` — пакеты передавать через `--package_file`.

## Дальше

1. Ручной прогон Windows v0.7.0 (трей, подключение, обновление на следующей версии) и Android на телефоне.
2. Linux-клиент на GPUI.
