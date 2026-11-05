param([switch]$InstallToolchain)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$cache = Join-Path $PSScriptRoot '.cache'
$cygwin = Join-Path $cache 'cygwin'
$source = Join-Path $cache 'zapret-source'
$commit = '87e058624c72863db53bdaf7fb6f16576dddb6ab'
$bash = Join-Path $cygwin 'bin/bash.exe'
New-Item -ItemType Directory -Path $cache -Force | Out-Null

if (-not (Test-Path -LiteralPath (Join-Path $cygwin 'bin/gcc.exe'))) {
    if (-not $InstallToolchain) { throw 'Run with -InstallToolchain to download a project-local Cygwin compiler (no registry or shortcuts).' }
    $installer = Join-Path $cache 'cygwin-setup.exe'
    if (-not (Test-Path -LiteralPath $installer)) { Invoke-WebRequest -UseBasicParsing 'https://cygwin.com/setup-x86_64.exe' -OutFile $installer }
    $arguments = @('--quiet-mode','hidden','--no-admin','--no-shortcuts','--no-startmenu','--no-desktop','--no-write-registry','--only-site','--site','https://mirrors.kernel.org/sourceware/cygwin/','--root',('"'+$cygwin+'"'),'--local-package-dir',('"'+(Join-Path $cache 'packages')+'"'),'--packages','gcc-core,make,zlib-devel,binutils')
    $process = Start-Process -FilePath $installer -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Cygwin setup failed: $($process.ExitCode)" }
}
if (-not (Test-Path -LiteralPath (Join-Path $source '.git'))) {
    git clone --depth 1 --branch v72.13 https://github.com/bol-van/zapret.git $source
    if ($LASTEXITCODE) { throw 'Source download failed' }
}
$actual = (git -C $source rev-parse HEAD).Trim()
if ($actual -ne $commit) { throw "Unexpected source revision: $actual" }
$dirty = git -C $source diff --name-only
$untracked = git -C $source ls-files --others --exclude-standard -- '*.c' '*.h' '*.a' 'Makefile'
if ($untracked) { throw 'Unexpected untracked compiler inputs in source tree' }
if ($dirty) { throw 'Source tree has local code modifications; inspect before building' }

# Rebuild our own resource instead of linking upstream precompiled .o resources.
$manifest = @'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges>
    <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
  </requestedPrivileges></security></trustInfo>
</assembly>
'@
[IO.File]::WriteAllText((Join-Path $source 'nfq/windows/res/selfbuild.manifest'), $manifest)
[IO.File]::WriteAllText((Join-Path $source 'nfq/windows/res/selfbuild.rc'), '1 24 "selfbuild.manifest"')
$shell = @'
set -eu
export PATH=/usr/bin:/bin
export SOURCE_DATE_EPOCH=1760000000
cd "$1/nfq/windows/res"
windres -i selfbuild.rc -o selfbuild.o
cd ../..
make cygwin64 RES_CYGWIN64=windows/res/selfbuild.o
'@
$shellPath = Join-Path $cache 'build-winws.sh'
[IO.File]::WriteAllText($shellPath, ($shell -replace "`r`n","`n")+"`n", [Text.UTF8Encoding]::new($false))
$cygpath = Join-Path $cygwin 'bin/cygpath.exe'
$posixScript = (& $cygpath -u $shellPath).Trim()
$posixSource = (& $cygpath -u $source).Trim()
& $bash --noprofile --norc $posixScript $posixSource
if ($LASTEXITCODE) { throw 'winws compilation failed' }
$destination = Join-Path $root 'app/bin/zapret'
Copy-Item -LiteralPath (Join-Path $source 'nfq/winws.exe') -Destination (Join-Path $destination 'winws.exe') -Force
# The executable must use the matching Cygwin runtime.
Copy-Item -LiteralPath (Join-Path $cygwin 'bin/cygwin1.dll') -Destination (Join-Path $destination 'cygwin1.dll') -Force
$hash = (Get-FileHash -LiteralPath (Join-Path $destination 'winws.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
$gcc = (& (Join-Path $cygwin 'bin/gcc.exe') --version | Select-Object -First 1)
@{repository='https://github.com/bol-van/zapret';tag='v72.13';commit=$commit;compiler=$gcc;sha256=$hash;resources='Locally rebuilt manifest, no upstream .o resources';driver='Existing signed WinDivert64.sys; not rebuilt'} |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $destination 'winws-build.json') -Encoding UTF8
Copy-Item -LiteralPath (Join-Path $source 'docs/LICENSE.txt') -Destination (Join-Path $destination 'LICENSE-zapret.txt') -Force
Write-Output "Self-built winws: $hash"
