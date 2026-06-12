# Section 6.4 — verify packaged release archive end-to-end.
#
# Usage:
#   .\scripts\smoke-release-artifact.ps1
#   .\scripts\smoke-release-artifact.ps1 -SkipBuild

param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Split-Path -Parent $Root
Set-Location $Root

if (-not $SkipBuild) {
    Write-Host "==> cargo build --release" -ForegroundColor Cyan
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
}

$Bin = Join-Path $Root "target\release\cbrlm.exe"
if (-not (Test-Path $Bin)) {
    throw "release binary not found: $Bin"
}

$Artifact = "cbrlm-windows-x64"
Write-Host "==> package artifact" -ForegroundColor Cyan
& (Join-Path $Root "scripts\package-artifact.ps1") $Artifact $Bin

$Zip = Join-Path $Root "dist\$Artifact.zip"
$HashFile = Join-Path $Root "dist\$Artifact.sha256"
if (-not (Test-Path $Zip)) { throw "archive missing: $Zip" }
if (-not (Test-Path $HashFile)) { throw "checksum file missing: $HashFile" }

Write-Host "==> verify checksum" -ForegroundColor Cyan
$expected = (Get-Content $HashFile -Raw).Split()[0].ToLower()
$actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $expected) {
    throw "checksum mismatch (expected $expected, got $actual)"
}

$Extract = Join-Path $env:TEMP "cbrlm-smoke-release"
if (Test-Path $Extract) { Remove-Item $Extract -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Extract | Out-Null
Expand-Archive -Path $Zip -DestinationPath $Extract -Force

$Extracted = Join-Path $Extract "cbrlm.exe"
if (-not (Test-Path $Extracted)) { throw "extracted binary missing" }

Write-Host "==> smoke extracted binary" -ForegroundColor Cyan
& $Extracted --version
if ($LASTEXITCODE -ne 0) { throw "cbrlm --version failed" }

$indexJson = '{"repo_path":".","project":"smoke-artifact","mode":"fast","persistence":false}'
$indexOut = & $Extracted @('cli','index_repository','--json','--quiet',$indexJson) 2>$null
if ($LASTEXITCODE -ne 0) { throw "index_repository from extracted binary failed" }
if ($indexOut -notmatch '"success":true') { throw "index did not succeed" }

Write-Host "Release artifact smoke passed." -ForegroundColor Green