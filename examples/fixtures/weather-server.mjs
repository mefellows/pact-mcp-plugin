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

server.registerTool(
  "get_weather",
  {
    title: "Get Weather",
    description: "Get the current weather for a city",
    inputSchema: { city: z.string() },
  },
  async ({ city }) => {
    const text = WEATHER[city];
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

const transport = new StdioServerTransport();
await server.connect(transport);
