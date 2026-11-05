<#
.SYNOPSIS
    WARP-UNBLOCK Setup — скачивает zapret, mihomo и wgcf из официальных релизов на GitHub.
.DESCRIPTION
    Всё кладётся внутрь проекта (zapret\, mihomo\, warp\), в систему ничего не ставится.
    Регистрация WARP делается позже, в _warp.ps1, когда уже работает zapret.
#>
param([switch]$NoPause)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProgressPreference = "SilentlyContinue"  # иначе Invoke-WebRequest в PS 5.1 качает в разы медленнее

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Start-Process powershell -Verb RunAs -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$($MyInvocation.MyCommand.Path)`""
    exit
}
chcp.com 866 > $null  # Та же кодовая страница, что у bat/cmd; работает с растровым шрифтом.

$zapretDir = Join-Path $Root "zapret"
$mihomoDir = Join-Path $Root "mihomo"
$warpDir = Join-Path $Root "warp"
$failed = @()

# Ищет в последнем релизе repo первый ассет, подходящий под один из шаблонов (по порядку), и качает его в $OutFile
function Get-ReleaseAsset([string]$Repo, [string[]]$Patterns, [string]$OutFile) {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "warp-unblock" }
    $asset = $null
    foreach ($p in $Patterns) {
        $asset = $release.assets | Where-Object { $_.name -match $p } | Select-Object -First 1
        if ($asset) { break }
    }
    if (-not $asset) { throw "в релизе $($release.tag_name) нет подходящей сборки" }
    Write-Host "     Качаю: $($asset.name) ($([math]::Round($asset.size/1MB, 1)) MB)..."
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $OutFile -UseBasicParsing
}

function Install-Step([string]$Title, [string]$Check, [string]$Url, [scriptblock]$Body) {
    if (Test-Path $Check) {
        Write-Host "$Title уже установлен" -ForegroundColor Green
        return
    }
    Write-Host "$Title — скачиваю..." -ForegroundColor Yellow
    try {
        & $Body
        Write-Host "     OK" -ForegroundColor Green
    } catch {
        Write-Host "     [!] Ошибка: $_" -ForegroundColor Red
        Write-Host "         Скачайте вручную: $Url" -ForegroundColor Red
        $script:failed += $Title
    }
}

Write-Host ""
Write-Host "╔══════════════════════════════════════════════════════════════╗" -ForegroundColor Cyan
Write-Host "║       WARP-UNBLOCK — Установка зависимостей                  ║" -ForegroundColor Cyan
Write-Host "╚══════════════════════════════════════════════════════════════╝" -ForegroundColor Cyan
Write-Host ""

# ============================================================================
#  1. Zapret — один zip под все платформы, берём сборку windows-x86_64 и fake-пакеты
# ============================================================================
Install-Step "[1/3] Zapret" (Join-Path $zapretDir "winws.exe") "https://github.com/bol-van/zapret/releases" {
    $zipPath = Join-Path $Root "zapret.zip"
    $tmpDir = Join-Path $Root "zapret-tmp"
    Get-ReleaseAsset "bol-van/zapret" @('^zapret-v[\d.]+\.zip$') $zipPath
    if (Test-Path $tmpDir) { Remove-Item $tmpDir -Recurse -Force }
    Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force

    $winws = Get-ChildItem -Path $tmpDir -Recurse -Filter "winws.exe" |
        Where-Object { $_.DirectoryName -match "x86_64|win64" } | Select-Object -First 1
    if (-not $winws) { throw "в архиве нет winws.exe для x86_64" }

    New-Item -ItemType Directory -Path $zapretDir -Force | Out-Null
    Copy-Item -Path (Join-Path $winws.DirectoryName "*") -Destination $zapretDir -Recurse -Force
    Get-ChildItem -Path $tmpDir -Recurse -Filter "*.bin" |
        Where-Object { $_.DirectoryName -match "fake" } |
        Copy-Item -Destination $zapretDir -Force

    Remove-Item $tmpDir -Recurse -Force
    Remove-Item $zipPath -Force
}

# ============================================================================
#  2. mihomo — compatible: сборка без требований к набору инструкций CPU
# ============================================================================
Install-Step "[2/3] mihomo" (Join-Path $mihomoDir "mihomo.exe") "https://github.com/MetaCubeX/mihomo/releases" {
    $zipPath = Join-Path $Root "mihomo.zip"
    Get-ReleaseAsset "MetaCubeX/mihomo" @('^mihomo-windows-amd64-compatible-v[\d.]+\.zip$', '^mihomo-windows-amd64.*\.zip$') $zipPath
    New-Item -ItemType Directory -Path $mihomoDir -Force | Out-Null
    Expand-Archive -Path $zipPath -DestinationPath $mihomoDir -Force
    $exe = Get-ChildItem -Path $mihomoDir -Filter "mihomo*.exe" | Select-Object -First 1
    if ($exe -and $exe.Name -ne "mihomo.exe") { Rename-Item $exe.FullName "mihomo.exe" }
    Remove-Item $zipPath -Force
}

# ============================================================================
#  3. wgcf — регистрирует аккаунт WARP и выдаёт WireGuard-профиль
# ============================================================================
Install-Step "[3/3] wgcf" (Join-Path $warpDir "wgcf.exe") "https://github.com/ViRb3/wgcf/releases" {
    New-Item -ItemType Directory -Path $warpDir -Force | Out-Null
    Get-ReleaseAsset "ViRb3/wgcf" @('^wgcf_[\d.]+_windows_amd64\.exe$') (Join-Path $warpDir "wgcf.exe")
}

New-Item -ItemType Directory -Path (Join-Path $mihomoDir "rule-sets") -Force | Out-Null
if ((Test-Path $zapretDir) -and -not (Test-Path (Join-Path $zapretDir "tls_clienthello_www_google_com.bin"))) {
    Write-Host "[!] Нет zapret\tls_clienthello_www_google_com.bin — TCP-стратегии не запустятся" -ForegroundColor Red
}

Write-Host ""
if ($failed.Count -gt 0) {
    Write-Host "  Не установлено: $($failed -join ', ')" -ForegroundColor Red
} else {
    Write-Host "  Готово. Подберите стратегию через test-strategies.bat, затем запускайте start.bat." -ForegroundColor Green
}
Write-Host ""

if (-not $NoPause) { Read-Host "Нажмите Enter для выхода" }
if ($failed.Count -gt 0) { exit 1 }
