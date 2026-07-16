# ADR 0001: Vendor plugin.proto V1 from pact-plugins

## Status
Accepted

## Context
The Pact plugin framework's gRPC contract (`io.pact.plugin.PactPlugin`) is defined
in `pact-foundation/pact-plugins`. The repository contains two proto files:

- `proto/plugin.proto` — V1, package `io.pact.plugin`.
- `proto/plugin_v2.proto` — V2, package `io.pact.plugin.v2`, documented in-repo as
  "intentionally mirrors V1 while capability-negotiation changes are introduced
  incrementally."

The canonical reference implementation, `pact-protobuf-plugin` (actually hosted at
`pactflow/pact-protobuf-plugin`, not `pact-foundation/pact-protobuf-plugin`), uses V1:
its plugin manifest declares `"pluginInterfaceVersion": 1` and its `main.rs` targets
V1 message/service shapes via the `pact-plugin-driver` crate.

## Decision
Vendor `proto/plugin.proto` (V1) verbatim into
`rust/pact-mcp-plugin/proto/plugin.proto`.

Source: `pact-foundation/pact-plugins` @ commit
`37318cdeaf4748b7babee9af6e5801d44dfd2c8d` (cloned into a scratch dir for this
task; the file is byte-identical between `proto/plugin.proto` and
`drivers/rust/driver/plugin.proto` in that repo).

We do NOT depend on the `pact-plugin-driver` crate (unlike pact-protobuf-plugin);
we compile the proto ourselves with `tonic-build` so we have full control over the
generated code and don't take on that crate's broader dependency surface for an
MVP. This can be revisited later if we want to reuse its mock-server/matching
utilities.

## Consequences
- If pact-plugins ships breaking V1 changes or the ecosystem moves to V2, we must
  re-vendor and update this ADR.
- Our gRPC server only needs `build_server(true)`; we don't need the client stubs.
