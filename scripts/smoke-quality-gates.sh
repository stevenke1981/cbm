#!/usr/bin/env bash
# Section 4 quality gates + smoke checks (RUST_REWRITE_TODO.md).
#
# Usage:
#   ./scripts/smoke-quality-gates.sh
#   ./scripts/smoke-quality-gates.sh --skip-build

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SKIP_BUILD=0
if [[ "${1:-}" == "--skip-build" ]]; then
  SKIP_BUILD=1
fi

echo "==> cargo fmt --check"
cargo fmt --check

echo "==> cargo test --all-targets"
cargo test --all-targets

echo "==> cargo clippy"
cargo clippy --all-targets -- -D warnings

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  echo "==> cargo build --release"
  cargo build --release
fi

if [[ -x "$ROOT/target/release/cbm" ]]; then
  BIN="$ROOT/target/release/cbm"
elif [[ -x "$ROOT/target/release/cbm.exe" ]]; then
  BIN="$ROOT/target/release/cbm.exe"
else
  echo "release binary not found; omit --skip-build" >&2
  exit 1
fi

INDEX_JSON='{"repo_path":".","project":"smoke-review","mode":"fast","persistence":false}'
SEARCH_JSON='{"project":"smoke-review","query":"run_cli","limit":3}'
ARCH_JSON='{"project":"smoke-review"}'

# Run CLI with --json --quiet; stdout is pure JSON, stderr is diagnostics.
run_cli() {
  "$BIN" "$@" 2>/dev/null
}

echo "==> smoke: index_repository"
index_out="$(run_cli cli index_repository --json --quiet "$INDEX_JSON")"
echo "$index_out" | grep -q '"success":true'
echo "$index_out" | grep -qE '"edges_extracted":[1-9]'

echo "==> smoke: search_graph"
search_out="$(run_cli cli search_graph --json --quiet "$SEARCH_JSON")"
echo "$search_out" | grep -q 'run_cli'

echo "==> smoke: get_architecture"
arch_out="$(run_cli cli get_architecture --json --quiet "$ARCH_JSON")"
for edge in CALLS CONTAINS IMPORTS; do
  echo "$arch_out" | grep -q "$edge"
done

echo "==> smoke: query_graph edge diversity"
query_out="$(run_cli cli query_graph --json --quiet '{"project":"smoke-review","query":"SELECT edge_type, COUNT(*) AS count FROM edges GROUP BY edge_type"}')"
# Validate JSON without python dependency
echo "$query_out" | grep -q '"edge_type"'
for edge in CALLS CONTAINS IMPORTS; do
  echo "$query_out" | grep -q "$edge"
done

echo "Section 4 quality gates passed."
