// G4 — the DSL authors tools/list expectations and MULTIPLE interactions in
// one test; the emitted pact carries them all; the engine mock serves them all;
// and the engine verifies every interaction against the real fixture server.

import { describe, it, expect } from "vitest";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { McpPact, McpProviderVerifier, like } from "../src";

const repoRoot = join(__dirname, "..", "..", "..", "..");
const fixtureServer = join(repoRoot, "examples", "fixtures", "weather-server.mjs");

describe("multi-interaction + tools/list DSL (G4)", () => {
  it("authors tools/list + two tools/call interactions, mocks them, and verifies them", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-multi-"));

    await new McpPact({ consumer: "weather-agent", provider: "weather-mcp", dir })
      .expectsToolsList([{ name: "get_weather" }])
      .whenClientCallsTool("get_weather", { city: "Melbourne" })
      .willRespondWith({ content: [{ type: "text", text: like("Sunny, 22C") }], isError: false })
      .whenClientCallsTool("get_weather", { city: "Sydney" })
      .willRespondWith({ content: [{ type: "text", text: "Cloudy, 19C" }], isError: false })
      .executeTest(async ({ transport }) => {
        const client = new Client({ name: "weather-agent", version: "1.0.0" });
        await client.connect(transport);

        const tools = await client.listTools();
        expect(tools.tools.map((t) => t.name)).toContain("get_weather");

        const melbourne = await client.callTool({ name: "get_weather", arguments: { city: "Melbourne" } });
        expect((melbourne.content as { text: string }[])[0].text).toContain("22C");

        const sydney = await client.callTool({ name: "get_weather", arguments: { city: "Sydney" } });
        expect((sydney.content as { text: string }[])[0].text).toBe("Cloudy, 19C");

        await client.close();
      });

    const pactPath = join(dir, "weather-agent-weather-mcp.json");
    const pact = JSON.parse(readFileSync(pactPath, "utf8"));
    expect(pact.interactions).toHaveLength(3);
    for (const interaction of pact.interactions) {
      expect(interaction.transport).toBe("mcp-stdio");
      expect(typeof interaction.key).toBe("string");
    }

    // The engine verifies ALL interactions (incl. tools/list subset) against
    // the real fixture server.
    await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pactPath] })
      .withServerTransport({ type: "stdio", command: "node", args: [fixtureServer] })
      .verify();
  }, 60_000);

  it("fails the consumer test when a call the pact does not describe arrives", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-multi-"));
    await expect(
      new McpPact({ consumer: "weather-agent", provider: "weather-mcp", dir })
        .whenClientCallsTool("get_weather", { city: "Melbourne" })
        .willRespondWith({ content: [{ type: "text", text: "Sunny, 22C" }], isError: false })
        .executeTest(async ({ transport }) => {
          const client = new Client({ name: "weather-agent", version: "1.0.0" });
          await client.connect(transport);
          await client.callTool({ name: "get_weather", arguments: { city: "Perth" } }).catch(() => undefined);
          await client.close();
        })
    ).rejects.toThrow(/mismatch/i);
  }, 60_000);
});
