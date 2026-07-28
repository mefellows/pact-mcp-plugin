#!/usr/bin/env bash
# Install the pact-mcp-plugin from GitHub releases into ~/.pact/plugins.
# Mirrors the pact plugin ecosystem's install-plugin.sh convention.
set -euo pipefail

VERSION="0.1.0"
REPO="mefellows/pact-mcp-plugin"

case "$(uname -s)" in
  Linux)  os="linux" ;;
  Darwin) os="osx" ;;
  *) echo "Unsupported OS $(uname -s) — download manually from https://github.com/${REPO}/releases" >&2; exit 1 ;;
esac
arch="$(uname -m | sed 's/arm64/aarch64/')"

asset="pact-mcp-plugin-${os}-${arch}.gz"
base="https://github.com/${REPO}/releases/download/v${VERSION}"
dest="${HOME}/.pact/plugins/mcp-${VERSION}"

echo "Installing pact-mcp-plugin ${VERSION} (${os}-${arch}) -> ${dest}"
mkdir -p "${dest}"
curl -fsSL "${base}/${asset}" | gunzip -c > "${dest}/pact-mcp-plugin"
chmod +x "${dest}/pact-mcp-plugin"
curl -fsSL "${base}/pact-plugin.json" -o "${dest}/pact-plugin.json"
echo "Done."
