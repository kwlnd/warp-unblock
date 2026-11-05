<#
.SYNOPSIS
    Работа с WARP для bat-скриптов. Не запускать напрямую.
.DESCRIPTION
    -Action register : регистрирует аккаунт WARP через wgcf (если ещё нет) и пересобирает mihomo\warp.yaml
    -Action check    : проверяет WARP через API запущенного mihomo (delay-тест), код возврата 0 — работает
#>
param(
    [ValidateSet("register", "check")][string]$Action,
    [int]$Port = 9090,
    [int]$Timeout = 30
)

$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$warpDir = Join-Path $Root "warp"
$wgcf = Join-Path $warpDir "wgcf.exe"
$account = Join-Path $warpDir "wgcf-account.toml"
$wgProfile = Join-Path $warpDir "wgcf-profile.conf"
$reservedFile = Join-Path $warpDir "reserved.txt"
$outYaml = Join-Path $Root "mihomo\warp.yaml"
# Эндпоинт берём IP-адресом: DNS до старта WARP может не работать. Подсеть — в lists\warp-ips.txt.
$endpointIp = "162.159.192.1"

if ($Action -eq "check") {
    $url = [uri]::EscapeDataString("https://www.gstatic.com/generate_204")
    $deadline = (Get-Date).AddSeconds($Timeout)
    do {
        try {
            $r = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/proxies/WARP/delay?timeout=10000&url=$url" -TimeoutSec 12
            Write-Host "     WARP отвечает, задержка $($r.delay) мс"
            exit 0
        } catch {
            Start-Sleep -Seconds 1
        }
    } while ((Get-Date) -lt $deadline)
    exit 1
}

# ---- register ----
if (-not (Test-Path $wgcf)) { Write-Host "[!] Нет warp\wgcf.exe — запустите setup.ps1"; exit 1 }

Push-Location $warpDir
try {
    if (-not (Test-Path $account)) {
        Write-Host "     Регистрирую аккаунт WARP..."
        & $wgcf register --accept-tos
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path $account)) {
            Write-Host "[!] Регистрация не прошла — возможно, API Cloudflare режется DPI, попробуйте другую стратегию"
            exit 1
        }
    }
    if (-not (Test-Path $wgProfile)) {
        & $wgcf generate
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path $wgProfile)) { Write-Host "[!] wgcf не смог создать профиль"; exit 1 }
    }
} finally {
    Pop-Location
}

$conf = Get-Content $wgProfile -Raw
function Get-Field([string]$Name) {
    if ($conf -match "(?m)^\s*$Name\s*=\s*(.+?)\s*$") { return $Matches[1] }
    throw "в wgcf-profile.conf нет поля $Name"
}
$privateKey = Get-Field "PrivateKey"
$publicKey = Get-Field "PublicKey"
$addresses = (Get-Field "Address") -split "\s*,\s*"
$ipv4 = ($addresses | Where-Object { $_ -notmatch ":" } | Select-Object -First 1) -replace "/\d+$", ""
$ipv6 = ($addresses | Where-Object { $_ -match ":" } | Select-Object -First 1) -replace "/\d+$", ""
$endpointPort = ((Get-Field "Endpoint") -split ":")[-1]

# reserved — 3 байта client_id; без них WARP обычно работает, но с ними надёжнее. Кешируется в warp\reserved.txt
$reserved = $null
if (Test-Path $reservedFile) { $reserved = (Get-Content $reservedFile -Raw).Trim() }
if (-not $reserved) { try {
    $toml = Get-Content $account -Raw
    $deviceId = [regex]::Match($toml, "(?m)^device_id\s*=\s*['`"]([^'`"]+)").Groups[1].Value
    $token = [regex]::Match($toml, "(?m)^access_token\s*=\s*['`"]([^'`"]+)").Groups[1].Value
    $reg = Invoke-RestMethod -Uri "https://api.cloudflareclient.com/v0a1922/reg/$deviceId" -TimeoutSec 10 -Headers @{
        "Authorization" = "Bearer $token"; "CF-Client-Version" = "a-6.3-1922"; "User-Agent" = "okhttp/3.12.1"
    }
    $b = [Convert]::FromBase64String($reg.config.client_id)
    $reserved = "[$($b[0]), $($b[1]), $($b[2])]"
    Set-Content -Path $reservedFile -Value $reserved
} catch {
    Write-Host "     reserved получить не удалось, работаю без него"
} }

$yaml = @"
# Сгенерировано _warp.ps1 из warp\wgcf-profile.conf, пересоздаётся при каждом запуске — правки вносить в _warp.ps1.
proxies:
  - name: WARP-WG
    type: wireguard
    server: $endpointIp
    port: $endpointPort
    ip: $ipv4
    ipv6: $ipv6
    private-key: $privateKey
    public-key: $publicKey
    mtu: 1280
    udp: true
    # Мусорные пакеты перед handshake (AmneziaWG): DPI не узнаёт WireGuard, обычный WARP их просто отбрасывает
    amnezia-wg-option:
      jc: 4
      jmin: 40
      jmax: 70
"@
if ($reserved) { $yaml += "`r`n    reserved: $reserved" }
New-Item -ItemType Directory -Path (Split-Path $outYaml) -Force | Out-Null
[IO.File]::WriteAllText($outYaml, $yaml + "`r`n", (New-Object Text.UTF8Encoding $false))
Write-Host "     OK: профиль WARP в mihomo\warp.yaml"
exit 0
