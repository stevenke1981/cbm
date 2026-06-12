# Install cbrlm from GitHub Release (Windows x64).
#
# Usage:
#   irm https://raw.githubusercontent.com/cbrlm/cbrlm/main/packaging/windows/install.ps1 | iex
#   $env:CBRLM_VERSION = "v0.1.0"; .\packaging\windows\install.ps1

param(
    [string]$Version = $(if ($env:CBRLM_VERSION) { $env:CBRLM_VERSION } else { "latest" }),
    [string]$Repo = $(if ($env:CBRLM_REPO) { $env:CBRLM_REPO } else { "cbrlm/cbrlm" })
)

$ErrorActionPreference = "Stop"

$InstallDir = if ($env:CBRLM_INSTALL_DIR) { $env:CBRLM_INSTALL_DIR } else { "$env:LOCALAPPDATA\cbrlm\bin" }
$Artifact = "cbrlm-windows-x64"
$Archive = "$Artifact.zip"

if ($Version -eq "latest") {
    $rel = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $rel.tag_name
}

$Url = "https://github.com/$Repo/releases/download/$Version/$Archive"
$Tmp = Join-Path $env:TEMP "cbrlm-install"
New-Item -ItemType Directory -Force -Path $Tmp, $InstallDir | Out-Null

$ChecksumsUrl = "https://github.com/$Repo/releases/download/$Version/SHA256SUMS.txt"
$ArchivePath = Join-Path $Tmp $Archive

Write-Host "Downloading $Url ..."
Invoke-WebRequest -Uri $Url -OutFile $ArchivePath

Write-Host "Verifying checksum ..."
$sums = Invoke-WebRequest -Uri $ChecksumsUrl -UseBasicParsing
$expected = ($sums.Content -split "`n" | Where-Object { $_ -match "\s+$([regex]::Escape($Archive))`$" } | ForEach-Object { ($_ -split '\s+')[0] } | Select-Object -First 1)
if (-not $expected) {
    throw "checksum for $Archive not found in SHA256SUMS.txt"
}
$actual = (Get-FileHash -Path $ArchivePath -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $expected.ToLower()) {
    throw "checksum mismatch for $Archive (expected $expected, got $actual)"
}

Expand-Archive -Path $ArchivePath -DestinationPath $Tmp -Force
Copy-Item (Join-Path $Tmp "cbrlm.exe") (Join-Path $InstallDir "cbrlm.exe") -Force

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$InstallDir;$userPath", "User")
    $env:Path = "$InstallDir;$env:Path"
    Write-Host "Added $InstallDir to user PATH"
}

$bin = Join-Path $InstallDir "cbrlm.exe"
& $bin install --yes --all

Write-Host ""
Write-Host "Installed cbrlm $Version → $bin" -ForegroundColor Green