param([string]$Toolchain = '1.95.0')
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location (Join-Path $root 'source')
try {
    cargo "+$Toolchain" fmt --check
    if ($LASTEXITCODE) { throw 'cargo "+$Toolchain" fmt failed' }
    cargo "+$Toolchain" test --locked
    if ($LASTEXITCODE) { throw 'cargo "+$Toolchain" test failed' }
    cargo "+$Toolchain" build --release --locked
    if ($LASTEXITCODE) { throw 'cargo "+$Toolchain" build failed' }
    Copy-Item -LiteralPath 'target/release/warp-unblock.exe' -Destination (Join-Path $root 'WarpUnblock.exe') -Force
} finally { Pop-Location }
Write-Output 'Built WarpUnblock.exe'
