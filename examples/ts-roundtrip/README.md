# examples/ts-roundtrip — live pact-js round-trip probe (Task A)

Attempts the real consumer authoring path: a pact-js V4 plugin DSL consumer test
(`PactV4` + `addSynchronousInteraction` + `usingPlugin({plugin:"mcp"})` +
`withPluginContents(json, "application/mcp+json")`) driving our installed `mcp`
engine, authoring one `tools/call` interaction with an inline
`matching(type, 'Sunny, 22C')` matcher, and writing the pact to `./pacts`.

## Status: BLOCKED at the FFI (see docs/decisions/0004)

The plugin loads and its `InitPlugin` is called, but pact-core's native FFI
(`@pact-foundation/pact` 17 / `pact-core` 19.2.0 / native 0.5.4) never routes the
`application/mcp+json` contents to our `ConfigureInteraction`, so pact-js throws
`Retrieved an empty message`. The native plugin-driver trace logs did not
surface in this environment, so the routing failure couldn't be diagnosed.

What WAS confirmed here (and folded back into the engine): a sync-message plugin
must return two `InteractionResponse` parts (request + response), and matchers
are authored inline as DSL strings. The engine's side of the contract is proven
by `rust/pact-mcp-plugin/tests/grpc_bootstrap.rs`
(`configure_interaction_returns_two_part_sync_message...`), which makes the exact
gRPC call pact core would.

## Run (requires the plugin installed at ~/.pact/plugins/mcp-0.1.0/)

```sh
# build + install the engine as a plugin
cargo build -p pact-mcp-plugin --release --manifest-path rust/Cargo.toml
mkdir -p ~/.pact/plugins/mcp-0.1.0
cp rust/target/release/pact-mcp-plugin ~/.pact/plugins/mcp-0.1.0/
cp pact-plugin.json ~/.pact/plugins/mcp-0.1.0/

cd examples/ts-roundtrip && npm install
node generate-pact.mjs   # currently errors at the FFI as described above
```
