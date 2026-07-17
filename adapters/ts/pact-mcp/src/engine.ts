// Locates the pact-mcp-plugin engine binary and runs its `compare` / `verify`
// CLI subcommands. The adapter delegates ALL matching to this engine.

import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/**
 * Resolve the engine binary, in priority order:
 *  1. PACT_MCP_ENGINE env var (explicit override).
 *  2. An installed plugin at ~/.pact/plugins/mcp-<version>/pact-mcp-plugin.
 *  3. A local dev build under rust/target/{debug,release} (relative to this repo).
 */
export function resolveEngine(): string {
  const override = process.env.PACT_MCP_ENGINE;
  if (override && existsSync(override)) return override;

  const pluginsDir = join(homedir(), ".pact", "plugins");
  if (existsSync(pluginsDir)) {
    const mcpDirs = readdirSync(pluginsDir)
      .filter((d) => d.startsWith("mcp-"))
      .sort();
    for (const d of mcpDirs.reverse()) {
      const p = join(pluginsDir, d, "pact-mcp-plugin");
      if (existsSync(p)) return p;
    }
  }

  // repo-relative dev build (adapters/ts/pact-mcp/src -> repo root is ../../../..)
  const repoRoot = join(__dirname, "..", "..", "..", "..");
  for (const profile of ["release", "debug"]) {
    const p = join(repoRoot, "rust", "target", profile, "pact-mcp-plugin");
    if (existsSync(p)) return p;
  }

  throw new Error(
    "Could not locate the pact-mcp-plugin engine binary. Set PACT_MCP_ENGINE, " +
      "install the plugin at ~/.pact/plugins/mcp-<version>/, or build rust/ (cargo build)."
  );
}

export interface CompareResult {
  match: boolean;
  mismatchPaths: string[];
}

/** Run `compare --fixture <file>` and parse the JSON result. */
export function runCompare(fixturePath: string, engine = resolveEngine()): CompareResult {
  const out = spawnSync(engine, ["compare", "--fixture", fixturePath], { encoding: "utf8" });
  if (out.status !== 0) {
    throw new Error(`engine compare failed (${out.status}): ${out.stderr}`);
  }
  return parseLastJson(out.stdout) as CompareResult;
}

export interface VerifyInteractionResult {
  description: string;
  success: boolean;
  mismatches?: { path: string; message: string }[];
  error?: string;
}
export interface VerifyResult {
  success: boolean;
  interactions: VerifyInteractionResult[];
}

/** Run `verify --pact <file> --command <cmd> [--arg ...]` (stdio provider). */
export function runVerify(
  pactPath: string,
  command: string,
  args: string[],
  engine = resolveEngine()
): VerifyResult {
  const cliArgs = ["verify", "--pact", pactPath, "--command", command];
  for (const a of args) cliArgs.push("--arg", a);
  const out = spawnSync(engine, cliArgs, { encoding: "utf8" });
  if (out.status !== 0) {
    throw new Error(`engine verify failed (${out.status}): ${out.stderr}`);
  }
  return parseLastJson(out.stdout) as VerifyResult;
}

/** Auth config for an HTTP target (values may use `${ENV}` interpolation). */
export type HttpAuth =
  | { type: "bearer"; token: string }
  | { type: "apiKey"; header: string; value: string }
  | { type: "headers"; headers: Record<string, string> };

/** Run `verify --pact <file> --url <url> [--auth <json>]` (HTTP provider). */
export function runVerifyHttp(
  pactPath: string,
  url: string,
  auth?: HttpAuth,
  engine = resolveEngine()
): VerifyResult {
  const cliArgs = ["verify", "--pact", pactPath, "--url", url];
  if (auth) cliArgs.push("--auth", JSON.stringify(auth));
  const out = spawnSync(engine, cliArgs, { encoding: "utf8" });
  if (out.status !== 0) {
    throw new Error(`engine verify (http) failed (${out.status}): ${out.stderr}`);
  }
  return parseLastJson(out.stdout) as VerifyResult;
}

/** The engine prints JSON on stdout and logs on stderr; take the last JSON line. */
export function parseLastJson(stdout: string): unknown {
  const line = stdout
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.startsWith("{"))
    .pop();
  if (!line) throw new Error(`no JSON in engine output: ${stdout}`);
  return JSON.parse(line);
}
