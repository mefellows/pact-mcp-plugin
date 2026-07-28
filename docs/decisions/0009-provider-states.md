# ADR 0009: Provider states for MCP interactions

## Status
Accepted

## Context
Plan §8 / Phase 3.5: map Pact provider states onto MCP server setup. Verified
upstream (pact-reference `pact_verifier` lib.rs): `execute_provider_states`
runs **before** the interaction is routed to a plugin transport, through the
standard `ProviderStateExecutor` — i.e. pact-js's `stateHandlers` (served via
its verifier proxy + `providerStatesSetupUrl`) fire for plugin-transport
interactions with **no plugin involvement needed**. The interaction's state
params are additionally merged into the `config.providerState` Struct the
plugin receives (ADR 0008).

## Decision

### Authoring
`McpPact.given(state, params?)` → pact-js `.given(...)` → the standard V4
`providerStates` field on the emitted interaction. Nothing plugin-specific is
persisted.

### Verification-time application (three routes, by runner)
1. **Standard pact-js Verifier (recommended):** users pass ordinary
   `stateHandlers`; the verifier invokes them before each interaction. Works
   today by construction — pinned by an E2E test where a state handler seeds
   the data the fixture server returns.
2. **Engine (CLI / gRPC / adapter) — stdio:** the engine passes the
   interaction's states to the spawned server as
   `PACT_MCP_PROVIDER_STATES` — a JSON array `[{"name": "...", "params": {...}}]`
   — set on the child process env. The server seeds itself at startup
   (fixture `weather-server.mjs` demonstrates). One spawn per interaction, so
   states cannot leak across interactions.
3. **TS `McpProviderVerifier.stateHandlers({...})`:** invoked in-process before
   verification of each interaction (for in-repo servers whose state lives
   behind seams the test controls). Handlers run for the states of every
   interaction in the pact, in order.

HTTP targets (deployed servers) use route 1 (or seed out-of-band); a dedicated
state-change POST endpoint is deliberately NOT invented here — the standard
`providerStatesSetupUrl` mechanism already covers it.

## Consequences
- `verify_interaction_stdio` gains the interaction's states; the engine CLI and
  gRPC verify paths thread them through automatically from the pact.
- The fixture stdio server understands `PACT_MCP_PROVIDER_STATES` (params
  `{city, weather}` override its canned data).
- No pact-format or spec changes: states use the standard V4 field.
