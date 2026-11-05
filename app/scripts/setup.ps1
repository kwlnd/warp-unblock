# Internal dependency installer. Invoked by WarpUnblock.exe.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
$Root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$downloads = Join-Path $Root 'data/downloads'
New-Item -ItemType Directory -Path $downloads -Force | Out-Null

function Download([string]$Url, [string]$File) {
    Write-Output "Downloading $Url"
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $File
}

$zapret = Join-Path $Root 'app/bin/zapret'
if (-not (Test-Path -LiteralPath (Join-Path $zapret 'winws.exe'))) { throw 'Self-built winws is missing. Run tools/build-winws.ps1; upstream winws will not be substituted.' }
$required = @('WinDivert.dll','WinDivert64.sys','cygwin1.dll','tls_clienthello_www_google_com.bin')
if (@($required | Where-Object { -not (Test-Path -LiteralPath (Join-Path $zapret $_)) }).Count) {
    $archive = Join-Path $downloads 'zapret-v72.13.zip'
    Download 'https://github.com/bol-van/zapret/releases/download/v72.13/zapret-v72.13.zip' $archive
    $extract = Join-Path $downloads 'zapret-v72.13'
    Expand-Archive -LiteralPath $archive -DestinationPath $extract -Force
    $winws = Get-ChildItem -LiteralPath $extract -Recurse -Filter winws.exe |
        Where-Object { $_.DirectoryName -match 'x86_64|x86-64|win64' } | Select-Object -First 1
    if (-not $winws) { throw 'No Windows x64 winws in upstream archive' }
    New-Item -ItemType Directory -Path $zapret -Force | Out-Null
    foreach ($name in $required) {
        if (Test-Path -LiteralPath (Join-Path $zapret $name)) { continue }
        $file = if ($name -eq 'tls_clienthello_www_google_com.bin') {
            Get-ChildItem -LiteralPath $extract -Recurse -Filter $name | Select-Object -First 1
        } else { Get-Item -LiteralPath (Join-Path $winws.DirectoryName $name) }
        if (-not $file) { throw "No $name in upstream archive" }
        if ($name -eq 'WinDivert64.sys' -and (Get-AuthenticodeSignature -LiteralPath $file.FullName).Status -ne 'Valid') { throw 'WinDivert driver signature verification failed' }
        Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $zapret $name)
    }
}

$mihomo = Join-Path $Root 'app/bin/mihomo'
if (-not (Test-Path -LiteralPath (Join-Path $mihomo 'mihomo.exe'))) {
    $archive = Join-Path $downloads 'mihomo-v1.19.33.zip'
    Download 'https://github.com/MetaCubeX/mihomo/releases/download/v1.19.33/mihomo-windows-amd64-compatible-v1.19.33.zip' $archive
    $extract = Join-Path $downloads 'mihomo-v1.19.33'
    Expand-Archive -LiteralPath $archive -DestinationPath $extract -Force
    New-Item -ItemType Directory -Path $mihomo -Force | Out-Null
    $file = Get-ChildItem -LiteralPath $extract -Recurse -Filter 'mihomo*.exe' | Select-Object -First 1
    if (-not $file) { throw 'No mihomo executable in upstream archive' }
    Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $mihomo 'mihomo.exe')
}

$wgcf = Join-Path $Root 'app/bin/wgcf'
if (-not (Test-Path -LiteralPath (Join-Path $wgcf 'wgcf.exe'))) {
    New-Item -ItemType Directory -Path $wgcf -Force | Out-Null
    Download 'https://github.com/ViRb3/wgcf/releases/download/v2.3.1/wgcf_2.3.1_windows_amd64.exe' (Join-Path $wgcf 'wgcf.exe')
}
Write-Output 'Dependencies ready'
