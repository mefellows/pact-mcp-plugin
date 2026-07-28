// McpProviderVerifier — verify a pact against a real MCP server.
//
// Verification (spawn server over stdio, initialize, replay each tools/call,
// compare the response) runs entirely in the Rust engine via its `verify` CLI —
// no TS-side matching. The provider's real MCP server is spawned unchanged.

import { readFileSync } from "node:fs";
import { runVerify, runVerifyHttp, HttpAuth, VerifyResult } from "./engine";

/** State setup callback; receives the state's params (ADR 0009). */
export type StateHandler = (params: Record<string, unknown>) => Promise<unknown> | unknown;

export interface StdioServerTransport {
  type: "stdio";
  /** Command to launch the real provider MCP server (e.g. "node"). */
  command: string;
  /** Arguments (e.g. ["dist/server.js"]). */
  args?: string[];
}

export interface HttpServerTransport {
  type: "http";
  /** URL of the running MCP server's Streamable HTTP endpoint. */
  url: string;
  /** Optional auth; secrets can use `${ENV}` and are never written to a pact. */
  auth?: HttpAuth;
}

export type ServerTransport = StdioServerTransport | HttpServerTransport;

export interface McpProviderVerifierOptions {
  provider: string;
  /** Pact files to verify. */
  pactUrls: string[];
}

export class McpProviderVerifier {
  private serverTransport?: ServerTransport;
  private handlers: Record<string, StateHandler> = {};

  constructor(private readonly opts: McpProviderVerifierOptions) {}

  /**
   * Register provider-state handlers, invoked in-process before each pact is
   * verified (ADR 0009). Engine-spawned stdio servers ALSO receive the states
   * via PACT_MCP_PROVIDER_STATES env — use handlers for state that lives
   * behind seams your test controls (databases, files, ...).
   */
  stateHandlers(handlers: Record<string, StateHandler>): this {
    this.handlers = { ...this.handlers, ...handlers };
    return this;
  }

  /** Run registered handlers for every state of every interaction in a pact. */
  private async applyStates(pactUrl: string): Promise<void> {
    if (Object.keys(this.handlers).length === 0) return;
    const pact = JSON.parse(readFileSync(pactUrl, "utf8")) as {
      interactions?: { providerStates?: { name: string; params?: Record<string, unknown> }[] }[];
    };
    for (const interaction of pact.interactions ?? []) {
      for (const state of interaction.providerStates ?? []) {
        const handler = this.handlers[state.name];
        if (handler) await handler(state.params ?? {});
      }
    }
  }

  /**
   * Verify against the provider's real MCP server. Two forms:
   *  - `{ type: 'stdio', command, args }` — the engine spawns it and replays.
   *  - `{ type: 'http', url, auth }` — the engine connects to a running server
   *    (deployed or loopback), injecting auth on every request.
   * The provider's real server is used unchanged.
   */
  withServerTransport(transport: ServerTransport): this {
    this.serverTransport = transport;
    return this;
  }

  /** Verify all pacts. Throws with a readable summary on any failure. */
  async verify(): Promise<VerifyResult[]> {
    if (!this.serverTransport) {
      throw new Error(
        "withServerTransport({ type: 'stdio'|'http', ... }) is required. " +
          "In-memory withServer(factory) verification is not implemented yet (see README)."
      );
    }
    const transport = this.serverTransport;

    const results: VerifyResult[] = [];
    const failures: string[] = [];
    for (const pactUrl of this.opts.pactUrls) {
      await this.applyStates(pactUrl);
      const result =
        transport.type === "http"
          ? runVerifyHttp(pactUrl, transport.url, transport.auth)
          : runVerify(pactUrl, transport.command, transport.args ?? []);
      results.push(result);
      if (!result.success) {
        for (const i of result.interactions.filter((x) => !x.success)) {
          failures.push(
            `  [${pactUrl}] ${i.description}: ${i.error ?? ""} ` +
              (i.mismatches ?? []).map((m) => `${m.path} ${m.message}`).join("; ")
          );
        }
      }
    }

    if (failures.length > 0) {
      throw new Error(`Provider verification failed:\n${failures.join("\n")}`);
    }
    return results;
  }
}
