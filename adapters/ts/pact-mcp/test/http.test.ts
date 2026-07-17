import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { spawn, ChildProcess } from "node:child_process";
import { join } from "node:path";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { McpPact, McpProviderVerifier, like } from "../src/index";

const repoRoot = join(__dirname, "..", "..", "..", "..");
const httpServer = join(repoRoot, "examples/fixtures/weather-http-server.mjs");
const pact = join(repoRoot, "examples/ts-roundtrip/pacts-committed/weather-agent-weather-mcp.json");

/** Spawn the HTTP fixture server, return its base URL + the process. */
function spawnHttpServer(env: Record<string, string>): Promise<{ url: string; proc: ChildProcess }> {
  return new Promise((resolve, reject) => {
    const proc = spawn("node", [httpServer], { env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "inherit"] });
    let buf = "";
    const t = setTimeout(() => reject(new Error("server did not start")), 10000);
    proc.stdout!.on("data", (d: Buffer) => {
      buf += d.toString();
      const line = buf.split("\n").find((l) => l.includes("port"));
      if (line) {
        clearTimeout(t);
        resolve({ url: `http://127.0.0.1:${JSON.parse(line).port}/`, proc });
      }
    });
  });
}

describe("HTTP provider verification with auth", () => {
  it("verifies against a bearer-protected server (passes with the right token)", async () => {
    process.env.PACT_MCP_HTTP_TOKEN = "the-secret";
    const { url, proc } = await spawnHttpServer({ REQUIRE_BEARER: "the-secret" });
    try {
      const results = await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pact] })
        .withServerTransport({ type: "http", url, auth: { type: "bearer", token: "${PACT_MCP_HTTP_TOKEN}" } })
        .verify();
      expect(results[0].success).toBe(true);
    } finally {
      proc.kill();
    }
  }, 30000);

  it("fails clearly against a bearer-protected server with no auth", async () => {
    const { url, proc } = await spawnHttpServer({ REQUIRE_BEARER: "the-secret" });
    try {
      const run = new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pact] })
        .withServerTransport({ type: "http", url }) // no auth
        .verify();
      await expect(run).rejects.toThrow(/verification failed/i);
    } finally {
      proc.kill();
    }
  }, 30000);
});

describe("HTTP consumer mock", () => {
  it("connects a REAL MCP Client to a loopback HTTP mock and matches", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-mcp-http-"));
    let text = "";
    await new McpPact({ consumer: "weather-agent", provider: "weather-mcp", dir, mockTransport: "http" })
      .whenClientCallsTool("get_weather", { city: "Melbourne" })
      .willRespondWith({ content: [{ type: "text", text: like("Sunny, 22C") }], isError: false })
      .executeTest(async ({ transport }) => {
        const client = new Client({ name: "weather-agent", version: "1.0.0" });
        await client.connect(transport);
        const res: any = await client.callTool({ name: "get_weather", arguments: { city: "Melbourne" } });
        text = res.content[0].text;
        await client.close();
      });
    expect(text).toContain("22C");
  }, 30000);
});
