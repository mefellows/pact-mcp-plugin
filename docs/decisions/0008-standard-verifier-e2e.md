# ADR 0008: Standard-verifier E2E — transport routing + config delivery

## Status
Accepted

## Context
The plan (§8) requires MCP pacts to verify from the **standard** Pact verifier
(pact-js `Verifier` / `pact_verifier_cli`) with the plugin installed — no
bespoke runner. Open item §16.5 asked how provider transport config (stdio
command, HTTP url/auth) and provider states reach the plugin's
`VerifyInteraction`. We verified upstream (pact-reference `pact_verifier`
1.3.5 `lib.rs::verify_interaction_using_transport`, pact-plugin-driver 0.7.5
`plugin_manager.rs`, pact-js 17 / pact-core 19 `verifier/types.d.ts`):

1. **Routing:** the verifier routes an interaction to a plugin transport ONLY
   when the V4 interaction carries a top-level `"transport": "<name>"` field —
   it looks up `transport/<name>` in the plugin catalogue
   (`interaction.as_v4().transport()` → `catalogue_manager::lookup_entry`).
   `pact_models::v4::sync_message::SynchronousMessage` supports and round-trips
   the field, but **pact-js's non-transport sync-message flow
   (`executeNonTransportTest`, our ADR 0006 Option B) never sets it** — pacts we
   emit today have no `transport`, so the stock verifier would fall through to
   plain-HTTP verification and fail.
2. **Config:** the verifier's `VerificationPreparationRequest.config` /
   `VerifyInteractionRequest.config` Struct contains exactly:
   `host` (from `providerBaseUrl`), optional `port` (from the
   `transports: [{ protocol, port }]` verifier option whose `protocol` suffix
   matches the catalogue transport key), and `providerState` (the interaction's
   provider-state params merged with verifier context). Nothing else — pact-js's
   `Transport` type is `{protocol, port, scheme?, path?}` and cannot carry a
   stdio spawn spec or auth config.

## Decision

### Routing — the adapter stamps `transport` onto the emitted pact
After pact-js writes the pact, `McpPact` post-processes the file and sets each
mcp interaction's top-level `"transport"` to the catalogue key: `mcp-stdio`
(default) or `mcp-http` (when `mockTransport: "http"`). This is the same field
`pactffi_create_mock_server_for_transport`-based tests get natively; we set it
ourselves because the non-transport sync-message flow cannot.

### Config — resolution ladder in `VerifyInteraction`
The engine resolves the provider target per interaction transport:

- **`mcp-http`**: URL = `http://{config.host}:{config.port}{path}` where `path`
  defaults to `/` and may be overridden with `PACT_MCP_SERVER_PATH`. Auth (which
  the verifier options cannot express) comes from `PACT_MCP_AUTH` — a JSON auth
  config (same shape as ADR 0007, `${ENV}` interpolation applies). Users run:
  `new Verifier({ providerBaseUrl, pactUrls, transports: [{ protocol: "mcp-http", port }] })`.
- **`mcp-stdio`**: spawn spec resolved in order:
  1. `config.command` / `config.args` (used by our own CLI/tests, and available
     to any driver that passes user config through);
  2. `PACT_MCP_SERVER_COMMAND` + `PACT_MCP_SERVER_ARGS` (whitespace-split) env
     vars on the verifier process — the plugin subprocess inherits its
     environment, so this works with stock pact-js;
  3. otherwise a hard error naming both mechanisms.
- Interaction transport is read from the pact interaction's top-level
  `transport`, falling back to the `mcp.server.transport` hint (`stdio`/`http`)
  for pacts written before this ADR.

Response matching rules from the pact are applied on this path (previously the
gRPC verify path dropped them — fixed alongside this ADR).

`providerState` handling is specified in ADR 0009 (task 1.2).

### Install layout
`scripts/install-local.sh` builds the release binary and installs
`~/.pact/plugins/mcp-<version>/{pact-mcp-plugin,pact-plugin.json}` — the layout
the plugin driver expects from the pact metadata `plugins: [{name: "mcp",
version}]` entry.

## Consequences
- A pact authored by the TS adapter verifies with the stock pact-js `Verifier`
  (proven by `adapters/ts/pact-mcp/test/standard-verifier.test.ts`, which
  installs the plugin, generates a pact, and verifies it over both transports).
- stdio spawn config and HTTP auth are deliberately environmental (env vars),
  not persisted in the pact — consistent with the secrets invariant (ADR 0007).
- The committed evidence pact gains a `"transport"` field; old pacts still
  verify via the `mcp.server.transport` fallback.
