// G5 — resources/read|list + prompts/get|list vertical slice: DSL authors
// them, the engine mock serves them to a REAL client, and the engine verifies
// them against the REAL fixture server.

import { describe, it, expect } from "vitest";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { McpPact, McpProviderVerifier, like } from "../src";

const repoRoot = join(__dirname, "..", "..", "..", "..");
const fixtureServer = join(repoRoot, "examples", "fixtures", "weather-server.mjs");

describe("resources + prompts (G5)", () => {
  it("authors, mocks, and verifies resources/read|list and prompts/get|list", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-rp-"));

    await new McpPact({ consumer: "weather-agent", provider: "weather-mcp", dir })
      .expectsResourcesList([{ uri: "weather://melbourne/report" }])
      .whenClientReadsResource("weather://melbourne/report")
      .willRespondWith({
        contents: [{ uri: "weather://melbourne/report", text: like("Sunny all week") }],
      })
      .expectsPromptsList([{ name: "weather-report" }])
      .whenClientGetsPrompt("weather-report", { city: "Melbourne" })
      .willRespondWith({
        messages: [
          { role: "user", content: { type: "text", text: like("Write a weather report for Melbourne") } },
        ],
      })
      .executeTest(async ({ transport }) => {
        const client = new Client({ name: "weather-agent", version: "1.0.0" });
        await client.connect(transport);

        const resources = await client.listResources();
        expect(resources.resources.map((r) => r.uri)).toContain("weather://melbourne/report");

        const report = await client.readResource({ uri: "weather://melbourne/report" });
        expect((report.contents[0] as { text: string }).text).toContain("Sunny");

        const prompts = await client.listPrompts();
        expect(prompts.prompts.map((p) => p.name)).toContain("weather-report");

        const prompt = await client.getPrompt({ name: "weather-report", arguments: { city: "Melbourne" } });
        expect((prompt.messages[0].content as { text: string }).text).toContain("Melbourne");

        await client.close();
      });

    // Verify all four interactions against the real fixture server.
    const pactPath = join(dir, "weather-agent-weather-mcp.json");
    await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pactPath] })
      .withServerTransport({ type: "stdio", command: "node", args: [fixtureServer] })
      .verify();
  }, 60_000);
});
