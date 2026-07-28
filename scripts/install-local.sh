#!/usr/bin/env bash
# Build the engine (release) and install it where the pact plugin driver looks:
#   ~/.pact/plugins/mcp-<version>/{pact-mcp-plugin,pact-plugin.json}
# The version is read from pact-plugin.json. See ADR 0008.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="$(node -p "require('${repo_root}/pact-plugin.json').version")"
dest="${HOME}/.pact/plugins/mcp-${version}"

echo "Building pact-mcp-plugin (release)..." >&2
cargo build --release --manifest-path "${repo_root}/rust/Cargo.toml" >&2

mkdir -p "${dest}"
cp "${repo_root}/rust/target/release/pact-mcp-plugin" "${dest}/"
cp "${repo_root}/pact-plugin.json" "${dest}/"

echo "Installed pact-mcp-plugin ${version} -> ${dest}" >&2
echo "${dest}"
