# ADR 0003: Use rmcp for the MCP client/stdio transport

## Status
Accepted

## Context
The engine needs to (a) spawn an MCP server subprocess, (b) speak newline-delimited
JSON-RPC 2.0 over its stdio, (c) perform the `initialize`/`initialized` handshake,
and (d) call `tools/list` / `tools/call`. The plan allowed falling back to raw
`serde_json` over `tokio::process` if `rmcp`'s API was unclear, unstable, or
fought us.

## Investigation
Probed `rmcp` (official Rust MCP SDK) in a scratch crate:
`cargo add rmcp --features client,transport-child-process` resolved cleanly to
**rmcp 2.2.0**. A minimal client — spawn child process, `initialize`/`initialized`
handshake, `list_tools`, `call_tool` — compiled with zero errors/warnings.

Key API surface used:
- `rmcp::transport::TokioChildProcess::new(tokio::process::Command) -> io::Result<Self>`
  (implements `Transport<RoleClient>`; a `.builder()` variant allows explicit
  `Stdio` control).
- `rmcp::ServiceExt::serve(self, transport)` — `().serve(transport).await?` runs
  the full `initialize` → `initialized` handshake and returns a `RunningService`.
- `Peer<RoleClient>::list_all_tools()` / `list_tools(...)` (auto-paginating
  convenience + raw form).
- `Peer<RoleClient>::call_tool(CallToolRequestParams)` — build params via
  `CallToolRequestParams::new(name).with_arguments(json_object)` (the struct is
  `#[non_exhaustive]`).

## Decision
Adopt `rmcp = "2.2"` with features `client`, `transport-child-process` for the
stdio transport (`src/transport/stdio.rs`) and handshake (`src/mcp/handshake.rs`).
No raw-JSON-RPC fallback needed for Phase 1.

## Consequences
- We depend on rmcp's typed `CallToolResult`/`Tool`/etc. models for the *transport*
  boundary; our own `mcp::model` types (matching the pact interaction schema) are
  still hand-written serde types independent of rmcp, and we convert between them
  at the transport boundary (`src/transport/stdio.rs`). This keeps the persisted
  pact fragment shape (docs/spec/interaction-schema.md) decoupled from rmcp's
  internal representation, which may evolve independently.
- If a future phase needs the Streamable HTTP client transport, re-verify rmcp's
  `transport-streamable-http-client` feature the same way before adopting it.
