# examples/provider-stdio

Phase 1 demo (plan task 1.9): end-to-end provider verification over stdio
against a **real** MCP server.

- `pacts/weather-agent-weather-mcp.json` — a Pact V4-shaped file with three
  `mcp` plugin interactions (`tools/call` against a `get_weather` tool):
  exact-text pass, type-matcher pass, and a tool-level `isError: true` pass.
- `../fixtures/weather-server.mjs` — the real provider: a genuine
  `@modelcontextprotocol/sdk` `StdioServerTransport` server exposing
  `get_weather`.

Verification is exercised as a Rust integration test —
`rust/pact-mcp-plugin/tests/provider_stdio_example.rs` — which spawns the real
fixture server as a subprocess (`node examples/fixtures/weather-server.mjs`)
for each interaction and runs it through the same
`verify_interaction_stdio` / `content::compare_response` path the plugin's
gRPC `VerifyInteraction` RPC uses.

## Run it

```sh
cd rust
cargo test -p pact-mcp-plugin --test provider_stdio_example
```

Requires `node` on `PATH` and `examples/fixtures/node_modules` installed
(`cd examples/fixtures && npm install`).
