# ADR 0005: Pinned versions + architecture confirmation (Phase 0/1)

## Status
Accepted

## Pinned versions (this run)

| Component | Version / commit | Source |
|---|---|---|
| `io.pact.plugin.PactPlugin` proto (V1) | pact-foundation/pact-plugins @ `37318cdeaf4748b7babee9af6e5801d44dfd2c8d`, `proto/plugin.proto` | ADR 0001 |
| `rmcp` | `2.2.0` | ADR 0003 |
| `tonic` / `tonic-build` | `0.12` | this crate's `Cargo.toml` |
| `prost` / `prost-types` | `0.13` | this crate's `Cargo.toml` |
| `protoc-bin-vendored` | `3.x` | ADR 0002 |
| MCP protocol version | `2025-06-18` (rmcp 2.2.0's negotiated default — not independently re-verified against the live MCP spec text in this task) | `mcp::handshake::SUPPORTED_PROTOCOL_VERSION` |

Note: `pact-protobuf-plugin` (the reference implementation studied for the
bootstrap pattern) is hosted at `pactflow/pact-protobuf-plugin`, not
`pact-foundation/pact-protobuf-plugin` as the plan doc assumed — corrected
during research (see the Phase 0 research notes / ADR 0001).

## Architecture confirmation

The implementation follows docs/plans/pact-mcp-plugin-implementation-plan.md
§1/§3 as written: a single Rust binary (`rust/pact-mcp-plugin`) implementing
the `PactPlugin` gRPC service, with:
- `mcp/` — the persisted interaction model + JSON-RPC envelope synthesis
  (docs/spec/interaction-schema.md).
- `content/` — MCP-aware matching (docs/spec/matching-semantics.md), consumed
  both by the conformance test harness and by `CompareContents`/`verify.rs`.
- `transport/stdio.rs` — the stdio wire transport (`rmcp`-backed).
- `verify.rs` — provider verification, reusing `content::compare_response`.
- `server.rs` / `main.rs` — the gRPC dispatch + startup handshake.

No deviation from the planned module layout. `config.rs` was added (not
explicitly named in §3's file tree) to hold the `ConfigureInteraction` <->
proto glue separately from the gRPC dispatch itself.

## stdio mock mode (task 1.8) — IMPLEMENTED (follow-up run)
- `pact-mcp-plugin mock --pact <file> [--results <file>]` runs a real MCP server
  over stdio (`src/mock.rs`, rmcp `ServerHandler`), synthesized from a pact.
- `StartMockServer` / `ShutdownMockServer` / `GetMockServerResults` are wired
  (`src/server.rs`). Because a stdio mock is spawned by the client (no listening
  socket), `StartMockServer` returns the spawnable `{command, args, env}`
  handoff as JSON in `MockServerDetails.address` (the proto has no first-class
  field for a spawn handoff — this is the pragmatic encoding, mirroring the
  plan's §7.2 "helper returns `{command,args,env}`"). `port` is 0. The spawned
  mock writes results to a file that Get/Shutdown read back.
- Verified end-to-end by a real `@modelcontextprotocol/sdk` client
  (`examples/consumer-stdio-mock`, `tests/consumer_stdio_mock.rs`).

## Deferred/stubbed
- Streamable HTTP + auth (Phase 2) — not started; Phase 1 was prioritized.
- `ConfigureInteraction`'s input contract + two-part response shape are now
  CONFIRMED against pact-protobuf-plugin source and a live pact-js run (ADR
  0004), but the full live FFI round trip is BLOCKED (pact-core never routes
  `application/mcp+json` to our ConfigureInteraction; native trace logs
  unavailable to diagnose). See ADR 0004 for CONFIRMED vs residual risk.
- Request-side matchers are not yet carried into provider verification / the
  mock from a persisted pact (the mock uses exact argument matching); only
  response-side matchers are exercised. Fixtures use literal request args.
