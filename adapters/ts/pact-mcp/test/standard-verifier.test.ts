// ADR 0008 / plan §8 — THE core promise: an MCP pact authored by this adapter
// verifies through the STANDARD pact-js Verifier (pact-core FFI verifier +
// plugin driver + our installed plugin), no bespoke runner.
//
// The vitest globalSetup (test/global-setup.ts) installs the current engine
// build at ~/.pact/plugins/mcp-<v>/ so the driver loads THIS build.

import { describe, it, expect } from "vitest";
import { Verifier } from "@pact-foundation/pact";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { spawn, ChildProcess } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { McpPact, like } from "../src";

const repoRoot = join(__dirname, "..", "..", "..", "..");
const fixtures = join(repoRoot, "examples", "fixtures");

function spawnHttpFixture(): Promise<{ proc: ChildProcess; port: number }> {
  return new Promise((resolve, reject) => {
    const proc = spawn("node", [join(fixtures, "weather-http-server.mjs")], {
      stdio: ["ignore", "pipe", "ignore"],
    });
    const t = setTimeout(() => reject(new Error("timed out waiting for fixture port")), 10000);
    let buf = "";
    proc.stdout!.on("data", (d: Buffer) => {
      buf += d.toString();
      const line = buf.split("\n").find((l) => l.includes("port"));
      if (line) {
        clearTimeout(t);
        resolve({ proc, port: JSON.parse(line).port as number });
      }
    });
    proc.on("error", reject);
  });
}

/** Author + emit a pact with the adapter (running the mock loop for realism). */
async function emitPact(dir: string, mockTransport: "stdio" | "http"): Promise<string> {
  await new McpPact({ consumer: "weather-agent", provider: "weather-mcp", dir, mockTransport })
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

describe("standard pact-js Verifier E2E (ADR 0008)", () => {
  it("verifies an mcp-http pact against the real HTTP fixture server", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-sv-"));
    const pactPath = await emitPact(dir, "http");
    const { proc, port } = await spawnHttpFixture();
    try {
      const output = await new Verifier({
        provider: "weather-mcp",
        providerBaseUrl: `http://127.0.0.1:${port}`,
        pactUrls: [pactPath],
        transports: [{ protocol: "mcp-http", port }],
        logLevel: "error",
      }).verifyProvider();
      // verifyProvider rejects on failure; the resolved output is a JSON
      // summary — double-check it recorded no errors.
      expect(JSON.parse(output).errors).toEqual([]);
    } finally {
      proc.kill();
    }
  }, 120_000);

  it("verifies an mcp-stdio pact by spawning the fixture server from env config", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-sv-"));
    const pactPath = await emitPact(dir, "stdio");
    process.env.PACT_MCP_SERVER_COMMAND = "node";
    process.env.PACT_MCP_SERVER_ARGS = join(fixtures, "weather-server.mjs");
    try {
      const output = await new Verifier({
        provider: "weather-mcp",
        providerBaseUrl: "http://127.0.0.1:65500", // unused for plugin transports
        pactUrls: [pactPath],
        logLevel: "error",
      }).verifyProvider();
      // verifyProvider rejects on failure; the resolved output is a JSON
      // summary — double-check it recorded no errors.
      expect(JSON.parse(output).errors).toEqual([]);
    } finally {
      delete process.env.PACT_MCP_SERVER_COMMAND;
      delete process.env.PACT_MCP_SERVER_ARGS;
    }
  }, 120_000);

  it("fails verification when the provider returns a tool error the pact does not expect", async () => {
    const dir = mkdtempSync(join(tmpdir(), "pact-sv-"));
    // Atlantis is unknown to the fixture -> isError: true from the provider,
    // but the pact expects a normal result => mismatch => verification fails.
    await new McpPact({ consumer: "weather-agent", provider: "weather-mcp", dir })
      .whenClientCallsTool("get_weather", { city: "Atlantis" })
      .willRespondWith({ content: [{ type: "text", text: like("Sunny, 22C") }], isError: false })
      .executeTest(async ({ transport }) => {
        const client = new Client({ name: "weather-agent", version: "1.0.0" });
        await client.connect(transport);
        await client.callTool({ name: "get_weather", arguments: { city: "Atlantis" } });
        await client.close();
      });
    const pactPath = join(dir, "weather-agent-weather-mcp.json");

    process.env.PACT_MCP_SERVER_COMMAND = "node";
    process.env.PACT_MCP_SERVER_ARGS = join(fixtures, "weather-server.mjs");
    try {
      await expect(
        new Verifier({
          provider: "weather-mcp",
          providerBaseUrl: "http://127.0.0.1:65500",
          pactUrls: [pactPath],
          logLevel: "error",
        }).verifyProvider()
      ).rejects.toThrow();
    } finally {
      delete process.env.PACT_MCP_SERVER_COMMAND;
      delete process.env.PACT_MCP_SERVER_ARGS;
    }
  }, 120_000);
});
