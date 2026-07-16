import { describe, it, expect } from "vitest";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { McpPact, like } from "../src/index";

describe("McpPact consumer DX", () => {
  it("connects a REAL @modelcontextprotocol/sdk Client to the Pact mock and matches", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-mcp-consumer-"));

    let toolText = "";
    await new McpPact({ consumer: "weather-agent", provider: "weather-mcp", dir })
      .whenClientCallsTool("get_weather", { city: "Melbourne" })
      .willRespondWith({
        content: [{ type: "text", text: like("Sunny, 22C") }],
        isError: false,
      })
      .executeTest(async ({ transport }) => {
        // A genuinely real MCP client — no stubbing.
        const client = new Client({ name: "weather-agent", version: "1.0.0" });
        await client.connect(transport);

        const tools = await client.listTools();
        expect(tools.tools.map((t) => t.name)).toContain("get_weather");

        const res: any = await client.callTool({ name: "get_weather", arguments: { city: "Melbourne" } });
        toolText = res.content[0].text;

        await client.close();
      });

    // The mock returned the configured response (the `like` matcher accepts it).
    expect(toolText).toContain("22C");
  }, 30000);

  it("fails the test when the real client calls with unexpected arguments", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-mcp-consumer-neg-"));

    const run = new McpPact({ consumer: "weather-agent", provider: "weather-mcp", dir })
      .whenClientCallsTool("get_weather", { city: "Melbourne" })
      .willRespondWith({ content: [{ type: "text", text: like("Sunny, 22C") }], isError: false })
      .executeTest(async ({ transport }) => {
        const client = new Client({ name: "weather-agent", version: "1.0.0" });
        await client.connect(transport);
        // Wrong argument -> the Rust mock records a mismatch and returns a protocol error.
        await client.callTool({ name: "get_weather", arguments: { city: "Atlantis" } }).catch(() => undefined);
        await client.close();
      });

    await expect(run).rejects.toThrow(/mismatch/i);
  }, 30000);
});
