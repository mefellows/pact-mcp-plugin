import { describe, it, expect } from "vitest";
import { spawn, ChildProcess } from "node:child_process";
import { join } from "node:path";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { McpPact, McpProviderVerifier, like } from "../src/index";

const repoRoot = join(__dirname, "..", "..", "..", "..");
const httpServer = join(repoRoot, "examples/fixtures/weather-http-server.mjs");

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

/** Emit an mcp-http-stamped pact so the standard verifier routes it over HTTP. */
async function emitHttpPact(dir: string): Promise<string> {
  await new McpPact({ consumer: "weather-agent", provider: "weather-mcp", dir, mockTransport: "http" })
    .whenClientCallsTool("get_weather", { city: "Melbourne" })
    .willRespondWith({ content: [{ type: "text", text: like("Sunny, 22C") }], isError: false })
    .executeTest(async ({ transport }) => {
      const client = new Client({ name: "weather-agent", version: "1.0.0" });
      await client.connect(transport);
      await client.callTool({ name: "get_weather", arguments: { city: "Melbourne" } });
      await client.close();
    });
  return join(dir, "weather-agent-weather-mcp.json");
}

describe("HTTP provider verification with auth", () => {
  it("verifies against a bearer-protected server (passes with the right token)", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-http-prov-"));
    const pact = await emitHttpPact(dir);
    process.env.PACT_MCP_HTTP_TOKEN = "the-secret";
    const { url, proc } = await spawnHttpServer({ REQUIRE_BEARER: "the-secret" });
    try {
      const output = await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pact], logLevel: "error" })
        .withServerTransport({ type: "http", url, auth: { type: "bearer", token: "${PACT_MCP_HTTP_TOKEN}" } })
        .verify();
      expect(JSON.parse(output).errors).toEqual([]);
    } finally {
      proc.kill();
      delete process.env.PACT_MCP_HTTP_TOKEN;
    }
  }, 120000);

  it("fails against a bearer-protected server with no auth", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-http-prov-"));
    const pact = await emitHttpPact(dir);
    const { url, proc } = await spawnHttpServer({ REQUIRE_BEARER: "the-secret" });
    try {
      const run = new McpProviderVerifier({ provider: "weather-mcp", pactUrls: [pact], logLevel: "error" })
        .withServerTransport({ type: "http", url }) // no auth
        .verify();
      await expect(run).rejects.toThrow();
    } finally {
      proc.kill();
    }
  }, 120000);
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
