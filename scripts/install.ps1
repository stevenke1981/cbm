# Install cbrlm (cbrlm-mcp) — build, copy binary, configure agents.
#
# Usage:
#   .\scripts\install.ps1
#   .\scripts\install.ps1 -SkipBuild
#   .\scripts\install.ps1 -AllAgents

param(
    [switch]$SkipBuild,
    [switch]$AllAgents
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = Split-Path -Parent $ScriptDir
$BuiltBinary = Join-Path $RootDir "target\release\cbrlm.exe"

function Write-Step([string]$Msg) {
    Write-Host ""
    Write-Host $Msg -ForegroundColor DarkGray
}

if (-not $SkipBuild) {
    Write-Step "Building release binary..."
    Push-Location $RootDir
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path $BuiltBinary)) {
    throw "Binary not found: $BuiltBinary (run without -SkipBuild)"
}

Write-Step "Running cbrlm install..."
$installArgs = @("install", "--yes", "--force")
if ($AllAgents) { $installArgs += "--all" }

& $BuiltBinary @installArgs
if ($LASTEXITCODE -ne 0) { throw "cbrlm install failed" }

Write-Host ""
Write-Host "Done! Restart your coding agent." -ForegroundColor Green
Write-Host "MCP server: cbrlm-mcp" -ForegroundColor DarkGray
Write-Host "Binary:     $BuiltBinary" -ForegroundColor DarkGray