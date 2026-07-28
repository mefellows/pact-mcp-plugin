#!/usr/bin/env node
// Minimal real MCP server used as a fixture for pact-mcp-plugin conformance /
// provider-verification tests. Exposes a single `get_weather` tool over stdio.
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

const server = new McpServer({
  name: "weather-fixture-server",
  version: "1.0.0",
});

const WEATHER = {
  Melbourne: "Sunny, 22C",
  Sydney: "Cloudy, 19C",
};

// Provider states (ADR 0009). Two seams:
//  - PACT_MCP_PROVIDER_STATES: JSON [{name, params:{city, weather}}] set by the
//    engine on spawn — seeds WEATHER at startup.
//  - STATE_FILE: path to a JSON {city: weather} map written by an external
//    state handler (e.g. pact-js stateHandlers); read lazily per call.
if (process.env.PACT_MCP_PROVIDER_STATES) {
  for (const state of JSON.parse(process.env.PACT_MCP_PROVIDER_STATES)) {
    if (state.params?.city && state.params?.weather) {
      WEATHER[state.params.city] = state.params.weather;
    }
  }
}

async function stateFileOverrides() {
  if (!process.env.STATE_FILE) return {};
  try {
    const { readFile } = await import("node:fs/promises");
    return JSON.parse(await readFile(process.env.STATE_FILE, "utf8"));
  } catch {
    return {};
  }
}

server.registerTool(
  "get_weather",
  {
    title: "Get Weather",
    description: "Get the current weather for a city",
    inputSchema: { city: z.string() },
  },
  async ({ city }) => {
    const overrides = await stateFileOverrides();
    const text = overrides[city] ?? WEATHER[city];
    if (!text) {
      return {
        isError: true,
        content: [{ type: "text", text: `Unknown city: ${city}` }],
      };
    }
    return {
      content: [{ type: "text", text }],
      isError: false,
    };
  }
);

// A resource + a prompt so resources/read|list and prompts/get|list can be
// verified against a real server (plan Phase 3.5).
server.registerResource(
  "melbourne-report",
  "weather://melbourne/report",
  { title: "Melbourne weather report", mimeType: "text/plain" },
  async (uri) => ({
    contents: [{ uri: uri.href, mimeType: "text/plain", text: "Sunny all week" }],
  })
);

server.registerPrompt(
  "weather-report",
  {
    title: "Weather report prompt",
    description: "Compose a weather report for a city",
    argsSchema: { city: z.string() },
  },
  async ({ city }) => ({
    description: "Compose a weather report for a city",
    messages: [
      {
        role: "user",
        content: { type: "text", text: `Write a weather report for ${city}` },
      },
    ],
  })
);

const transport = new StdioServerTransport();
await server.connect(transport);
