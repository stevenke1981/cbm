#!/usr/bin/env bash
# Package a built cbrlm binary into a release archive.
# Usage: ./scripts/package-artifact.sh <artifact-name> <binary-path>
# Example: ./scripts/package-artifact.sh cbrlm-linux-x64 target/x86_64-unknown-linux-gnu/release/cbrlm

set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "Usage: $0 <artifact-name> <binary-path>" >&2
  exit 1
fi

ARTIFACT="$1"
BINARY="$2"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist"
STAGE="$DIST/stage-$ARTIFACT"

if [ ! -f "$BINARY" ]; then
  echo "Binary not found: $BINARY" >&2
  exit 1
fi

rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$BINARY" "$STAGE/"
if [ -f "$ROOT/LICENSE" ]; then
  cp "$ROOT/LICENSE" "$STAGE/"
fi

mkdir -p "$DIST"
OUT="$DIST/${ARTIFACT}.tar.gz"
tar -C "$STAGE" -czf "$OUT" .
rm -rf "$STAGE"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$DIST" && sha256sum "${ARTIFACT}.tar.gz" > "${ARTIFACT}.sha256")
elif command -v shasum >/dev/null 2>&1; then
  (cd "$DIST" && shasum -a 256 "${ARTIFACT}.tar.gz" | awk '{print $1 "  '"${ARTIFACT}.tar.gz"'"}' > "${ARTIFACT}.sha256")
fi

echo "Created $OUT"