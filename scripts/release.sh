#!/usr/bin/env bash
# Build release artifacts for one platform, following the pact plugin release
# conventions (mirrors pactflow/pact-protobuf-plugin's release.sh):
#   target/artifacts/pact-mcp-plugin-<os>-<arch>.gz (+ .sha256)
#   target/artifacts/pact-plugin.json
#   target/artifacts/install-plugin.sh (+ .sha256)   [linux builder only]
#
# Usage: release.sh <Linux|Windows|macOS> <version tag, e.g. v0.1.0 or refs/tags/v0.1.0>
set -euo pipefail

if [ $# -lt 2 ]; then
  echo "Usage: $0 <Linux|Windows|macOS> <version tag>" >&2
  exit 1
fi

os="$1"
version="$(basename "$2" | sed -e 's/^v//')"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
artifacts="${repo_root}/rust/target/artifacts"
manifest="${repo_root}/pact-plugin.json"

cd "${repo_root}/rust"
mkdir -p "${artifacts}"
cp "${manifest}" "${artifacts}/"

package() { # <binary path> <asset name>
  gzip -c "$1" > "${artifacts}/$2.gz"
  openssl dgst -sha256 -r "${artifacts}/$2.gz" > "${artifacts}/$2.gz.sha256"
}

case "${os}" in
  Linux)
    cargo build --release
    package target/release/pact-mcp-plugin "pact-mcp-plugin-linux-$(uname -m)"
    sed -e "s/^VERSION=.*/VERSION=\"${version}\"/" "${repo_root}/scripts/install-plugin.sh" \
      > "${artifacts}/install-plugin.sh"
    openssl dgst -sha256 -r "${artifacts}/install-plugin.sh" > "${artifacts}/install-plugin.sh.sha256"
    ;;
  Windows)
    cargo build --release
    package target/release/pact-mcp-plugin.exe "pact-mcp-plugin-windows-x86_64.exe"
    ;;
  macOS)
    cargo build --release
    package target/release/pact-mcp-plugin "pact-mcp-plugin-osx-$(uname -m | sed 's/arm64/aarch64/')"
    ;;
  *)
    echo "${os} is not a recognised OS" >&2
    exit 1
    ;;
esac

ls -la "${artifacts}"
