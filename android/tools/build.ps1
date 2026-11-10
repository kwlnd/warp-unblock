param([string[]]$Tasks = @('testDebugUnitTest', 'assembleRelease', 'lintRelease'))
# Локальная сборка Android без установки чего-либо в систему: JDK 17, Gradle 8.9 и Android SDK
# лежат в android/.toolchain (см. docs/DEVELOPMENT.md), кэши Gradle и Android — там же.
$ErrorActionPreference = 'Stop'
$android = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$tc = Join-Path $android '.toolchain'
$jdk = Get-ChildItem (Join-Path $tc 'jdk17') -Directory | Select-Object -First 1
if (-not $jdk) { throw "Нет JDK в $tc\jdk17 — см. docs/DEVELOPMENT.md" }
$env:JAVA_HOME = $jdk.FullName
$env:ANDROID_HOME = Join-Path $tc 'sdk'
$env:ANDROID_USER_HOME = Join-Path $tc 'android-home'
$env:GRADLE_USER_HOME = Join-Path $tc 'gradle-home'
$gradle = Join-Path $tc 'gradle\gradle-8.9\bin\gradle.bat'
& $gradle -p $android @Tasks --no-daemon
if ($LASTEXITCODE) { throw "gradle failed: $LASTEXITCODE" }
