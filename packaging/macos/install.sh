#!/usr/bin/env bash
# Install cbm from GitHub Release (macOS x64 / Apple Silicon).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/cbm/cbm/main/packaging/macos/install.sh | bash
#   CBM_VERSION=v0.1.0 ./packaging/macos/install.sh

set -euo pipefail

REPO="${CBM_REPO:-cbm/cbm}"
VERSION="${CBM_VERSION:-latest}"
INSTALL_DIR="${CBM_INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="${CBM_CONFIG_DIR:-$HOME/.config/cbm/bin}"

arch="$(uname -m)"
case "$arch" in
  x86_64) ARTIFACT="cbm-macos-x64" ;;
  arm64) ARTIFACT="cbm-macos-arm64" ;;
  *)
    echo "Unsupported macOS architecture: $arch" >&2
    exit 1
    ;;
esac

if [ "$VERSION" = "latest" ]; then
  API="https://api.github.com/repos/${REPO}/releases/latest"
  VERSION="$(curl -fsSL "$API" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": "([^"]+)".*/\1/')"
fi

BASE="https://github.com/${REPO}/releases/download/${VERSION}"
ARCHIVE="${ARTIFACT}.tar.gz"
URL="${BASE}/${ARCHIVE}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading ${URL} ..."
curl -fsSL "$URL" -o "$TMP/${ARCHIVE}"
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
