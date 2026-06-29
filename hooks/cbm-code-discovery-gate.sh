#!/usr/bin/env bash
# cbm-mcp search augmenter (Claude Code PreToolUse).
# NEVER blocks — only adds graph context. Failures are silent (exit 0).
set -euo pipefail
BIN="${CBM_BIN:-{{CBM_BIN}}}"
if [ ! -x "$BIN" ]; then exit 0; fi
"$BIN" hook-augment 2>/dev/null || true
exit 0
