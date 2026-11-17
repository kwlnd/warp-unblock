#!/usr/bin/env sh
# Ярлык и значок для переносимой установки (из архива): добавляет WARP Unblock в меню приложений.
# Ничего не копирует: программа остаётся в этой папке. Запуск: ./install.sh, удаление: ./install.sh --remove
set -eu

dir="$(cd "$(dirname "$0")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
desktop="$data/applications/warp-unblock.desktop"
icon="$data/icons/hicolor/256x256/apps/warp-unblock.png"

if [ "${1:-}" = "--remove" ]; then
  rm -f "$desktop" "$icon" "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/warp-unblock.desktop"
  echo "Ярлык и автозапуск удалены. Папку с программой можно удалить вручную."
  exit 0
fi

case "$dir" in
  *[\"\`\$\\]*) echo "В пути к папке есть спецсимволы (\" \` \$ \\): переложите программу в другое место." >&2; exit 1 ;;
esac

mkdir -p "$data/applications" "$(dirname "$icon")"
cp "$dir/share/warp-unblock.png" "$icon"
sed "s|@EXEC@|\"$dir/WarpUnblock\"|" "$dir/share/warp-unblock.desktop" > "$desktop"
chmod 0644 "$desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" || true
echo "Готово: WARP Unblock есть в меню приложений. Запуск: $dir/WarpUnblock"
