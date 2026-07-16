// McpProviderVerifier — verify a pact against a real MCP server.
//
// Verification (spawn server over stdio, initialize, replay each tools/call,
// compare the response) runs entirely in the Rust engine via its `verify` CLI —
// no TS-side matching. The provider's real MCP server is spawned unchanged.

import { runVerify, VerifyResult } from "./engine";

export interface StdioServerTransport {
  type: "stdio";
  /** Command to launch the real provider MCP server (e.g. "node"). */
  command: string;
  /** Arguments (e.g. ["dist/server.js"]). */
  args?: string[];
}

export interface McpProviderVerifierOptions {
  provider: string;
  /** Pact files to verify. */
  pactUrls: string[];
}

export class McpProviderVerifier {
  private serverTransport?: StdioServerTransport;

  constructor(private readonly opts: McpProviderVerifierOptions) {}

  /**
   * Verify against the provider's real MCP server spawned over stdio. This is
   * the "real Server, unchanged" path — the engine spawns it, does the MCP
   * handshake, and replays each interaction.
   */
  withServerTransport(transport: StdioServerTransport): this {
    this.serverTransport = transport;
    return this;
  }

  /** Verify all pacts. Throws with a readable summary on any failure. */
  async verify(): Promise<VerifyResult[]> {
    if (!this.serverTransport) {
      throw new Error(
        "withServerTransport({ type: 'stdio', command, args }) is required. " +
          "In-memory withServer(factory) verification is not implemented yet (see README)."
      );
    }
    const { command, args = [] } = this.serverTransport;

    const results: VerifyResult[] = [];
    const failures: string[] = [];
    for (const pactUrl of this.opts.pactUrls) {
      const result = runVerify(pactUrl, command, args);
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
