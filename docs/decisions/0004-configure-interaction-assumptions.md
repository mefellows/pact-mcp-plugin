# ADR 0004: ConfigureInteraction contentsConfig shape + rule path convention (best-effort)

## Status
Accepted, with a flagged open item

## Context
`ConfigureInteractionRequest.contentsConfig` (a `google.protobuf.Struct`) is
"data specified by the user in the consumer test" — but the exact shape a real
pact-js/pact-jvm V4 synchronous-message plugin builder sends, and the exact
convention pact core uses to re-root a plugin's returned
`InteractionResponse.rules` (a flat `map<string, MatchingRules>`) into the
persisted interaction's `matchingRules.request` / `matchingRules.response`
maps, was **not** independently re-verified against a real consumer-side pact
library round trip in this task (Phase 1 scope is the Rust engine + stdio
provider verification; no TS/JVM adapter work was done — see plan §11 Phase 3).

## Decision (pragmatic MVP shortcut)
`config::configure_interaction` assumes `contentsConfig` is shaped like the
`mcp` object in docs/spec/interaction-schema.md §2, plus an inline
`matchingRules: { request: {...}, response: {...} }` (same shape as the
conformance fixtures):

```jsonc
{
  "operation": "tools/call",
  "request": { ... },
  "response": { ... },
  "matchingRules": { "response": { "$.content[0].text": { "matchers": [...] } } },
  "server": { "transport": "stdio" }
}
```

Returned proto rules are namespaced `$.request.<path>` / `$.response.<path>`
(stripping the fixtures' `$.` root and re-adding it under the section prefix).

This round-trips correctly through our own `content::compare_response` (see
`config::tests::configures_a_tools_call_interaction_and_round_trips...`), but
has **not** been validated against real pact-js/pact-jvm output.

## Consequences / follow-up
When Phase 3 (TS adapter) is built, this must be verified against actual
pact-js plugin builder output and corrected if the real convention differs
(e.g. if pact core expects rules keyed without the `request`/`response`
prefix, or expects the inline-matcher-object DSL used by
`pact-protobuf-plugin` — special JSON marker objects like
`{"pact:matcher:type":"type","value":"..."}` embedded directly in the request/
response JSON — rather than a separate `matchingRules` block). Treat this ADR
as provisional until that cross-implementation check happens.
