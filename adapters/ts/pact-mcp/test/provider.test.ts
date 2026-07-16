import { describe, it, expect } from "vitest";
import { join } from "node:path";
import { McpProviderVerifier } from "../src/index";

const repoRoot = join(__dirname, "..", "..", "..", "..");

describe("McpProviderVerifier", () => {
  it("verifies a real pact against the REAL fixture MCP server over stdio", async () => {
    // The pact was authored by a real pact-js consumer test (committed evidence);
    // the provider is the real @modelcontextprotocol/sdk weather server.
    const pact = join(repoRoot, "examples/ts-roundtrip/pacts-committed/weather-agent-weather-mcp.json");
    const server = join(repoRoot, "examples/fixtures/weather-server.mjs");

    const results = await new McpProviderVerifier({
      provider: "weather-mcp",
      pactUrls: [pact],
    })
      .withServerTransport({ type: "stdio", command: "node", args: [server] })
      .verify();

    expect(results[0].success).toBe(true);
    expect(results[0].interactions[0].success).toBe(true);
  }, 30000);

  it("throws with a readable summary when the provider does not satisfy the pact", async () => {
    // Point at a pact expecting an exact response the fixture server won't return.
    const pact = join(__dirname, "fixtures", "wrong-expectation.json");
    const server = join(repoRoot, "examples/fixtures/weather-server.mjs");

    const run = new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pact] })
      .withServerTransport({ type: "stdio", command: "node", args: [server] })
      .verify();

    await expect(run).rejects.toThrow(/verification failed/i);
  }, 30000);
});
