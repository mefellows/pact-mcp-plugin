// ADR 0008 — the emitted pact must carry a top-level `transport` (the plugin
// catalogue key, so the STANDARD verifier routes the interaction to the plugin)
// and a stable `key` (the driver addresses interactions by unique_key(), which
// is an opaque hash unless an explicit key is stamped).

import { describe, it, expect } from "vitest";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { McpPact, like } from "../src";

async function emitPact(dir: string, mockTransport: "stdio" | "http") {
  await new McpPact({ consumer: "stamp-agent", provider: "stamp-mcp", dir, mockTransport })
    .whenClientCallsTool("get_weather", { city: "Melbourne" })
    .willRespondWith({ content: [{ type: "text", text: like("Sunny, 22C") }], isError: false })
    .executeTest(async ({ transport }) => {
      const client = new Client({ name: "stamp-agent", version: "1.0.0" });
      await client.connect(transport);
      await client.callTool({ name: "get_weather", arguments: { city: "Melbourne" } });
      await client.close();
    });
  return JSON.parse(readFileSync(join(dir, "stamp-agent-stamp-mcp.json"), "utf8"));
}

describe("emitted pact stamping (ADR 0008)", () => {
  it("stamps transport mcp-stdio + a stable key for the stdio mock", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-stamp-"));
    const pact = await emitPact(dir, "stdio");
    const interaction = pact.interactions[0];
    expect(interaction.transport).toBe("mcp-stdio");
    expect(typeof interaction.key).toBe("string");
    expect(interaction.key.length).toBeGreaterThan(0);
  });

  it("stamps transport mcp-http (and the http server hint) for the http mock", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-stamp-"));
    const pact = await emitPact(dir, "http");
    const interaction = pact.interactions[0];
    expect(interaction.transport).toBe("mcp-http");
    expect(interaction.pluginConfiguration.mcp.server.transport).toBe("http");
    expect(typeof interaction.key).toBe("string");
  });
});
