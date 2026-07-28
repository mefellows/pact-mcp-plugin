// McpPact — ergonomic consumer DX.
//
// Design (Option B — see adapters/ts/pact-mcp/README.md and ADR 0006):
// pact-js's synchronous-message plugin flow does NOT expose a live mock the
// consumer's real Client connects to (`withPluginContents().executeTest()` just
// returns the configured contents and writes the pact). So we:
//   1. author + emit a REAL pact via pact-js (which invokes the Rust engine's
//      ConfigureInteraction), then
//   2. spawn the Rust engine's stdio MOCK (reading that pact) and hand the user's
//      real @modelcontextprotocol/sdk Client a transport connected to it.
// All matching stays in the Rust engine (the mock reuses the engine matcher).

import { PactV4 } from "@pact-foundation/pact";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import type { Transport } from "@modelcontextprotocol/sdk/shared/transport.js";
import { spawn, ChildProcess } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, existsSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { resolveEngine } from "./engine";
import { buildDsl } from "./matchers";

export interface McpPactOptions {
  consumer: string;
  provider: string;
  /** Directory to write pacts to (default: ./pacts). */
  dir?: string;
  /** Plugin version installed at ~/.pact/plugins/mcp-<version> (default "0.1.0"). */
  pluginVersion?: string;
  /**
   * Transport the real Client uses to reach the Pact mock:
   *  - "stdio" (default): the engine mock over a stdio subprocess pipe.
   *  - "http": a loopback Streamable HTTP mock (real HTTP, ephemeral port).
   * Matching is identical (Rust engine) either way.
   */
  mockTransport?: "stdio" | "http";
}

/** The value the user's `executeTest` callback receives. */
export interface McpTestScope {
  /** A transport connected to the Pact MCP mock; pass to `client.connect(...)`. */
  transport: Transport;
}

interface PendingInteraction {
  description: string;
  operation: "tools/call" | "tools/list";
  request: unknown;
  response?: unknown;
  providerStates: { name: string; params?: Record<string, unknown> }[];
}

export class McpPact {
  private interactions: PendingInteraction[] = [];
  private pendingStates: { name: string; params?: Record<string, unknown> }[] = [];

  constructor(private readonly opts: McpPactOptions) {}

  /**
   * Declare a provider state (standard V4 `providerStates`, ADR 0009) for the
   * NEXT declared interaction. At verification time the state is applied via
   * the verifier's `stateHandlers` or, for engine-spawned stdio servers,
   * `PACT_MCP_PROVIDER_STATES` env.
   */
  given(state: string, params?: Record<string, unknown>): this {
    this.pendingStates.push(params ? { name: state, params } : { name: state });
    return this;
  }

  /** The consumer's real Client will call `tools/call` for `name` with `args`. */
  whenClientCallsTool(name: string, args: unknown = {}): this {
    const nth = this.interactions.filter((i) => i.operation === "tools/call").length;
    this.interactions.push({
      description: `a tools/call to ${name}${nth > 0 ? ` (${nth + 1})` : ""}`,
      operation: "tools/call",
      request: { name, arguments: args },
      providerStates: this.pendingStates,
    });
    this.pendingStates = [];
    return this;
  }

  /** The expected `tools/call` result (may contain matchers from ./matchers). */
  willRespondWith(response: unknown): this {
    const latest = this.interactions[this.interactions.length - 1];
    if (!latest || latest.operation !== "tools/call" || latest.response !== undefined) {
      throw new Error("willRespondWith(...) must follow whenClientCallsTool(...)");
    }
    latest.response = response;
    return this;
  }

  /**
   * The consumer relies on these tools appearing in `tools/list` (subset,
   * consumer-driven — the server may expose more; matching-semantics §3).
   */
  expectsToolsList(tools: { name: string; inputSchema?: unknown }[]): this {
    this.interactions.push({
      description: "a tools/list",
      operation: "tools/list",
      request: {},
      response: { tools },
      providerStates: this.pendingStates,
    });
    this.pendingStates = [];
    return this;
  }

