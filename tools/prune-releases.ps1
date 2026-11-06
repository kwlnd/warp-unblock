param([switch]$PrepareBackups, [switch]$Apply)
$ErrorActionPreference = 'Stop'
$repo = 'kwlnd/warp-unblock'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$backup = Join-Path $root 'dist/release-backups'

function Invoke-Gh([string[]]$Arguments) {
    $output = & gh @Arguments
    if ($LASTEXITCODE) { throw "GitHub command failed: $($Arguments[0])" }
    return $output
}

$releases = Invoke-Gh -Arguments @('release', 'list', '--repo', $repo, '--limit', '1000', '--json', 'tagName,isDraft,isPrerelease,publishedAt') | ConvertFrom-Json
$stable = @($releases | Where-Object { -not $_.isDraft -and -not $_.isPrerelease -and $_.tagName -match '^(android-)?v\d+\.\d+\.\d+$' })
$keep = @()
foreach ($android in @($false, $true)) {
    $latest = $stable | Where-Object { $_.tagName.StartsWith('android-') -eq $android } | Sort-Object publishedAt -Descending | Select-Object -First 1
    if ($latest) { $keep += $latest.tagName }
}
if ($keep.Count -ne 2) { throw 'Expected a stable release for both platforms; nothing will be deleted.' }
$remove = @($stable | Where-Object { $_.tagName -notin $keep })
Write-Output "Keep: $($keep -join ', ')"
Write-Output "Remove $($remove.Count) releases: $($remove.tagName -join ', ')"
Write-Output 'Git tags are preserved. Default mode only shows this plan.'
if (-not $PrepareBackups -and -not $Apply) { return }

New-Item -ItemType Directory -Force $backup | Out-Null
@{ repository = $repo; keep = $keep; remove = @($remove.tagName) } | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 (Join-Path $backup 'plan.json')
foreach ($release in $remove) {
    $tag = $release.tagName
    $dir = Join-Path $backup $tag
    New-Item -ItemType Directory -Force $dir | Out-Null
    $raw = Invoke-Gh -Arguments @('release', 'view', $tag, '--repo', $repo, '--json', 'tagName,name,body,assets,publishedAt,targetCommitish,url')
    $raw | Set-Content -Encoding UTF8 (Join-Path $dir 'release.json')
    $meta = $raw | ConvertFrom-Json
    Invoke-Gh -Arguments @('release', 'download', $tag, '--repo', $repo, '--dir', $dir, '--skip-existing') | Out-Null
    foreach ($asset in $meta.assets) {
        if ([IO.Path]::GetFileName($asset.name) -cne $asset.name) { throw "Unsafe asset name in $tag" }
        $path = Join-Path $dir $asset.name
        if (-not (Test-Path -LiteralPath $path) -or (Get-Item -LiteralPath $path).Length -ne $asset.size) { throw "Incomplete backup: $tag / $($asset.name)" }
    }
    # Verify published checksum files, then save our hashes for the complete archive.
    foreach ($sum in Get-ChildItem -LiteralPath $dir -File | Where-Object { $_.Name -match '(?i)(sha256|SHA256SUMS\.txt)$' }) {
        foreach ($line in Get-Content -LiteralPath $sum.FullName) {
            if ($line -match '^([a-fA-F0-9]{64})\s+\*?(.+)$') {
                $expected = $Matches[1]; $name = $Matches[2].Trim()
                if ([IO.Path]::GetFileName($name) -cne $name) { throw 'Unsafe checksum path' }
                if ((Get-FileHash -LiteralPath (Join-Path $dir $name) -Algorithm SHA256).Hash -ine $expected) { throw "Checksum mismatch: $tag / $name" }
            }
        }
    }
    Get-ChildItem -LiteralPath $dir -File | Where-Object Name -ne 'backup-hashes.json' | ForEach-Object {
        @{ name = $_.Name; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash }
    } | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $dir 'backup-hashes.json')
}
Write-Output "Backups verified: $backup"
if ($Apply) {
    foreach ($release in $remove) {
        Invoke-Gh -Arguments @('release', 'delete', $release.tagName, '--repo', $repo, '--yes') | Out-Null
    }
    Write-Output 'Old releases deleted; tags and local backups preserved.'
}
