#!/usr/bin/env bash
# Кладёт ядра для Linux в <папка>/app/bin и сверяет скачанное с зафиксированными SHA-256.
# Использование: fetch-components.sh x86_64|aarch64 <папка>
set -euo pipefail

ARCH="${1:?архитектура: x86_64 или aarch64}"
OUT="${2:?папка, в которой будет app/bin}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"

MIHOMO=v1.19.33
WGCF=2.3.1
OPERA=v1.31.0
ZAPRET=v72.13

case "$ARCH" in
  x86_64)
    mihomo_file="mihomo-linux-amd64-compatible-$MIHOMO.gz"
    mihomo_sha=386508bfb1c72c43b6a0eda73708ecdce82546581b0ebcb16366c53757c622b6
    wgcf_file="wgcf_${WGCF}_linux_amd64"
    wgcf_sha=4fb7a136aa470ece11d17949c7db07b040ec9c604742dc8680bcf1889f85d0c8
    opera_file="opera-proxy.linux-amd64"
    opera_sha=e539c6f583c23715d16810925307d74b136aa1f7bc256d586a52e63d03b0346d
    zapret_dir=linux-x86_64
    ;;
  aarch64)
    mihomo_file="mihomo-linux-arm64-$MIHOMO.gz"
    mihomo_sha=ba408b317080ee265c46cb923a50018c1b82cbd708551cdd46a1e3e307396187
    wgcf_file="wgcf_${WGCF}_linux_arm64"
    wgcf_sha=710ab2d8ac145d81a4cf0c11bdb0e05914f77ae2a45f67b87e718d84b4cb7301
    opera_file="opera-proxy.linux-arm64"
    opera_sha=fe1e72f1bfb2302350fff82cf0710859699118ae47d5b541d99c5f13aebcb935
    zapret_dir=linux-arm64
    ;;
  *) echo "Неизвестная архитектура: $ARCH" >&2; exit 1 ;;
esac
zapret_sha=25c74e6c5f48963fa244c2955e76694a07c39447245a0457e2efdc74b3317e68

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fetch() { # url файл sha256
  curl -fsSL --retry 3 -o "$work/$2" "$1"
  echo "$3  $work/$2" | sha256sum -c --quiet - || { echo "Не сошёлся SHA-256: $2" >&2; exit 1; }
}

bin="$OUT/app/bin"
mkdir -p "$bin/mihomo" "$bin/wgcf" "$bin/opera" "$bin/zapret"

fetch "https://github.com/MetaCubeX/mihomo/releases/download/$MIHOMO/$mihomo_file" "$mihomo_file" "$mihomo_sha"
gunzip -c "$work/$mihomo_file" > "$bin/mihomo/mihomo"

fetch "https://github.com/ViRb3/wgcf/releases/download/v$WGCF/$wgcf_file" "$wgcf_file" "$wgcf_sha"
install -m 0755 "$work/$wgcf_file" "$bin/wgcf/wgcf"

fetch "https://github.com/Alexey71/opera-proxy/releases/download/$OPERA/$opera_file" "$opera_file" "$opera_sha"
install -m 0755 "$work/$opera_file" "$bin/opera/opera-proxy"

fetch "https://github.com/bol-van/zapret/releases/download/$ZAPRET/zapret-$ZAPRET.tar.gz" zapret.tar.gz "$zapret_sha"
tar -xzf "$work/zapret.tar.gz" -C "$work" "zapret-$ZAPRET/binaries/$zapret_dir/nfqws"
install -m 0755 "$work/zapret-$ZAPRET/binaries/$zapret_dir/nfqws" "$bin/zapret/nfqws"

# Приманки для nfqws и лицензии те же, что у Windows-сборки (zapret v72.13, files/fake/).
for f in tls_clienthello_www_google_com.bin quic_initial_www_google_com.bin \
         quic_initial_vk_com.bin wireguard_initiation.bin LICENSE-zapret.txt; do
  install -m 0644 "$REPO/windows/app/bin/zapret/$f" "$bin/zapret/$f"
done
install -m 0644 "$REPO/windows/app/bin/opera/LICENSE.txt" "$bin/opera/LICENSE.txt"
chmod 0755 "$bin/mihomo/mihomo"

"$bin/mihomo/mihomo" -v 2>/dev/null | head -1 || true
echo "Компоненты для $ARCH готовы: $bin"
