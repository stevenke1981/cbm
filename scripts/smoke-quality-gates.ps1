# Section 4 quality gates + smoke checks (RUST_REWRITE_TODO.md).
#
# Usage:
#   .\scripts\smoke-quality-gates.ps1
#   .\scripts\smoke-quality-gates.ps1 -SkipBuild

param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Split-Path -Parent $Root
Set-Location $Root

Write-Host "==> cargo fmt --check" -ForegroundColor Cyan
cargo fmt --check
if ($LASTEXITCODE -ne 0) { throw "cargo fmt --check failed" }

Write-Host "==> cargo test --all-targets" -ForegroundColor Cyan
cargo test --all-targets
if ($LASTEXITCODE -ne 0) { throw "cargo test --all-targets failed" }

Write-Host "==> cargo clippy" -ForegroundColor Cyan
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { throw "cargo clippy failed" }

if (-not $SkipBuild) {
    Write-Host "==> cargo build --release" -ForegroundColor Cyan
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
}

$Bin = Join-Path $Root "target\release\cbm.exe"
if (-not (Test-Path $Bin)) {
    $Bin = Join-Path $Root "target\release\cbm"
}
if (-not (Test-Path $Bin)) {
    throw "release binary not found; run without -SkipBuild"
}

function Invoke-CbmCli([string[]]$CliArgs) {
    $out = & $Bin @CliArgs 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) {
        throw "cbm cli failed: $($CliArgs -join ' ')`n$out"
    }
    return $out
}

function Invoke-CbmCliStdout([string[]]$CliArgs) {
    $out = & $Bin @CliArgs 2>$null
    if ($LASTEXITCODE -ne 0) {
        throw "cbm cli failed: $($CliArgs -join ' ')"
    }
    return ($out | Out-String).Trim()
}

Write-Host "==> smoke: index_repository" -ForegroundColor Cyan
$indexOut = Invoke-CbmCliStdout @(
    'cli', 'index_repository', '--json', '--quiet',
    '{"repo_path":".","project":"smoke-review","mode":"fast","persistence":false}'
)
if ($indexOut -notmatch '"success":true') { throw "index_repository did not report success" }
if ($indexOut -notmatch '"edges_extracted":[1-9]') { throw "index_repository emitted no edges" }

Write-Host "==> smoke: search_graph" -ForegroundColor Cyan
$searchOut = Invoke-CbmCliStdout @(
    'cli', 'search_graph', '--json', '--quiet',
    '{"project":"smoke-review","query":"run_cli","limit":3}'
)
if ($searchOut -notmatch 'run_cli') { throw "search_graph did not find run_cli" }

Write-Host "==> smoke: get_architecture" -ForegroundColor Cyan
$archOut = Invoke-CbmCliStdout @(
    'cli', 'get_architecture', '--json', '--quiet',
    '{"project":"smoke-review"}'
)
foreach ($edge in @("CALLS", "CONTAINS", "IMPORTS")) {
    if ($archOut -notmatch $edge) { throw "get_architecture missing edge type $edge" }
}

Write-Host "==> smoke: query_graph edge diversity" -ForegroundColor Cyan
$queryOut = Invoke-CbmCliStdout @(
    'cli', 'query_graph', '--json', '--quiet',
    '{"project":"smoke-review","query":"SELECT edge_type, COUNT(*) AS count FROM edges GROUP BY edge_type"}'
)
try {
    $null = $queryOut | ConvertFrom-Json
} catch {
    throw "query_graph stdout is not valid JSON: $queryOut"
}
foreach ($edge in @("CALLS", "CONTAINS", "IMPORTS")) {
    if ($queryOut -notmatch $edge) { throw "query_graph missing edge type $edge" }
}

Write-Host "Section 4 quality gates passed." -ForegroundColor Green
