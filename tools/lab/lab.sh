#!/system/bin/sh
# Стенд стратегий WARP: одна проверка.
# lab.sh ENDPOINT PORT JC JMIN JMAX HOLD [аргументы nfqws...]
# HOLD — сколько секунд после первого ответа держать соединение под нагрузкой (0 — только рукопожатие).
# SLOT (env, 0..9) — параллельные проверки на своих портах; ход удержания пишется в $W/hold-$SLOT.log.
# Печатает: OK <мс> <ответов>/<проверок> | FAIL <причина>
W=/data/local/tmp/wlab
EP=$1; PORT=$2; JC=$3; JMIN=$4; JMAX=$5; HOLD=$6; shift 6
. $W/account.env
SLOT=${SLOT:-0}
R=$W/run$SLOT
MP=$((15631+SLOT)); AP=$((19091+SLOT)); Q=$((200+SLOT))
rm -rf $R; mkdir -p $R
cat > $R/config.yaml <<EOF
mixed-port: $MP
external-controller: 127.0.0.1:$AP
log-level: warning
interface-name: wlan0
ipv6: false
mode: rule
proxies:
  - name: WARP-WG
    type: wireguard
    server: $EP
    port: $PORT
    ip: $IPV4
    private-key: $PRIVATE
    public-key: $PUBLIC
    mtu: 1280
    udp: true
    reserved: [$RESERVED]
    amnezia-wg-option:
      jc: $JC
      jmin: $JMIN
      jmax: $JMAX
proxy-groups:
  - name: WARP
    type: select
    proxies: [WARP-WG]
rules:
  - MATCH,WARP
EOF
NFQ=""
cleanup() {
  [ -n "$MPID" ] && kill $MPID 2>/dev/null
  if [ -n "$NFQ" ]; then
    kill $NPID 2>/dev/null
    iptables -t mangle -D OUTPUT -o wlan0 -p udp -d $EP --dport $PORT -m mark ! --mark 0x40000000/0x40000000 -j NFQUEUE --queue-num $Q --queue-bypass 2>/dev/null
  fi
  wait 2>/dev/null
}
trap cleanup EXIT
if [ $# -gt 0 ]; then
  NFQ=1
  $W/nfqws --qnum=$Q "$@" > $R/nfqws.log 2>&1 &
  NPID=$!
  iptables -t mangle -I OUTPUT -o wlan0 -p udp -d $EP --dport $PORT -m mark ! --mark 0x40000000/0x40000000 -j NFQUEUE --queue-num $Q --queue-bypass
fi
$W/mihomo -d $R -f $R/config.yaml > $R/mihomo.log 2>&1 &
MPID=$!
i=0
while ! curl -s -m 1 http://127.0.0.1:$AP/version >/dev/null 2>&1; do
  i=$((i+1)); [ $i -gt 30 ] && { echo "FAIL core"; exit; }
  sleep 0.1
done
probe() {
  curl -s -m 7 "http://127.0.0.1:$AP/group/WARP/delay?url=https%3A%2F%2Fwww.gstatic.com%2Fgenerate_204&timeout=5000" | sed -n 's/.*"WARP-WG":\([0-9]*\).*/\1/p'
}
D=$(probe)
[ -z "$D" ] && { echo "FAIL handshake"; exit; }
OKS=1; TRIES=1
START=$(date +%s); END=$(( START + HOLD )); LAST=$START
[ "$HOLD" -gt 0 ] && echo "$(date +%T) start $EP:$PORT $D ms" > $W/hold-$SLOT.log
while [ $(date +%s) -lt $END ]; do
  sleep 10
  TRIES=$((TRIES+1))
  # 100 КБ через туннель: пустой канал ТСПУ может и не заметить.
  B=$(curl -s -x socks5h://127.0.0.1:$MP -o /dev/null -m 8 -w '%{size_download}' 'https://speed.cloudflare.com/__down?bytes=100000')
  if [ "$B" = "100000" ]; then OKS=$((OKS+1)); LAST=$(date +%s); fi
  echo "$(date +%T) $((($(date +%s)-START)/60))m ok=$OKS/$TRIES last_ok=$((($(date +%s)-LAST)))s ago" >> $W/hold-$SLOT.log
done
echo "OK $D $OKS/$TRIES"
