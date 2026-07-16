// Real @modelcontextprotocol/sdk client that spawns our plugin's stdio MOCK
// mode as its server, does the MCP handshake, lists tools, and calls a tool.
//
// Usage: node client.mjs <path-to-plugin-binary> <path-to-pact> <results-path> <city>
// Prints the tool-call result JSON (or an {"error": ...} object) to stdout.
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const [, , binary, pactPath, resultsPath, city] = process.argv;

const transport = new StdioClientTransport({
  command: binary,
  args: ["mock", "--pact", pactPath, "--results", resultsPath],
});

const client = new Client({ name: "weather-agent", version: "1.0.0" });
await client.connect(transport);

const tools = await client.listTools();
const out = { tools: tools.tools.map((t) => t.name) };

try {
  const res = await client.callTool({ name: "get_weather", arguments: { city } });
  out.call = res;
} catch (e) {
  out.error = String(e.message ?? e);
}

console.log(JSON.stringify(out));
await client.close();
