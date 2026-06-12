# Section 6.4 / 7.1 / 7.6 — verify packaged release archive end-to-end.
#
# Usage (local developer):
#   .\scripts\smoke-release-artifact.ps1
#   .\scripts\smoke-release-artifact.ps1 -SkipBuild
#
# Usage (CI matrix — archive already packaged):
#   .\scripts\smoke-release-artifact.ps1 -SkipBuild -SkipPackage `
#     -ArtifactName cbrlm-windows-x64 `
#     -ArchivePath dist\cbrlm-windows-x64.zip

param(
    [switch]$SkipBuild,
    [switch]$SkipPackage,
    [string]$ArtifactName = "cbrlm-windows-x64",
    [string]$BinaryPath = "",
    [string]$ArchivePath = "",
    [switch]$SkipMcpSmoke,
    [switch]$SkipInstallDryRun
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Split-Path -Parent $Root
Set-Location $Root

if (-not $ArchivePath) {
    $ArchivePath = Join-Path $Root "dist\$ArtifactName.zip"
}

if (-not $SkipBuild) {
    Write-Host "==> cargo build --release" -ForegroundColor Cyan
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
}

if (-not $BinaryPath) {
    $BinaryPath = Join-Path $Root "target\release\cbrlm.exe"
}

if (-not $SkipPackage) {
    if (-not (Test-Path $BinaryPath)) {
        throw "release binary not found: $BinaryPath"
    }
    Write-Host "==> package artifact" -ForegroundColor Cyan
    & (Join-Path $Root "scripts\package-artifact.ps1") $ArtifactName $BinaryPath
}

$Zip = $ArchivePath
$HashFile = [System.IO.Path]::ChangeExtension($Zip, ".sha256")
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

function Format-McpFrame([string]$Body) {
    $len = [System.Text.Encoding]::UTF8.GetByteCount($Body)
    return "Content-Length: $len`r`n`r`n$Body"
}

function Read-McpLine([System.IO.Stream]$Stream) {
    $bytes = New-Object System.Collections.Generic.List[byte]
    while ($true) {
        $b = $Stream.ReadByte()
        if ($b -lt 0) { return $null }
        if ($b -eq 10) { break }
        if ($b -ne 13) { [void]$bytes.Add([byte]$b) }
    }
    if ($bytes.Count -eq 0) { return "" }
    return [System.Text.Encoding]::UTF8.GetString($bytes.ToArray())
}

function Read-McpFrame([System.IO.Stream]$Stream) {
    $len = 0
    while ($true) {
        $line = Read-McpLine $Stream
        if ($null -eq $line -or $line -eq "") { break }
        if ($line -match '^Content-Length:\s*(\d+)') {
            $len = [int]$Matches[1]
        }
    }
    if ($len -le 0) { throw "MCP response missing Content-Length header" }
    $buf = New-Object byte[] $len
    $read = 0
    while ($read -lt $len) {
        $n = $Stream.Read($buf, $read, $len - $read)
        if ($n -le 0) { throw "unexpected EOF reading MCP body (read $read of $len)" }
        $read += $n
    }
    return [System.Text.Encoding]::UTF8.GetString($buf)
}

function Invoke-McpSmoke([string]$Binary) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Binary
    $psi.UseShellExecute = $false
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.Environment["CBRLM_WATCHER"] = "0"
    $proc = [System.Diagnostics.Process]::Start($psi)

    try {
        $init = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"1"}}}'
        $proc.StandardInput.Write((Format-McpFrame $init))
        $proc.StandardInput.Flush()
        $initResp = Read-McpFrame $proc.StandardOutput.BaseStream
        if ($initResp -notmatch '"result"') { throw "MCP initialize failed: $initResp" }

        $list = '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
        $proc.StandardInput.Write((Format-McpFrame $list))
        $proc.StandardInput.Flush()
        $listResp = Read-McpFrame $proc.StandardOutput.BaseStream
        if ($listResp -notmatch 'index_repository') { throw "MCP tools/list missing index_repository: $listResp" }
    } finally {
        try { $proc.StandardInput.Close() } catch {}
        if (-not $proc.WaitForExit(5000)) { $proc.Kill() }
    }
}

$SmokeCache = Join-Path $env:TEMP "cbrlm-smoke-cache"
if (Test-Path $SmokeCache) { Remove-Item $SmokeCache -Recurse -Force }
New-Item -ItemType Directory -Force -Path $SmokeCache | Out-Null
$env:CBRLM_CACHE_DIR = $SmokeCache
$env:CBRLM_WATCHER = "0"

Write-Host "==> smoke extracted binary" -ForegroundColor Cyan
& $Extracted --version
if ($LASTEXITCODE -ne 0) { throw "cbrlm --version failed" }

$indexJson = '{"repo_path":".","project":"smoke-artifact","mode":"fast","persistence":false}'
$indexOut = & $Extracted @('cli','index_repository','--json','--quiet',$indexJson) 2>$null
if ($LASTEXITCODE -ne 0) { throw "index_repository from extracted binary failed" }
if ($indexOut -notmatch '"success":true') { throw "index did not succeed" }

if (-not $SkipInstallDryRun) {
    Write-Host "==> smoke install dry-run" -ForegroundColor Cyan
    $dryOut = & $Extracted @('install','--dry-run','--all') 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) { throw "install --dry-run failed: $dryOut" }
    if ($dryOut -notmatch '\[dry-run\]') { throw "install dry-run produced no dry-run markers" }
}

if (-not $SkipMcpSmoke) {
    Write-Host "==> smoke MCP initialize + tools/list" -ForegroundColor Cyan
    Invoke-McpSmoke $Extracted
}

Write-Host "Release artifact smoke passed." -ForegroundColor Green