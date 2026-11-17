#!/usr/bin/env bash
# Собирает релизные файлы для Linux из готового `cargo build --release`:
#   dist/WarpUnblock-<версия>-linux-<arch>.tar.gz (+ .sha256), warp-unblock_<версия>_<arch>.deb, .rpm
# Использование: package.sh x86_64|aarch64 vX.Y.Z   (нужны curl, tar, sha256sum, gunzip)
set -euo pipefail

ARCH="${1:?архитектура: x86_64 или aarch64}"
TAG="${2:?версия, например v0.8.0}"
VERSION="${TAG#v}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
DIST="$REPO/dist"
STAGE="$DIST/stage"
NAME="WarpUnblock-$TAG-linux-$ARCH"
NFPM=2.49.0

case "$ARCH" in
  x86_64) deb_arch=amd64; nfpm_sha=2afaa5ffb5e3493402e31e7c67a5a1fb42133e3f66fc315ca0051934125561c7; nfpm_file=nfpm_${NFPM}_Linux_x86_64.tar.gz ;;
  aarch64) deb_arch=arm64; nfpm_sha=d4eee71839e92c68cbd3b101640bad32d2856fc551acae0635be9118e48c9d9c; nfpm_file=nfpm_${NFPM}_Linux_arm64.tar.gz ;;
  *) echo "Неизвестная архитектура: $ARCH" >&2; exit 1 ;;
esac

exe="$REPO/windows/source/target/release/warp-unblock"
test -x "$exe" || { echo "Нет $exe: сначала cargo build --release" >&2; exit 1; }

rm -rf "$STAGE"
app="$STAGE/WarpUnblock"
mkdir -p "$app/app/licenses" "$app/share"
install -m 0755 "$exe" "$app/WarpUnblock"
cp -r "$REPO/windows/app/config" "$app/app/config"
cp -r "$REPO/windows/app/assets" "$app/app/assets"
"$HERE/fetch-components.sh" "$ARCH" "$app"
install -m 0644 "$REPO/LICENSE" "$app/app/licenses/LICENSE.txt"
install -m 0644 "$REPO/THIRD_PARTY.md" "$app/app/licenses/THIRD-PARTY.txt"
install -m 0755 "$REPO/linux/packaging/install.sh" "$app/install.sh"
install -m 0644 "$REPO/linux/packaging/warp-unblock.desktop" "$app/share/warp-unblock.desktop"
install -m 0644 "$REPO/windows/source/assets/icons/app.png" "$app/share/warp-unblock.png"
install -m 0644 "$REPO/linux/README.md" "$STAGE/README.md"

# Проверка собранного: ядра на месте, nfqws принимает все профили, mihomo принимает конфиг,
# задание убивает дочерние процессы. Данные проверки в архив не попадают.
WARP_ROOT="$app" "$app/WarpUnblock" --self-test || { cat "$app/data/logs/self-test-result.txt" "$app/data/logs/self-test.log" >&2 || true; exit 1; }
cat "$app/data/logs/self-test-result.txt"
rm -rf "$app/data"

# Переносимый архив: в корне README.md и папка WarpUnblock/ (как ZIP на Windows; так же его читает апдейтер).
mkdir -p "$DIST"
tar --owner=0 --group=0 --numeric-owner --sort=name -C "$STAGE" -czf "$DIST/$NAME.tar.gz" README.md WarpUnblock
(cd "$DIST" && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")

# deb и rpm: /opt/warp-unblock, ярлык, значок и правило polkit.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL --retry 3 -o "$tmp/nfpm.tar.gz" "https://github.com/goreleaser/nfpm/releases/download/v$NFPM/$nfpm_file"
echo "$nfpm_sha  $tmp/nfpm.tar.gz" | sha256sum -c --quiet -
tar -xzf "$tmp/nfpm.tar.gz" -C "$tmp" nfpm

sed "s|@EXEC@|/opt/warp-unblock/WarpUnblock|" "$REPO/linux/packaging/warp-unblock.desktop" > "$tmp/warp-unblock.desktop"
cat > "$tmp/nfpm.yaml" <<EOF
name: warp-unblock
arch: $deb_arch
platform: linux
version: $VERSION
section: net
priority: optional
maintainer: kwlnd <kwlnd@users.noreply.github.com>
description: |
  Заблокированные сервисы через Cloudflare WARP, нейросети через Opera EU.
  Окно и трей работают от пользователя, права для TUN запрашивает polkit.
homepage: https://github.com/kwlnd/warp-unblock
license: MIT
contents:
  - src: $app/WarpUnblock
    dst: /opt/warp-unblock/WarpUnblock
    file_info: { mode: 0755 }
  - src: $app/app/
    dst: /opt/warp-unblock/app/
    type: tree
  - src: /opt/warp-unblock/WarpUnblock
    dst: /usr/bin/warp-unblock
    type: symlink
  - src: $tmp/warp-unblock.desktop
    dst: /usr/share/applications/warp-unblock.desktop
  - src: $app/share/warp-unblock.png
    dst: /usr/share/icons/hicolor/256x256/apps/warp-unblock.png
  - src: $REPO/linux/packaging/org.warpunblock.helper.policy
    dst: /usr/share/polkit-1/actions/org.warpunblock.helper.policy
overrides:
  deb:
    depends: [pkexec | policykit-1, libvulkan1, libxkbcommon0, libxkbcommon-x11-0, libfontconfig1, libfreetype6, libwayland-client0, libxcb1, xdg-utils, nftables | iptables]
    recommends: [mesa-vulkan-drivers]
  rpm:
    depends: [polkit, vulkan-loader, libxkbcommon, libxkbcommon-x11, fontconfig, freetype, libwayland-client, libxcb, xdg-utils]
    recommends: [mesa-vulkan-drivers]
    suggests: [nftables, iptables]
EOF
"$tmp/nfpm" package -f "$tmp/nfpm.yaml" -p deb -t "$DIST/warp-unblock_${VERSION}_${deb_arch}.deb"
"$tmp/nfpm" package -f "$tmp/nfpm.yaml" -p rpm -t "$DIST/warp-unblock-${VERSION}-1.${ARCH}.rpm"
rm -rf "$STAGE"
ls -l "$DIST"
