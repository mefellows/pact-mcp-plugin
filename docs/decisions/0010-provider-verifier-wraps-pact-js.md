# ADR 0010: McpProviderVerifier is a thin wrapper over the pact-js Verifier

## Status
Accepted. Supersedes the adapter-side engine-CLI verification path introduced
with the TS adapter (ADR 0006 era).

## Context
The TS adapter originally shipped a `McpProviderVerifier` that drove provider
verification by shelling out to the engine's `verify` CLI and formatting its own
mismatch summary. It read local `pactUrls` and reported results itself. That
path had **none** of the Pact framework's machinery: no Pact Broker / PactFlow
fetch, no consumer version selectors, no publishing of verification results, no
`can-i-deploy`, no pending/WIP pacts, and non-standard reporting. It looked like
the blessed API but was a dead-end for any real (broker-based) workflow.

Meanwhile ADR 0008 already proved the stock pact-js `Verifier` drives
verification through the installed plugin (per-interaction `transport` routing).
So the framework path existed and worked — the adapter was reimplementing a
worse subset of it.

## Decision
Reduce `McpProviderVerifier` to a **thin wrapper over `new Verifier(...)`**
(`@pact-foundation/pact`). Its only job is to assemble the MCP-specific transport
configuration and forward everything else:

- **stdio** → `PACT_MCP_SERVER_COMMAND` / `PACT_MCP_SERVER_ARGS` (+ optional
  `env`) and a placeholder `providerBaseUrl`.
- **http** → `transports: [{ protocol: "mcp-http", port }]`, `providerBaseUrl`
  parsed from the URL, `PACT_MCP_SERVER_PATH` / `PACT_MCP_AUTH` env.
- **broker** (`fromPactBroker`) → `pactBrokerUrl` / token / selectors / pending /
  WIP; plus `providerVersion`, `providerVersionBranch`,
  `publishVerificationResult`, `stateHandlers`, and a `withVerifierOptions`
  escape hatch — all passed straight to pact-js.

Config assembly is a pure `buildVerifierConfig()` (unit-tested); `verify()`
applies the env, calls `verifyProvider()`, and restores the env. Failure
reporting is now pact-js's own — no bespoke error string.

The adapter no longer calls the engine `verify` CLI (`runVerify` /
`runVerifyHttp` removed). The engine's `verify` subcommand remains for its gRPC
path and direct CLI use. Provider states keep working via both routes with no
adapter code: the plugin seeds the spawned server from the interaction's
`providerStates` (ADR 0009), and pact-js invokes `stateHandlers`.

## Consequences
- Broker fetch, result publishing, `can-i-deploy`, selectors, pending/WIP, and
  standard reporting come for free — the wrapper is the standard framework.
- One verification implementation, not two. Less to maintain and no divergence.
- `.verify()` now returns the verifier output string and rejects on failure
  (previously returned a `VerifyResult[]`). Tests updated to freshly-emitted,
  transport-stamped pacts (the stock verifier routes on the top-level
  `transport` field — ADR 0008 — which older committed pacts lack).
- Users can equally use `new Verifier(...)` directly; the wrapper is convenience.
