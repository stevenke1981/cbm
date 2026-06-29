# Install cbm from GitHub Release (Windows x64).
#
# Usage:
#   irm https://raw.githubusercontent.com/cbm/cbm/main/packaging/windows/install.ps1 | iex
#   $env:CBM_VERSION = "v0.1.0"; .\packaging\windows\install.ps1

param(
    [string]$Version = $(if ($env:CBM_VERSION) { $env:CBM_VERSION } else { "latest" }),
    [string]$Repo = $(if ($env:CBM_REPO) { $env:CBM_REPO } else { "cbm/cbm" })
)

$ErrorActionPreference = "Stop"

$InstallDir = if ($env:CBM_INSTALL_DIR) { $env:CBM_INSTALL_DIR } else { "$env:LOCALAPPDATA\cbm\bin" }
$Artifact = "cbm-windows-x64"
$Archive = "$Artifact.zip"

if ($Version -eq "latest") {
    $rel = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $rel.tag_name
}

$Url = "https://github.com/$Repo/releases/download/$Version/$Archive"
$Tmp = Join-Path $env:TEMP "cbm-install"
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
Copy-Item (Join-Path $Tmp "cbm.exe") (Join-Path $InstallDir "cbm.exe") -Force

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$InstallDir;$userPath", "User")
    $env:Path = "$InstallDir;$env:Path"
    Write-Host "Added $InstallDir to user PATH"
}

$bin = Join-Path $InstallDir "cbm.exe"
& $bin install --yes --all

Write-Host ""
Write-Host "Installed cbm $Version → $bin" -ForegroundColor Green
