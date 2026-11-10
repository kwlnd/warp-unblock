# Сторонние компоненты

Лицензия MIT в `LICENSE` относится только к коду приложения (`source/`, `tools/`, `app/scripts/`, `app/config/`). Остальное — чужие программы, лежащие в `app/bin/` как отдельные исполняемые файлы со своими лицензиями. Лицензии указаны по данным проектов; перед распространением сверяйте их с актуальными страницами.

| Компонент | Файл | Лицензия | Источник |
| --- | --- | --- | --- |
| mihomo (Meta, prerelease Alpha) | `app/bin/mihomo/mihomo.exe` | GPL-3.0 | https://github.com/MetaCubeX/mihomo |
| opera-proxy v1.31.0 | `app/bin/opera/opera-proxy.exe` | MIT (`app/bin/opera/LICENSE.txt`) | https://github.com/Alexey71/opera-proxy |
| wgcf | `app/bin/wgcf/wgcf.exe` | MIT | https://github.com/ViRb3/wgcf |
| zapret / winws (v72.13, `87e0586`), собран из исходников — см. `app/bin/zapret/winws-build.json` | `app/bin/zapret/winws.exe` | MIT (`app/bin/zapret/LICENSE-zapret.txt`) | https://github.com/bol-van/zapret |
| WinDivert | `app/bin/zapret/WinDivert.dll`, `WinDivert64.sys` | LGPL-3.0 или GPL-2.0 | https://reqrypt.org/windivert.html |
| Cygwin runtime | `app/bin/zapret/cygwin1.dll` | LGPL-3.0 (с исключением для линковки) | https://cygwin.com |

Исходный код GPL/LGPL-компонентов доступен по ссылкам выше. Приложение запускает их как отдельные процессы и не линкуется с ними.

## SHA-256

```
926bfea2ce48a247d6a7ce4cf641980e416202d5e40fe4ee5c949089ace0e197  app/bin/mihomo/mihomo.exe
e222fd9443d7cb3264d825a3e86967797fc9c938507c5d93e3cc21e4aed11dcc  app/bin/wgcf/wgcf.exe
b94def76338c970a66102bd47249d79088aa98c8e317eb5278294a57cd9a7fc4  app/bin/zapret/winws.exe
c1e060ee19444a259b2162f8af0f3fe8c4428a1c6f694dce20de194ac8d7d9a2  app/bin/zapret/WinDivert.dll
8da085332782708d8767bcace5327a6ec7283c17cfb85e40b03cd2323a90ddc2  app/bin/zapret/WinDivert64.sys
ed9d5379061e5de79c3375cc62246c2142ea1c97d94f4d16bfbea6fb1b7fc889  app/bin/zapret/cygwin1.dll
```

Правила маршрутизации берутся из [MetaCubeX/meta-rules-dat](https://github.com/MetaCubeX/meta-rules-dat), [Davoyan/mihomo-rule-sets](https://github.com/Davoyan/mihomo-rule-sets) и [kwlnd/mihomo-routing](https://github.com/kwlnd/mihomo-routing) и скачиваются при работе приложения.

Библиотеки Rust подтягиваются через `windows/source/Cargo.lock`; GUI построен на [GPUI](https://github.com/zed-industries/zed) и [GPUI Kit / gpui-component](https://github.com/longbridge/gpui-kit) (Apache-2.0), иконки Lucide (ISC). Шрифт [Manrope](https://github.com/googlefonts/manrope) (SIL OFL 1.1, `windows/source/assets/fonts/OFL.txt`) вшит в exe; статические начертания нарезаны из вариативного файла Google Fonts.

Диалоги файлов: rfd (MIT); генерация QR: qrcode-rust (MIT/Apache-2.0); Base64: base64 (MIT/Apache-2.0). Версии закреплены в Cargo.lock.
