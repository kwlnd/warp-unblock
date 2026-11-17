# WARP Unblock для Linux

Та же программа, что для Windows и Android: заблокированные сервисы идут через Cloudflare WARP, нейросети — через Opera EU поверх WARP, остальной интернет — напрямую. Сборки для **x86_64** и **aarch64**.

**[Скачать последнюю версию](https://github.com/kwlnd/warp-unblock/releases/latest)**

| Файл | Для кого |
| --- | --- |
| `WarpUnblock-…-linux-x86_64.tar.gz`, `…-linux-aarch64.tar.gz` | любой дистрибутив: распаковал и запустил |
| `warp-unblock_…_amd64.deb`, `…_arm64.deb` | Debian, Ubuntu, Mint, Pop!_OS |
| `warp-unblock-…-1.x86_64.rpm`, `…-1.aarch64.rpm` | Fedora, openSUSE, ALT |

## Установка

**Из архива.** Распакуйте в папку с правом записи, например `~/WarpUnblock`, и запустите `WarpUnblock/WarpUnblock`. Чтобы программа появилась в меню приложений, выполните `./install.sh` в этой папке (`./install.sh --remove` убирает ярлык). Программа обновляется сама из окна.

**Из пакета.**

```bash
sudo apt install ./warp-unblock_*_amd64.deb      # Debian, Ubuntu
sudo dnf install ./warp-unblock-*.x86_64.rpm     # Fedora
```

Программа ставится в `/opt/warp-unblock`, настройки, аккаунт WARP и ваши списки лежат в `~/.local/share/warp-unblock`. Обновляйте новым пакетом: кнопка «Обновить» в окне для пакетной установки не работает.

## Что нужно в системе

- **polkit** (`pkexec`) и работающий агент аутентификации — в GNOME, KDE, XFCE он есть. Окно запрашивает пароль один раз за сеанс, когда вы нажимаете «Подключить» или «Подобрать».
- **nftables** или **iptables** — для очереди пакетов nfqws (маскировка рукопожатия WireGuard).
- **Vulkan** (`libvulkan1`, а в виртуальной машине ещё `mesa-vulkan-drivers`) — окно рисуется через GPU.
- **Трей.** Значок показывается через StatusNotifierItem: в KDE, XFCE, Cinnamon и MATE работает сразу, в GNOME нужно расширение [AppIndicator](https://extensions.gnome.org/extension/615/appindicator-support/). Без трея окно просто не прячется при закрытии.

Если включён другой VPN (особенно клиенты на ядре Clash), выключите его.

## Как это работает

Окно, трей и настройки — от обычного пользователя. Права root нужны только двум процессам: mihomo (создаёт TUN и маршруты) и nfqws (очередь пакетов NFQUEUE). Их запускает помощник — тот же бинарник с ключом `--helper` под `pkexec`. Он один на весь сеанс, принимает команды от окна по каналу, запускает только файлы из `app/bin/` рядом с собой и сам убирает TUN и правила nftables, когда окно закрылось или упало.

Правила очереди — отдельная таблица `inet warp_unblock_<N>` с флагом `bypass`: если nfqws упадёт, пакеты идут как обычно. Таблицы, оставшиеся после сбоя, помощник удаляет при следующем старте.

Всё остальное — как в Windows-версии: профили и адреса, подбор и автопереключение, списки доменов, резервные копии и QR. Подробности — в [README Windows](https://github.com/kwlnd/warp-unblock/blob/main/windows/README.md) и [docs/DEVELOPMENT.md](https://github.com/kwlnd/warp-unblock/blob/main/docs/DEVELOPMENT.md).

## Автозапуск

Переключатель в настройках кладёт `~/.config/autostart/warp-unblock.desktop` и запускает программу с `--tray`. Подключение при входе потребует пароль polkit.

## Удаление

```bash
sudo apt remove warp-unblock        # или sudo dnf remove warp-unblock
rm -rf ~/.local/share/warp-unblock ~/.config/autostart/warp-unblock.desktop
```

Для архива: `./install.sh --remove` и удалить папку. Данные при переносимой установке лежат в `data/` рядом с программой.

## Если что-то не работает

Журналы лежат в `data/logs/` (`launcher.log`, `mihomo.log`, `nfqws` — в `zapret.log`): рядом с программой или в `~/.local/share/warp-unblock/data/logs/`. Они могут содержать домены и IP; ключи из `data/` публиковать нельзя.

- «Не найден pkexec» — поставьте `polkit` (`pkexec`).
- «Права администратора не получены» — окно пароля закрыто или нет агента polkit (например, голая оконная среда): запустите любой агент polkit (`lxpolkit`, `polkit-gnome-authentication-agent-1`, `lxqt-policykit`).
- Окно не открывается — проверьте `vulkaninfo`; в виртуальной машине поставьте `mesa-vulkan-drivers`.
