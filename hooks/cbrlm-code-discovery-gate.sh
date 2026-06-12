#!/usr/bin/env bash
# cbrlm-mcp search augmenter (Claude Code PreToolUse).
# NEVER blocks — only adds graph context. Failures are silent (exit 0).
set -euo pipefail
BIN="${CBRLM_BIN:-{{CBRLM_BIN}}}"
if [ ! -x "$BIN" ]; then exit 0; fi
"$BIN" hook-augment 2>/dev/null || true
exit 0