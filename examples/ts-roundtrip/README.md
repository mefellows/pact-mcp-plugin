# examples/ts-roundtrip — live pact-js round-trip probe (Task A)

Attempts the real consumer authoring path: a pact-js V4 plugin DSL consumer test
(`PactV4` + `addSynchronousInteraction` + `usingPlugin({plugin:"mcp"})` +
`withPluginContents(json, "application/mcp+json")`) driving our installed `mcp`
engine, authoring one `tools/call` interaction with an inline
`matching(type, 'Sunny, 22C')` matcher, and writing the pact to `./pacts`.

## Status: WORKING end-to-end (see docs/decisions/0004 — RESOLVED)

The consumer test drives our installed `mcp` plugin and writes a real pact to
`./pacts`. A copy of the emitted pact is committed as evidence at
`./pacts-committed/weather-agent-weather-mcp.json`, and our engine verifies it
against the fixture MCP server in
`rust/pact-mcp-plugin/tests/live_roundtrip_verify.rs`.

Root cause of the earlier "Retrieved an empty message" block: the pact plugin
driver matches a content-matcher's `content-types` value as a **regex** anchored
at both ends, so the `+` in `application/mcp+json` was a quantifier and never
matched literally. Fixed by registering the escaped `application/mcp\+json` in
`catalogue.rs`.

## Run (requires the plugin installed at ~/.pact/plugins/mcp-0.1.0/)

```sh
# build + install the engine as a plugin
cargo build -p pact-mcp-plugin --release --manifest-path rust/Cargo.toml
mkdir -p ~/.pact/plugins/mcp-0.1.0
cp rust/target/release/pact-mcp-plugin ~/.pact/plugins/mcp-0.1.0/
cp pact-plugin.json ~/.pact/plugins/mcp-0.1.0/

cd examples/ts-roundtrip && npm install
node generate-pact.mjs   # writes a pact to ./pacts
```
