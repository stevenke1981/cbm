#!/usr/bin/env bash
# Local release build — cross-compile common targets and package archives.
#
# Usage:
#   ./scripts/build-release.sh
#   ./scripts/build-release.sh --target x86_64-unknown-linux-gnu
#   ./scripts/build-release.sh --no-cross

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

NO_CROSS=false
TARGETS=()

while [ "$#" -gt 0 ]; do
  case "$1" in
    --no-cross) NO_CROSS=true ;;
    --target)
      shift
      TARGETS+=("$1")
      ;;
    -h|--help)
      echo "Usage: $0 [--no-cross] [--target TRIPLE ...]"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
  shift
done

if [ "${#TARGETS[@]}" -eq 0 ]; then
  if [ "$NO_CROSS" = true ]; then
    TARGETS=("native")
  else
    case "$(uname -s)" in
      Linux)
        TARGETS=("x86_64-unknown-linux-gnu")
        if [ "$(uname -m)" = "aarch64" ]; then
          TARGETS=("aarch64-unknown-linux-gnu")
        fi
        ;;
      Darwin)
        if [ "$(uname -m)" = "arm64" ]; then
          TARGETS=("aarch64-apple-darwin")
        else
          TARGETS=("x86_64-apple-darwin")
        fi
        ;;
      *)
        TARGETS=(
          "x86_64-unknown-linux-gnu"
          "x86_64-pc-windows-msvc"
          "aarch64-apple-darwin"
          "x86_64-apple-darwin"
        )
        ;;
    esac
  fi
fi

echo "Running tests..."
cargo test --all-targets

artifact_name() {
  case "$1" in
    native)
      local os arch
      os="$(uname -s | tr '[:upper:]' '[:lower:]')"
      arch="$(uname -m)"
      case "$arch" in
        x86_64|amd64) arch="x64" ;;
        aarch64|arm64) arch="arm64" ;;
      esac
      echo "cbrlm-${os}-${arch}"
      ;;
    x86_64-unknown-linux-gnu) echo "cbrlm-linux-x64" ;;
    aarch64-unknown-linux-gnu) echo "cbrlm-linux-arm64" ;;
    aarch64-unknown-linux-musl) echo "cbrlm-linux-arm64-musl" ;;
    x86_64-pc-windows-msvc) echo "cbrlm-windows-x64" ;;
    aarch64-apple-darwin) echo "cbrlm-macos-arm64" ;;
    x86_64-apple-darwin) echo "cbrlm-macos-x64" ;;
    *) echo "cbrlm-$1" ;;
  esac
}

binary_path() {
  local target="$1"
  if [ "$target" = "native" ]; then
    if [ "$(uname -s)" = "MINGW"* ] || [ "$(uname -s)" = "MSYS"* ] || [ -f "$ROOT/target/release/cbrlm.exe" ]; then
      echo "$ROOT/target/release/cbrlm.exe"
    else
      echo "$ROOT/target/release/cbrlm"
    fi
    return
  fi
  if [[ "$target" == *"windows"* ]]; then
    echo "$ROOT/target/$target/release/cbrlm.exe"
  else
    echo "$ROOT/target/$target/release/cbrlm"
  fi
}

install_target() {
  local target="$1"
  if ! rustup target list --installed | grep -qx "$target"; then
    echo "Installing Rust target: $target"
    rustup target add "$target"
  fi
}

rm -rf "$ROOT/dist"
mkdir -p "$ROOT/dist"

for target in "${TARGETS[@]}"; do
  name="$(artifact_name "$target")"
  echo ""
  echo "==> Building $name ($target)"
  if [ "$target" = "native" ]; then
    cargo build --release
  else
    install_target "$target"
    cargo build --release --target "$target"
  fi
  bin="$(binary_path "$target")"
  if [[ "$name" == *"windows"* ]]; then
    pwsh -NoProfile -File "$ROOT/scripts/package-artifact.ps1" "$name" "$bin"
  else
    bash "$ROOT/scripts/package-artifact.sh" "$name" "$bin"
  fi
done

{
  echo "# CBRLM release checksums"
  for f in "$ROOT/dist"/*.sha256; do
    [ -f "$f" ] && cat "$f"
  done
} > "$ROOT/dist/SHA256SUMS.txt" 2>/dev/null || true

echo ""
echo "Release artifacts:"
ls -la "$ROOT/dist"