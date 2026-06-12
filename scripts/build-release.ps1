# Local release build — Windows native + optional Linux cross-compile.
#
# Usage:
#   .\scripts\build-release.ps1              # Windows x64 only
#   .\scripts\build-release.ps1 -All         # Windows + Linux x64 (needs zig)
#   .\scripts\build-release.ps1 -Targets @("x86_64-unknown-linux-gnu")

param(
    [switch]$All,
    [string[]]$Targets = @()
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Split-Path -Parent $Root
Set-Location $Root

if ($All -and $Targets.Count -eq 0) {
    $Targets = @(
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu"
    )
    Write-Host "Note: macOS builds require macOS runners (see GitHub Release workflow)." -ForegroundColor DarkGray
    Write-Host "      Linux cross-compile on Windows requires: zig + cargo install cargo-zigbuild" -ForegroundColor DarkGray
}

if ($Targets.Count -eq 0) {
    $Targets = @("x86_64-pc-windows-msvc")
}

Write-Host "Running tests..."
cargo test --all-targets
if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

function Get-ArtifactName([string]$Target) {
    switch ($Target) {
        "x86_64-pc-windows-msvc" { "cbrlm-windows-x64" }
        "aarch64-pc-windows-msvc" { "cbrlm-windows-arm64" }
        "x86_64-unknown-linux-gnu" { "cbrlm-linux-x64" }
        "aarch64-unknown-linux-gnu" { "cbrlm-linux-arm64" }
        "aarch64-apple-darwin" { "cbrlm-macos-arm64" }
        "x86_64-apple-darwin" { "cbrlm-macos-x64" }
        default { "cbrlm-$Target" }
    }
}

function Get-BinaryPath([string]$Target) {
    if ($Target -like "*windows*") {
        return Join-Path $Root "target\$Target\release\cbrlm.exe"
    }
    return Join-Path $Root "target\$Target\release\cbrlm"
}

function Build-Target([string]$Target) {
    $installed = rustup target list --installed
    if ($installed -notcontains $Target) {
        rustup target add $Target
    }

    if ($Target -like "*linux*" -and $IsWindows) {
        $zigbuild = Get-Command cargo-zigbuild -ErrorAction SilentlyContinue
        $zig = Get-Command zig -ErrorAction SilentlyContinue
        if ($zigbuild -and $zig) {
            cargo zigbuild --release --target $Target
            return
        }
        Write-Warning "Skipping $Target — install zig + cargo-zigbuild for Linux cross-compile on Windows"
        Write-Warning "  winget install zig.zig"
        Write-Warning "  cargo install cargo-zigbuild"
        return
    }

    cargo build --release --target $Target
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $Target" }
}

$Dist = Join-Path $Root "dist"
if (Test-Path $Dist) { Remove-Item $Dist -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

foreach ($target in $Targets) {
    $name = Get-ArtifactName $target
    Write-Host ""
    Write-Host "==> Building $name ($target)"

    Build-Target $target
    $bin = Get-BinaryPath $target
    if (-not (Test-Path $bin)) {
        Write-Warning "Binary not found, skipping package: $bin"
        continue
    }

    if ($name -like "*windows*") {
        & (Join-Path $Root "scripts\package-artifact.ps1") $name $bin
    } else {
        bash (Join-Path $Root "scripts\package-artifact.sh") $name $bin
    }
}

$sums = Join-Path $Dist "SHA256SUMS.txt"
if (Test-Path $sums) { Remove-Item $sums -Force }
Get-ChildItem $Dist -Filter "*.sha256" -ErrorAction SilentlyContinue | ForEach-Object {
    Add-Content $sums (Get-Content $_.FullName)
}

Write-Host ""
Write-Host "Release artifacts:"
Get-ChildItem $Dist | Format-Table Name, Length