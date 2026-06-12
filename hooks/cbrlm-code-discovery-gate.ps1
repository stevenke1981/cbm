# cbrlm-mcp search augmenter (Claude Code PreToolUse).
# NEVER blocks a tool call — only adds graph context. Failures are silent (exit 0).
param()
$ErrorActionPreference = "SilentlyContinue"
$bin = if ($env:CBRLM_BIN) { $env:CBRLM_BIN } else { "{{CBRLM_BIN}}" }
if (-not (Test-Path $bin)) { exit 0 }
& $bin hook-augment 2>$null
exit 0