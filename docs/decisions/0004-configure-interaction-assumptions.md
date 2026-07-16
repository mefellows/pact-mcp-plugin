# ADR 0004: ConfigureInteraction convention — inline-DSL matchers + two-part sync message

## Status
Partially resolved. Input contract + response shape CONFIRMED against real
sources; live pact-js FFI round trip BLOCKED (see "Residual risk").

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

## Residual risk — BLOCKED (live FFI routing)
Despite the plugin loading and InitPlugin being called, pact-core's FFI
**never invoked our `ConfigureInteraction`** for `withPluginContents(...,
"application/mcp+json")` — verified by instrumenting the plugin: `InitPlugin
called` was logged, `ConfigureInteraction ENTER` never was. pact-js ignores the
FFI return code, so the interaction ends up with empty contents and the test
throws `Retrieved an empty message`. The native FFI trace logs did not surface
in this environment (`pactffiInitWithLogLevel`/`RUST_LOG`/`LOG_LEVEL=trace`
produced no plugin-driver trace), so the routing failure could not be diagnosed
further. This is a bounded stop per the run instructions.

Consequently, these remain UNCONFIRMED by a live end-to-end run:
- The exact category pact core persists the two-part rules under in the final
  pact file (expected `request`/`response` parts each with
  `matchingRules.body.$.<path> = {combine, matchers}` — `config::persisted_body_category`
  renders that shape and a unit test pins it, but it was not observed in a real
  emitted pact file).
- The exact interaction key convention pact core uses for plugin sync messages.

`server.rs::interaction_from_value` tolerates BOTH the single-fragment
`examples/` shape and the real two-part shape (request/response contents +
operation from pluginConfiguration) so provider verification works against
whichever pact-core emits, but only the single-fragment path is exercised by a
real fixture pact in tests today.

## Follow-up
Re-attempt the live round trip on an environment where the pact-core native FFI
plugin-driver trace logs are available (or a newer pact-js/native FFI), confirm
the persisted pact shape, and close the two residual items above. The reproducer
is committed at `examples/ts-roundtrip/generate-pact.mjs`.
