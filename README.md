# WARP Unblock — Android

Android-версия WARP Unblock. Те же правила маршрутизации, что в Windows-версии: по умолчанию трафик идёт напрямую (DIRECT), а YouTube, Discord, Telegram, мессенджеры, нейросети и списки заблокированных — через Cloudflare WARP.

Windows-версия живёт в ветке `main`; здесь только Android.

## Установка

Скачайте APK из [Releases](../../releases) (теги `android-v*`):

| Файл | Для чего |
| --- | --- |
| `WarpUnblock-android-vX.Y.Z-arm64-v8a.apk` | почти все телефоны последних лет |
| `WarpUnblock-android-vX.Y.Z-armeabi-v7a.apk` | старые 32-битные устройства |

Нужен Android 7.0+ (API 24). Разрешите установку из неизвестных источников, откройте приложение, нажмите «Подключить» и подтвердите запрос VPN.

## Как устроено

```
приложение (Kotlin, Compose)
 ├─ VpnService → TUN
 ├─ hev-socks5-tunnel (JNI) — пробрасывает TUN в локальный SOCKS5
 └─ mihomo (процесс из папки нативных библиотек) — WireGuard до WARP, правила маршрутизации
```

- Само приложение исключено из VPN (`addDisallowedApplication`), иначе WireGuard-трафик ядра зациклился бы в туннель.
- Аккаунт WARP регистрируется при первом подключении (тот же запрос, что делает wgcf) и хранится в приватной папке приложения.
- 11 профилей доставки WireGuard к WARP: UDP-порт, эндпоинт и мусорные пакеты перед handshake (AmneziaWG). Профили с `zapret` из Windows-версии здесь недоступны — на Android нет аналога winws. «Подобрать автоматически» перебирает профили без VPN и сохраняет первый рабочий.
- Конфиг ядра — `android/app/src/main/assets/config.yaml`, списки доменов — `assets/lists/`.

## Если регистрация WARP не проходит

Приложение регистрирует аккаунт WARP прямо с телефона: сначала напрямую, потом через локальный SOCKS-прокси [ByeDPI](https://github.com/hufrea/byedpi) (аналог zapret без root, работает только для запроса регистрации) с перебором 7 стратегий. Если и так не вышло (оператор режет `api.cloudflareclient.com`), будет ошибка «Не удалось зарегистрировать WARP». Тогда:

1. На компьютере получите профиль: в папке `data/warp/` Windows-версии лежит `wgcf-profile.conf` (или выполните `wgcf register --accept-tos` и `wgcf generate` в пустой папке).
2. Отправьте файл на телефон и на вкладке «Профили» нажмите «Импортировать профиль».

Не запускайте один и тот же профиль на компьютере и телефоне одновременно (один ключ WARP — одно подключение). Лучше зарегистрировать отдельный аккаунт для телефона.

## Ограничения

- Регистрация WARP идёт напрямую до Cloudflare. Если оператор режет `api.cloudflareclient.com`, регистрация не пройдёт, а обхода DPI для неё на Android нет.
- Приложение не проверено на реальных устройствах автором: собирается в GitHub Actions, дальше только ручная проверка. Если что-то не работает, пришлите лог из приложения.
- Без root другие VPN одновременно работать не будут (Android разрешает один активный VPN).

## Сборка

Собирается в GitHub Actions (`.github/workflows/android.yml`): скачивает ядро mihomo для Android, собирает hev-socks5-tunnel через NDK и вызывает Gradle. Локально те же шаги: положить `libmihomo.so` и `libhev-socks5-tunnel.so` в `android/app/src/main/jniLibs/<abi>/` и выполнить `gradle -p android assembleRelease`.

APK подписываются хобби-ключом `android/hobby-signing.jks` (пароль `warp-unblock`), чтобы обновления ставились поверх. Это не секрет и не ключ для магазина приложений.

## Сторонние компоненты

| Компонент | Лицензия |
| --- | --- |
| [mihomo](https://github.com/MetaCubeX/mihomo) (v1.19.33, android) | GPL-3.0 |
| [ByeDPI](https://github.com/hufrea/byedpi) (v0.8.1, только регистрация) | MIT |
| [hev-socks5-tunnel](https://github.com/heiher/hev-socks5-tunnel) (2.9.3) | MIT |
| [Bouncy Castle](https://www.bouncycastle.org/) (X25519) | MIT |
| Jetpack Compose, AndroidX | Apache-2.0 |

Код приложения — MIT (`LICENSE`).
