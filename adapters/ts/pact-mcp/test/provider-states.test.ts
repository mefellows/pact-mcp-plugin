// ADR 0009 — provider states end to end:
//  1. `McpPact.given(...)` persists the standard V4 `providerStates` field.
//  2. The engine passes states to the spawned stdio server via
//     PACT_MCP_PROVIDER_STATES (McpProviderVerifier path).
//  3. `McpProviderVerifier.stateHandlers({...})` run before verification.
//  4. The STANDARD pact-js Verifier fires ordinary stateHandlers for
//     plugin-transport interactions (no plugin involvement).

import { describe, it, expect } from "vitest";
import { Verifier } from "@pact-foundation/pact";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { McpPact, McpProviderVerifier } from "../src";

const repoRoot = join(__dirname, "..", "..", "..", "..");
const fixtureServer = join(repoRoot, "examples", "fixtures", "weather-server.mjs");

/** Emit a pact for Hobart — a city the fixture only knows via provider state. */
async function emitHobartPact(dir: string): Promise<string> {
  await new McpPact({ consumer: "weather-agent", provider: "weather-mcp", dir })
    .given("the Hobart weather is known", { city: "Hobart", weather: "Windy, 12C" })
    .whenClientCallsTool("get_weather", { city: "Hobart" })
    .willRespondWith({ content: [{ type: "text", text: "Windy, 12C" }], isError: false })
    .executeTest(async ({ transport }) => {
      const client = new Client({ name: "weather-agent", version: "1.0.0" });
      await client.connect(transport);
      await client.callTool({ name: "get_weather", arguments: { city: "Hobart" } });
      await client.close();
    });
  return join(dir, "weather-agent-weather-mcp.json");
}

describe("provider states (ADR 0009)", () => {
  it("given() persists the standard providerStates field on the interaction", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-ps-"));
    const pactPath = await emitHobartPact(dir);
    const pact = JSON.parse(readFileSync(pactPath, "utf8"));
    expect(pact.interactions[0].providerStates).toEqual([
      { name: "the Hobart weather is known", params: { city: "Hobart", weather: "Windy, 12C" } },
    ]);
  });

  it("engine verification seeds the spawned server from the interaction's states", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-ps-"));
    const pactPath = await emitHobartPact(dir);
    // No state handler: PACT_MCP_PROVIDER_STATES seeding alone must make
    // Hobart known to the fixture.
    await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pactPath] })
      .withServerTransport({ type: "stdio", command: "node", args: [fixtureServer] })
      .verify();
  });

  it("stateHandlers run before verification (McpProviderVerifier path)", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-ps-"));
    const pactPath = await emitHobartPact(dir);
    const calls: unknown[] = [];
    await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pactPath] })
      .withServerTransport({ type: "stdio", command: "node", args: [fixtureServer] })
      .stateHandlers({
        "the Hobart weather is known": async (params) => {
          calls.push(params);
        },
      })
      .verify();
    expect(calls).toEqual([{ city: "Hobart", weather: "Windy, 12C" }]);
  });

  it("standard pact-js Verifier fires stateHandlers for plugin-transport interactions", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-ps-"));
    const pactPath = await emitHobartPact(dir);

    // The handler seeds a state file the fixture reads lazily per call; the
    // fixture inherits STATE_FILE through verifier -> plugin -> child env.
    const stateFile = join(mkdtempSync(join(tmpdir(), "pact-ps-state-")), "state.json");
    process.env.STATE_FILE = stateFile;
    process.env.PACT_MCP_SERVER_COMMAND = "node";
    process.env.PACT_MCP_SERVER_ARGS = fixtureServer;
    // Strip the engine-side seeding so ONLY the state handler can make this
    // pass: hide the params from the engine by rewriting the pact without them.
    const pact = JSON.parse(readFileSync(pactPath, "utf8"));
    pact.interactions[0].providerStates = [{ name: "the Hobart weather is known" }];
    writeFileSync(pactPath, JSON.stringify(pact));

    try {
      const output = await new Verifier({
        provider: "weather-mcp",
        providerBaseUrl: "http://127.0.0.1:65500",
        pactUrls: [pactPath],
        logLevel: "error",
        stateHandlers: {
          "the Hobart weather is known": async () => {
            writeFileSync(stateFile, JSON.stringify({ Hobart: "Windy, 12C" }));
            return {};
          },
        },
      }).verifyProvider();
      expect(JSON.parse(output).errors).toEqual([]);
    } finally {
      delete process.env.STATE_FILE;
      delete process.env.PACT_MCP_SERVER_COMMAND;
      delete process.env.PACT_MCP_SERVER_ARGS;
    }
  }, 120_000);
});
