// Build + install the CURRENT engine at ~/.pact/plugins/mcp-<version>/ once
// per test run: both the pact plugin driver (standard-verifier tests) and
// resolveEngine() prefer the installed plugin, so a stale install would test
// yesterday's engine. Incremental cargo build — fast when nothing changed.
import { execFileSync } from "node:child_process";
import { join } from "node:path";

export default function setup(): void {
  const repoRoot = join(__dirname, "..", "..", "..", "..");
  execFileSync("bash", [join(repoRoot, "scripts", "install-local.sh")], { stdio: "inherit" });
}
