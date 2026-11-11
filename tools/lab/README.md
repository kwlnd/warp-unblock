# Стенд стратегий WARP на Android с root

Проверяет профили WARP (адрес, порт, мусор Amnezia, аргументы zapret) из реальной сети телефона, мимо любого VPN на нём.

- На телефон в `/data/local/tmp/wlab/`: `lab.sh`, `nfqws` (zapret, `binaries/android-arm64`) и `fake/`, `mihomo` (`libmihomo.so` из установленного APK), `account.env` (`PRIVATE`, `PUBLIC`, `IPV4`, `RESERVED` из `files/warp.json` приложения; root, 600).
- Одна проверка: `su -c 'SLOT=0 sh lab.sh ADDR PORT JC JMIN JMAX HOLD [аргументы nfqws…]'` → `OK <мс> <успешно>/<попыток>` или `FAIL …`. `HOLD` > 0 — держать соединение и качать 100 КБ раз в 10 с, ход в `hold-$SLOT.log`. `SLOT` 0–9 — параллельные проверки.
- Серия с ПК: `python drive.py catalog` или `python drive.py matrix list.json`, результаты в JSONL.
- На время тестов `dumpsys deviceidle disable`, после — `enable` и удалить `/data/local/tmp/wlab`.
