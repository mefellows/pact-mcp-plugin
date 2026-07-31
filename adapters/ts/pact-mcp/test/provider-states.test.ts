// ADR 0009 — provider states, verified through the standard pact-js Verifier
// (McpProviderVerifier is now a thin wrapper over it):
//  1. `McpPact.given(...)` persists the standard V4 `providerStates` field.
//  2. The plugin seeds the spawned stdio server from the interaction's
//     providerStates automatically (PACT_MCP_PROVIDER_STATES) — no handler needed.
//  3. `stateHandlers` run before verification and can set up external state the
//     server reads (the realistic route for DBs/files).

import { describe, it, expect } from "vitest";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { McpPact, McpProviderVerifier } from "../src";

const repoRoot = join(__dirname, "..", "..", "..", "..");
const fixtureServer = join(repoRoot, "examples", "fixtures", "weather-server.mjs");

/** Hobart is unknown to the fixture unless a provider state supplies it. */
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

  it("the plugin seeds the spawned server from the interaction's states (no handler)", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-ps-"));
    const pactPath = await emitHobartPact(dir);
    // No stateHandlers: PACT_MCP_PROVIDER_STATES seeding by the plugin alone
    // must make Hobart known to the fixture.
    const output = await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pactPath], logLevel: "error" })
      .withServerTransport({ type: "stdio", command: "node", args: [fixtureServer] })
      .verify();
    expect(JSON.parse(output).errors).toEqual([]);
  }, 120000);

  it("stateHandlers run before verification and set up external state the server reads", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-ps-"));
    const pactPath = await emitHobartPact(dir);
    // Strip the params so plugin env-seeding can't cover Hobart; ONLY the state
    // handler writing a file the fixture reads can make verification pass.
    const pact = JSON.parse(readFileSync(pactPath, "utf8"));
    pact.interactions[0].providerStates = [{ name: "the Hobart weather is known" }];
    writeFileSync(pactPath, JSON.stringify(pact));

    const stateFile = join(mkdtempSync(join(tmpdir(), "pact-ps-state-")), "state.json");
    const output = await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pactPath], logLevel: "error" })
      .withServerTransport({ type: "stdio", command: "node", args: [fixtureServer], env: { STATE_FILE: stateFile } })
      .stateHandlers({
        "the Hobart weather is known": async () => {
          writeFileSync(stateFile, JSON.stringify({ Hobart: "Windy, 12C" }));
        },
      })
      .verify();
    expect(JSON.parse(output).errors).toEqual([]);
  }, 120000);
});
