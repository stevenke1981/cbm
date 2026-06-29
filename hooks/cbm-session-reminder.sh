#!/usr/bin/env bash
# SessionStart hook: remind agent to use cbm-mcp (CBM) tools.
set -euo pipefail
BIN="${CBM_BIN:-{{CBM_BIN}}}"
if [ -x "$BIN" ]; then
  "$BIN" hook-session-start
else
  cat << 'REMINDER'
CRITICAL - Code Discovery Protocol (CBM / cbm-mcp):
1. ALWAYS use cbm MCP tools FIRST for code exploration:
   - search_graph / rlm_filter to find functions, classes, routes
   - trace_path for call chains and data flow
   - rlm_read_symbol / get_code_snippet for exact symbol source
   - rlm_scan / rlm_peek / rlm_chunk for logs and huge non-code files
2. Project names use cbm+ prefix; run index_repository first if not indexed.
3. Use Grep/Glob/Read freely for configs; always Read a file before editing it.
REMINDER
fi
