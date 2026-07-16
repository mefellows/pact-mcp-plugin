# ADR 0004: ConfigureInteraction convention — inline-DSL matchers + two-part sync message

## Status
RESOLVED. A real pact-js V4 plugin consumer test now drives our plugin, emits a
pact, and that pact is verified end-to-end by our engine against the fixture MCP
server. Root cause of the earlier FFI-routing block found and fixed (see
"Root cause"). Evidence pact committed at
`examples/ts-roundtrip/pacts-committed/weather-agent-weather-mcp.json`; the live
round trip is a test (`tests/live_roundtrip_verify.rs`).

## Root cause of the FFI-routing block (RESOLVED)
pact-core never invoked `ConfigureInteraction` because the pact plugin driver
matches a content-matcher catalogue entry's `content-types` value as a **regex
anchored at both ends** (`^(?:<value>)$`) against the incoming content type
(confirmed in `pact-plugins` `catalogue_manager.rs::matches_pattern`, whose doc
comment states: *"Regex metacharacters in a content type (most commonly `+`, as
in a `+json`/`+xml` structured syntax suffix) need to be escaped by the plugin
author for a literal match."*). Our value `application/mcp+json` is a regex in
which `+` is a one-or-more quantifier (`mcp+` = "mc" then one-or-more "p"), so it
matched `application/mcpjson` and never the literal `application/mcp+json` → no
content matcher found → the interaction contents were silently dropped and
pact-js threw `Retrieved an empty message`. pact-protobuf-plugin never hit this:
its content types (`application/protobuf;application/grpc`) contain no regex
metacharacters. **Fix:** `catalogue.rs` now registers
`application/mcp\+json` (escaped `+`) via `CONTENT_TYPE_PATTERN`.

## Context
`ConfigureInteractionRequest.contentsConfig` is "data specified by the user in
the consumer test". The original implementation guessed both (a) how consumers
express matchers and (b) how the plugin returns rules. This ADR records what was
subsequently VERIFIED against the real Pact ecosystem.

## CONFIRMED (via research + a live-but-partial pact-js run)

### 1. Matchers are authored inline as DSL strings (not a matchingRules block)
Confirmed by reading `pactflow/pact-protobuf-plugin`
(`src/protobuf/mod.rs` `build_proto_value`): a consumer expresses a matcher as a
**Pact matcher-definition DSL string** embedded in the leaf value of the
request/response JSON — e.g. `"text": "matching(type, 'Sunny, 22C')"`,
`"name": "notEmpty('x')"`, `"zoom": "matching(number, 5)"`,
`"text": "matching(regex, '^[A-Z].*', 'Sunny')"`. There is **no** separate
`matchingRules` block and **no** `{"pact:matcher:type": ...}` marker object.

`config.rs` now parses these with the SAME functions the protobuf plugin uses —
`pact_models::matchingrules::expressions::{is_matcher_def, parse_matcher_def}`
(crate `pact_models` 1.3.11) — walks the trees, strips each matcher leaf to its
example value for the persisted body, and records a rule at its `DocPath`. Rule
JSON round-trips as `{"match":"type"}` etc. via `MatchingRule::to_json`.

### 2. A sync-message plugin returns TWO InteractionResponse parts
Confirmed by (a) `pact-protobuf-plugin` source
(`construct_protobuf_interaction_for_service` returns a `part_name: "request"`
InteractionResponse plus a `part_name: "response"` one) AND (b) the live pact-js
run: with a single merged `InteractionResponse`, pact core failed with
`Retrieved an empty message` from `pactffiGetSyncMessageRequestContents` —
because there was no `request` part. `server.rs::configure_interaction` now
returns two parts, each with `part_name` set, its own body, and rules keyed
`$.<path>` **rooted at that part's own body** (NOT namespaced `$.request.*`/
`$.response.*` — that earlier guess was wrong). `operation` + `server` are
carried in each part's `pluginConfiguration.interactionConfiguration` (they are
not part of the body). This exactly mirrors the protobuf plugin.

### 3. The plugin loads and InitPlugin is routed
The live run confirmed pact-core (`@pact-foundation/pact` 17, `pact-core`
19.2.0, native FFI 0.5.4) finds the installed plugin at
`~/.pact/plugins/mcp-0.1.0/`, loads it, and calls `InitPlugin` with our
catalogue (verified via a temporary file-logging probe in the plugin).

### 4. Direct-gRPC contract test
`tests/grpc_bootstrap.rs::configure_interaction_returns_two_part_sync_message...`
exercises the EXACT gRPC call pact core makes: it hands the plugin a
`contentsConfig` Struct with an inline `matching(type,...)` and asserts the
two-part response, the stripped example body, the `$.content[0].text` type rule,
and the `operation` in pluginConfiguration. This validates our side of the
contract even though the FFI round trip could not complete.

## The real persisted pact shape (now OBSERVED, was residual risk)
The pact emitted by the live pact-js run (committed under
`examples/ts-roundtrip/pacts-committed/`) confirms:
- **Two-part sync message.** `interaction.request.contents.content` is the
  request body (`{name, arguments}`); `interaction.response` is an ARRAY and
  `response[0].contents.content` is the response body (`{content, isError}`).
  Each `contents` also carries `contentType: "application/mcp+json"`,
  `contentTypeHint: "TEXT"`, `encoded: false`.
- **Rule category + keys.** Response-part matchers persist under
  `response[0].matchingRules.body`, keyed `$.content[0].text` (rooted at the
  part body, NOT namespaced), each as `{combine: "AND", matchers: [{match:
  "type"}]}` — exactly what `config::persisted_body_category` renders. Our
  earlier `$.request.*`/`$.response.*` namespacing guess was wrong and has been
  removed; the two-part refactor is correct.
- **operation/server.** Persisted in
  `interaction.pluginConfiguration.<pluginName>` (i.e.
  `pluginConfiguration.mcp.operation` / `.server.transport`).
- **Interaction key.** pact-js uses the interaction `description`; no separate
  `key` field is emitted.

`server.rs::interaction_from_value` + `response_matching_rules` read exactly this
shape (and still tolerate our single-fragment `examples/` shape). The engine
verifies the real pact-js-authored pact against the fixture MCP server in
`tests/live_roundtrip_verify.rs`.

## Notes
- The direct-gRPC contract test
  (`tests/grpc_bootstrap.rs::configure_interaction_returns_two_part_sync_message...`)
  is kept alongside the live round trip — it exercises the plugin's gRPC surface
  without needing node/pact-js installed, so it stays useful in CI environments
  where the live path can't run.
- Reproduce the authoring step: `examples/ts-roundtrip/generate-pact.mjs`
  (requires the plugin installed at `~/.pact/plugins/mcp-<version>/`).
