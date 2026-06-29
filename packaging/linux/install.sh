#!/usr/bin/env bash
# Install cbm from GitHub Release (Linux x64 / arm64).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/cbm/cbm/main/packaging/linux/install.sh | bash
#   CBM_VERSION=v0.1.0 ./packaging/linux/install.sh

set -euo pipefail

REPO="${CBM_REPO:-cbm/cbm}"
VERSION="${CBM_VERSION:-latest}"
INSTALL_DIR="${CBM_INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="${CBM_CONFIG_DIR:-$HOME/.config/cbm/bin}"

arch="$(uname -m)"
case "$arch" in
  x86_64|amd64) ARTIFACT="cbm-linux-x64" ;;
  aarch64|arm64) ARTIFACT="cbm-linux-arm64" ;;
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
install -m 755 "$TMP/cbm" "$CONFIG_DIR/cbm"
ln -sf "$CONFIG_DIR/cbm" "$INSTALL_DIR/cbm"

if ! echo ":$PATH:" | grep -q ":${INSTALL_DIR}:"; then
  echo ""
  echo "Add to PATH: export PATH=\"${INSTALL_DIR}:\$PATH\""
fi

if command -v cbm >/dev/null 2>&1; then
  echo "Configuring MCP agents..."
  cbm install --yes --all || true
fi

echo ""
echo "Installed cbm ${VERSION} → ${CONFIG_DIR}/cbm"
