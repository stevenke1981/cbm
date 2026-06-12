#!/usr/bin/env bash
# Install cbrlm (cbrlm-mcp) — build, copy binary, configure agents.
#
# Usage:
#   ./scripts/install.sh
#   ./scripts/install.sh --skip-build
#   ./scripts/install.sh --all-agents

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY="$ROOT_DIR/target/release/cbrlm"

SKIP_BUILD=false
ALL_AGENTS=false
for arg in "$@"; do
  case "$arg" in
    --skip-build) SKIP_BUILD=true ;;
    --all-agents) ALL_AGENTS=true ;;
  esac
done

GREEN='\033[0;32m'
GRAY='\033[0;90m'
NC='\033[0m'

if [ "$SKIP_BUILD" = false ]; then
  echo -e "${GRAY}Building release binary...${NC}"
  (cd "$ROOT_DIR" && cargo build --release)
fi

if [ ! -f "$BINARY" ]; then
  echo "Binary not found: $BINARY" >&2
  exit 1
fi

INSTALL_ARGS=(install --yes --force)
if [ "$ALL_AGENTS" = true ]; then
  INSTALL_ARGS+=(--all)
fi

echo -e "${GRAY}Running cbrlm install...${NC}"
"$BINARY" "${INSTALL_ARGS[@]}"

echo ""
echo -e "${GREEN}Done! Restart your coding agent.${NC}"
echo -e "${GRAY}MCP server: cbrlm-mcp${NC}"
echo -e "${GRAY}Binary:     $BINARY${NC}"