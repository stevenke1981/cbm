#!/usr/bin/env bash
# Install cbrlm from GitHub Release (Linux x64 / arm64).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/cbrlm/cbrlm/main/packaging/linux/install.sh | bash
#   CBRLM_VERSION=v0.1.0 ./packaging/linux/install.sh

set -euo pipefail

REPO="${CBRLM_REPO:-cbrlm/cbrlm}"
VERSION="${CBRLM_VERSION:-latest}"
INSTALL_DIR="${CBRLM_INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="${CBRLM_CONFIG_DIR:-$HOME/.config/cbrlm/bin}"

arch="$(uname -m)"
case "$arch" in
  x86_64|amd64) ARTIFACT="cbrlm-linux-x64" ;;
  aarch64|arm64) ARTIFACT="cbrlm-linux-arm64" ;;
  *)
    echo "Unsupported Linux architecture: $arch" >&2
    exit 1
    ;;
esac

if [ "$VERSION" = "latest" ]; then
  API="https://api.github.com/repos/${REPO}/releases/latest"
  VERSION="$(curl -fsSL "$API" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": "([^"]+)".*/\1/')"
fi

TAG="${VERSION#v}"
BASE="https://github.com/${REPO}/releases/download/${VERSION}"
ARCHIVE="${ARTIFACT}.tar.gz"
URL="${BASE}/${ARCHIVE}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading ${URL} ..."
curl -fsSL "$URL" -o "$TMP/${ARCHIVE}"

echo "Verifying checksum ..."
curl -fsSL "${BASE}/SHA256SUMS.txt" -o "$TMP/SHA256SUMS.txt"
expected="$(grep " ${ARCHIVE}$" "$TMP/SHA256SUMS.txt" | awk '{print $1}')"
if [ -z "$expected" ]; then
  echo "checksum for ${ARCHIVE} not found in SHA256SUMS.txt" >&2
  exit 1
fi
actual="$(sha256sum "$TMP/${ARCHIVE}" | awk '{print $1}')"
if [ "$actual" != "$expected" ]; then
  echo "checksum mismatch for ${ARCHIVE}" >&2
  exit 1
fi

tar -xzf "$TMP/${ARCHIVE}" -C "$TMP"

mkdir -p "$INSTALL_DIR" "$CONFIG_DIR"
install -m 755 "$TMP/cbrlm" "$CONFIG_DIR/cbrlm"
ln -sf "$CONFIG_DIR/cbrlm" "$INSTALL_DIR/cbrlm"

if ! echo ":$PATH:" | grep -q ":${INSTALL_DIR}:"; then
  echo ""
  echo "Add to PATH: export PATH=\"${INSTALL_DIR}:\$PATH\""
fi

if command -v cbrlm >/dev/null 2>&1; then
  echo "Configuring MCP agents..."
  cbrlm install --yes --all || true
fi

echo ""
echo "Installed cbrlm ${VERSION} → ${CONFIG_DIR}/cbrlm"