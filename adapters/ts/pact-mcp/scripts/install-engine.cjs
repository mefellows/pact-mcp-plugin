#!/usr/bin/env node
// Provision the pact-mcp-plugin engine binary into ~/.pact/plugins/mcp-<version>/,
// where the Pact plugin driver and this adapter's engine resolver both look.
//
// Runs automatically on `npm install` (postinstall) and can be re-run via the
// `pact-mcp-install` bin. Designed to NEVER hard-fail an install:
//   - skips when PACT_MCP_SKIP_INSTALL is set (CI / air-gapped / source builds),
//   - skips in this plugin's own repo (a local cargo build is used instead),
//   - skips if the target version is already installed,
//   - on any download error, warns and exits 0 with guidance (the binary can be
//     installed later via `npx pact-mcp-install` or scripts/install-plugin.sh).
"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const https = require("node:https");
const zlib = require("node:zlib");

const pkg = require(path.join(__dirname, "..", "package.json"));
const VERSION = process.env.PACT_MCP_ENGINE_VERSION || pkg.version;
const REPO = (pkg.engineConfig && pkg.engineConfig.repo) || "mefellows/pact-mcp-plugin";

function log(msg) {
  process.stdout.write(`[pact-mcp] ${msg}\n`);
}
function warn(msg) {
  process.stderr.write(`[pact-mcp] ${msg}\n`);
}

/** Map Node's platform/arch onto the release asset naming (see scripts/release.sh). */
function assetInfo() {
  const osName = { linux: "linux", darwin: "osx", win32: "windows" }[process.platform];
  const arch = { x64: "x86_64", arm64: "aarch64" }[process.arch];
  if (!osName || !arch) return null;
  const exe = process.platform === "win32" ? ".exe" : "";
  // e.g. pact-mcp-plugin-osx-aarch64.gz, pact-mcp-plugin-windows-x86_64.exe.gz
  const asset = `pact-mcp-plugin-${osName}-${arch}${exe}.gz`;
  return { asset, exe };
}

function download(url, redirects = 5) {
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { "User-Agent": "pact-mcp-install" } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          if (redirects === 0) return reject(new Error("too many redirects"));
          res.resume();
          return resolve(download(res.headers.location, redirects - 1));
        }
        if (res.statusCode !== 200) {
          res.resume();
          return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
        }
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve(Buffer.concat(chunks)));
        res.on("error", reject);
      })
      .on("error", reject);
  });
}

async function main() {
  if (process.env.PACT_MCP_SKIP_INSTALL) {
    log("PACT_MCP_SKIP_INSTALL set — skipping engine download.");
    return;
  }
  // In this plugin's own repo, a local cargo build is used (see engine.ts /
  // the test global-setup); never try to download.
  if (fs.existsSync(path.join(__dirname, "..", "..", "..", "..", "rust", "Cargo.toml"))) {
    log("detected the plugin source repo — skipping download (use scripts/install-local.sh).");
    return;
  }

  const dest = path.join(os.homedir(), ".pact", "plugins", `mcp-${VERSION}`);
  const info = assetInfo();
  if (!info) {
    warn(`unsupported platform ${process.platform}/${process.arch}; install the engine manually from https://github.com/${REPO}/releases`);
    return;
  }
  const binPath = path.join(dest, `pact-mcp-plugin${info.exe}`);
  if (fs.existsSync(binPath)) {
    log(`engine ${VERSION} already installed at ${dest}`);
    return;
  }

  const base = `https://github.com/${REPO}/releases/download/v${VERSION}`;
  try {
    log(`downloading engine ${VERSION} (${info.asset})…`);
    const gz = await download(`${base}/${info.asset}`);
    const bin = zlib.gunzipSync(gz);
    fs.mkdirSync(dest, { recursive: true });
    fs.writeFileSync(binPath, bin, { mode: 0o755 });

    log("downloading plugin manifest…");
    const manifest = await download(`${base}/pact-plugin.json`);
    fs.writeFileSync(path.join(dest, "pact-plugin.json"), manifest);

    log(`installed engine ${VERSION} → ${dest}`);
  } catch (err) {
    warn(`could not download the engine binary (${err.message}).`);
    warn(`This is non-fatal. Install it later with:  npx pact-mcp-install`);
    warn(`or build from source:  https://github.com/${REPO}#build--test`);
  }
}

main().catch((err) => {
  // Never fail the install.
  warn(`unexpected error: ${err.message}`);
});
