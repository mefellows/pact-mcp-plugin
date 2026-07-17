# ADR 0007: Streamable HTTP transport via rmcp; AuthProvider design

## Status
Accepted

## Context
Phase 2 adds the Streamable HTTP transport (the second half of the universal
transport pair: stdio + loopback HTTP) plus auth, for verifying deployed MCP
servers and for the loopback HTTP consumer mock. The plan required a **VERIFY
UPSTREAM** check on whether rmcp's Streamable HTTP *client* transport supports
custom headers (needed for auth), with a `reqwest` hand-rolled fallback if not.

## Investigation (rmcp 2.2.0)
`rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig`
exposes exactly what we need:
- `auth_header: Option<String>` — a bearer token (without the `Bearer ` prefix);
- `custom_headers: HashMap<HeaderName, HeaderValue>` — arbitrary headers, injected
  on **every** request including `initialize`;
- automatic `Mcp-Session-Id` capture/resend, `Accept: application/json,
  text/event-stream`, and both response modes (JSON body + `text/event-stream`
  SSE) — all handled by the transport.

The `transport-streamable-http-client-reqwest` feature adds
`StreamableHttpClientTransport::from_config(config)` using a bundled reqwest
client. A scratch build with the feature compiled and the config builder
(`with_uri(url).auth_header(tok).custom_headers(map)`) is public and stable.

## Decision
Use **rmcp's Streamable HTTP client transport** (reqwest convenience feature),
mirroring the stdio decision (ADR 0003). No hand-rolled reqwest JSON-RPC client
is needed — rmcp already does session handling, SSE/JSON negotiation, and custom
headers. If a future need (e.g. non-reqwest TLS, streaming quirks) forces a
hand-roll, revisit here.

For the **server** side (loopback HTTP consumer mock) we use rmcp's
`transport-streamable-http-server` with an `axum` router on an ephemeral port.

## AuthProvider design
`auth::AuthProvider` resolves an auth config into a `ResolvedAuth { auth_header:
Option<String>, custom_headers: Vec<(name, value)> }`:
- `bearer { token }` → `auth_header = token` (rmcp adds the `Bearer ` prefix);
- `apiKey { header, value }` → a single custom header;
- `headers { map }` → custom headers verbatim.

All values support `${ENV}` interpolation, resolved at verification time from the
process environment. **Secrets are never written to the pact** — auth lives only
on the verification/transport config, which is not part of the persisted
interaction fragment (`config.rs` has no auth input; a test asserts a configured
interaction's JSON contains no auth material). stdio auth stays env/args
passthrough (no HTTP headers). The trait is left clean for an OAuth2 impl
(Phase 4) that will resolve to the same `ResolvedAuth`.
