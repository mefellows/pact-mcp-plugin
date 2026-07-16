// Task A — LIVE pact-js round trip.
//
// Uses the REAL pact-js V4 plugin DSL (PactV4 + addSynchronousInteraction +
// usingPlugin + withPluginContents) to author one tools/call interaction with
// an inline `matching(type, ...)` matcher, driving our installed `mcp` plugin
// (~/.pact/plugins/mcp-0.1.0) via its gRPC ConfigureInteraction. pact-js then
// writes the pact file to ./pacts. No live mock transport is used — the
// withPluginContents().executeTest(m => ...) path just calls ConfigureInteraction
// and persists the interaction, which is exactly what we need to observe the
// real persisted matchingRules shape.
import { PactV4 } from "@pact-foundation/pact";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

const pact = new PactV4({
  consumer: "weather-agent",
  provider: "weather-mcp",
  dir: join(__dirname, "pacts"),
});

const mcpContents = JSON.stringify({
  "pact:content-type": "application/mcp+json",
  mcp: {
    operation: "tools/call",
    request: { name: "get_weather", arguments: { city: "Melbourne" } },
    response: {
      content: [{ type: "text", text: "matching(type, 'Sunny, 22C')" }],
      isError: false,
    },
    server: { transport: "stdio" },
  },
});

const result = await pact
  .addSynchronousInteraction("a request for the Melbourne weather")
  .usingPlugin({ plugin: "mcp", version: "0.1.0" })
  .withPluginContents(mcpContents, "application/mcp+json")
  .executeTest(async (m) => {
    // The mock message exposes the configured request/response contents.
    console.log("configured request:", JSON.stringify(m.Request));
    console.log("configured response:", JSON.stringify(m.Response));
    return "ok";
  });

console.log("executeTest result:", result);
