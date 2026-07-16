# ADR 0006: TS adapter consumer flow — Option B (engine stdio mock), not a pact-js live transport

## Status
Accepted

## Context
Phase 3 delivers the TypeScript adapter (`adapters/ts/pact-mcp`). The §10 DX ideal
is "real Client ↔ real mock, zero wire": the user's real
`@modelcontextprotocol/sdk` `Client` connects to a Pact-synthesized mock and calls
tools, with matching in the Rust engine. The open question (the "crux") was
whether pact-js's synchronous-message plugin flow exposes a **live** mock
transport the user's real Client can connect to (Option A), or only validates at
pact-build time (Option B).

## Investigation (empirical, against @pact-foundation/pact 17 / pact-core 19.2.0)
Read `node_modules/@pact-foundation/pact/src/v4/message/index.js`:
- `SynchronousMessageWithPluginContents.executeTest(cb)` → `executeNonTransportTest`,
  which calls `interaction.getRequestContents()` / `getResponseContents()` and
  invokes `cb({ Request, Response })` — i.e. it hands back the **configured
  contents** and then writes the pact. **No live mock transport is exposed.**
- The only live-transport path is `startTransport(transport, address, config)` →
  `pactffiCreateMockServerForTransport(address, transport, config)`, which needs a
  **listening-socket** mock (a `{ port, address }`), i.e. an HTTP-style transport.
  Our plugin's stdio mock is spawned by the client and has no port, so this path
  is not usable for a stdio/in-memory consumer mock. (HTTP transport is Phase 2.)

**Conclusion: Option A is not available for sync-message plugins in pact-js.**

## Decision — Option B via the engine's stdio mock
The adapter's `McpPact.executeTest`:
1. Authors + emits a real pact via the pact-js V4 plugin flow (`usingPlugin` +
   `withPluginContents`), which invokes the Rust engine's `ConfigureInteraction`
   (proven working — the content-type regex fix in ADR 0004 made this route).
2. Spawns the Rust engine's **stdio mock** (`pact-mcp-plugin mock`, reusing
   `src/mock.rs` = the engine matcher) reading that emitted pact, and hands the
   user's real `Client` a `StdioClientTransport` connected to it.
3. On teardown, reads the mock's results file and throws on any mismatch.

Rationale:
- **Matching stays 100% in Rust** — the mock reuses the engine's matcher; no TS
  matching. The provider verifier and conformance gate likewise delegate to the
  engine via thin `verify` / `compare` CLI subcommands (same Rust functions as
  the gRPC methods), avoiding a TS gRPC client + proto duplication.
- **The Client is genuinely real** — an unmodified `@modelcontextprotocol/sdk`
  `Client`. stdio is a first-class MCP transport, so "real Client ↔ real mock over
  stdio" is a legitimate, standard exchange, not a stub.
- **A real pact is emitted** by canonical pact-js.

The trade-off vs the §10 "zero-wire in-memory" ideal: the link is a local stdio
pipe to the mock subprocess rather than `InMemoryTransport.createLinkedPair()`.
A pure in-memory linked pair would require either TS-side matching (forbidden) or
a TS in-memory mock `Server` delegating every call to the engine (gRPC/CLI per
call). Deferred as future work; the stdio path is simpler, reuses the whole
engine mock, and is fully real.

## Consequences
- `withServer(inMemoryFactory)` provider verification and the in-memory linked
  pair are not implemented yet; `withServerTransport({type:"stdio",...})` is.
- Matcher helpers (`like`, `regex`, …) emit the engine's inline-DSL strings; they
  are NOT pact-js's matcher objects (incompatible formats). Documented in the
  adapter README.
