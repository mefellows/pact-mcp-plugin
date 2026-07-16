# examples/consumer-stdio-mock — stdio mock mode (Task 1.8)

Demonstrates the consumer side: a **real** `@modelcontextprotocol/sdk` client
spawns the plugin's `mock` stdio mode as its MCP server (synthesized from a pact
file), does the `initialize` handshake, lists tools, and calls a tool.

- `pacts/weather-agent-weather-mcp.json` — a pact with one `get_weather`
  `tools/call` interaction (expects `city: "Melbourne"` → `"Sunny, 22C"`).
- `client.mjs` — the real MCP SDK client.

The plugin's mock CLI:

```sh
pact-mcp-plugin mock --pact <pact.json> [--results <results.json>]
```

speaks MCP over its own stdio, answers `initialize`/`tools/list` (tools
synthesized from the configured interactions), matches incoming `tools/call`s by
name + arguments (reusing the engine matcher), returns the configured response
on a match, and records a mismatch to the results file on a miss.

## Run (as a cargo integration test)

```sh
cd examples/consumer-stdio-mock && npm install   # once
cd ../../rust && cargo test -p pact-mcp-plugin --test consumer_stdio_mock
```

Covers a matching call (returns the configured response) and an unexpected call
(returns a protocol error and records a `$.arguments.city` mismatch).

## gRPC handoff (§7.2)

`StartMockServer` persists the pact and returns a spawnable
`{command, args, env}` handoff (JSON, in the `MockServerDetails.address` field —
stdio mocks have no listening port), pointing at this same `mock` CLI. The
spawned mock writes results to a file that `GetMockServerResults` /
`ShutdownMockServer` read back.