  /**
   * Author + emit the pact, then run `test` with a transport connected to the
   * engine mock. Throws if any interaction was not matched.
   */
  async executeTest<T>(test: (scope: McpTestScope) => Promise<T>): Promise<T> {
    if (this.interactions.length === 0) {
      throw new Error("declare at least one interaction (whenClientCallsTool / expectsToolsList) before executeTest");
    }
    const missing = this.interactions.find((i) => i.response === undefined);
    if (missing) {
      throw new Error(`interaction "${missing.description}" has no willRespondWith(...)`);
    }

    const dir = this.opts.dir ?? join(process.cwd(), "pacts");
    const version = this.opts.pluginVersion ?? "0.1.0";
    const serverHint = (this.opts.mockTransport ?? "stdio") === "http" ? "http" : "stdio";

    // 1. Author + emit the real pact via pact-js (invokes Rust
    //    ConfigureInteraction). One executeTest per interaction; pact-core
    //    merges them into the same pact file.
    const pact = new PactV4({ consumer: this.opts.consumer, provider: this.opts.provider, dir });
    for (const pending of this.interactions) {
      const mcpContents = {
        "pact:content-type": "application/mcp+json",
        mcp: {
          operation: pending.operation,
          request: buildDsl(pending.request),
          response: buildDsl(pending.response),
          server: { transport: serverHint },
        },
      };
      let interaction = pact.addSynchronousInteraction(pending.description);
      for (const state of pending.providerStates) {
        interaction = interaction.given(state.name, state.params as never);
      }
      await interaction
        .usingPlugin({ plugin: "mcp", version })
        .withPluginContents(JSON.stringify(mcpContents), "application/mcp+json")
        .executeTest(async () => {
          // no-op: this call authors + writes the pact file.
        });
    }

    const pactPath = join(dir, `${this.opts.consumer}-${this.opts.provider}.json`);
    if (!existsSync(pactPath)) {
      throw new Error(`expected pact-js to write a pact at ${pactPath}`);
    }
    stampPact(pactPath, (this.opts.mockTransport ?? "stdio") === "http" ? "mcp-http" : "mcp-stdio");

    // 2. Stand up the engine mock reading that pact; hand the user a transport.
    const engine = resolveEngine();
    const resultsPath = join(mkdtempSync(join(tmpdir(), "pact-mcp-")), "results.json");

    if ((this.opts.mockTransport ?? "stdio") === "http") {
      return this.runHttpMock(engine, pactPath, resultsPath, test);
    }

    const transport = new StdioClientTransport({
      command: engine,
      args: ["mock", "--pact", pactPath, "--results", resultsPath],
    });

    let value: T;
    try {
      value = await test({ transport });
    } finally {
      await transport.close().catch(() => undefined);
    }

    // 3. Assert the mock recorded no errors/mismatches.
    assertMockClean(resultsPath);
    return value;
  }

  /** HTTP mock variant: spawn `mock --http`, connect a real HTTP client. */
  private async runHttpMock<T>(
    engine: string,
    pactPath: string,
    resultsPath: string,
    test: (scope: McpTestScope) => Promise<T>
  ): Promise<T> {
    const proc: ChildProcess = spawn(
      engine,
      ["mock", "--pact", pactPath, "--results", resultsPath, "--http"],
      { stdio: ["pipe", "pipe", "inherit"] }
    );

    const url = await new Promise<string>((resolve, reject) => {
      let buf = "";
      const t = setTimeout(() => reject(new Error("timed out waiting for http mock url")), 10000);
      proc.stdout!.on("data", (d: Buffer) => {
        buf += d.toString();
        const line = buf.split("\n").find((l) => l.includes("url"));
        if (line) {
          clearTimeout(t);
          resolve(JSON.parse(line).url);
        }
      });
      proc.on("error", reject);
    });

    const transport = new StreamableHTTPClientTransport(new URL(url));
    let value: T;
    try {
      value = await test({ transport });
    } finally {
      await transport.close().catch(() => undefined);
      proc.stdin!.end(); // signal EOF -> mock shuts down + flushes results
      await new Promise<void>((r) => proc.on("exit", () => r()));
    }
    assertMockClean(resultsPath);
    return value;
  }
}

/**
 * Post-process the pact pact-js wrote (ADR 0008): the standard verifier only
 * routes an interaction to a plugin transport when the interaction carries a
 * top-level `transport` (our catalogue key), and it addresses interactions by
 * `unique_key()` — an opaque hash unless an explicit `key` is present. pact-js's
 * non-transport sync-message flow can set neither, so we stamp both here.
 */
function stampPact(pactPath: string, transport: "mcp-stdio" | "mcp-http"): void {
  const pact = JSON.parse(readFileSync(pactPath, "utf8")) as {
    interactions?: { description?: string; transport?: string; key?: string; pluginConfiguration?: unknown }[];
  };
  for (const interaction of pact.interactions ?? []) {
    if (!interaction.pluginConfiguration) continue;
    interaction.transport = transport;
    interaction.key ??= createHash("sha256")
      .update(interaction.description ?? JSON.stringify(interaction))
      .digest("hex")
      .slice(0, 16);
  }
  writeFileSync(pactPath, JSON.stringify(pact, null, 2));
}

function assertMockClean(resultsPath: string): void {
  if (!existsSync(resultsPath)) return; // no requests recorded
  const results = JSON.parse(readFileSync(resultsPath, "utf8")) as {
    path: string;
    error?: string;
    mismatches?: string[];
  }[];
  const failures = results.filter((r) => r.error || (r.mismatches && r.mismatches.length));
  if (failures.length > 0) {
    throw new Error(
      "MCP mock recorded mismatches:\n" +
        failures.map((f) => `  ${f.path}: ${f.error ?? ""} ${(f.mismatches ?? []).join(", ")}`).join("\n")
    );
  }
}
