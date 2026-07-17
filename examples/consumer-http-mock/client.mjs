// Real @modelcontextprotocol/sdk client that connects to a loopback HTTP MCP
// mock (stood up by the engine's StartMockServer) and calls a tool.
// Usage: node client.mjs <url> <city>
// Prints { tools, call } or { error } as JSON.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";

const [, , url, city] = process.argv;

const transport = new StreamableHTTPClientTransport(new URL(url));
const client = new Client({ name: "weather-agent", version: "1.0.0" });
await client.connect(transport);

const out = {};
const tools = await client.listTools();
out.tools = tools.tools.map((t) => t.name);
try {
  out.call = await client.callTool({ name: "get_weather", arguments: { city } });
} catch (e) {
  out.error = String(e.message ?? e);
}
console.log(JSON.stringify(out));
await client.close();
