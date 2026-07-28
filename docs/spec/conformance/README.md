# Conformance Fixtures (v0)

Golden `{interaction, actual, expected-result}` triples that pin the MCP matching semantics. **The Rust engine and the TS adapter both run every fixture as a test.** A conformance failure is a release blocker — it means an implementation diverged from the spec.

## Fixture format

Each `*.json` file:

```jsonc
{
  "description": "human-readable intent",
  "operation": "tools/call",                 // must match interaction.mcp.operation
  "interaction": {
    "mcp": { "schemaVersion": "0", "operation": "...", "request": {...}, "response": {...} },
    "matchingRules": { "response": { "<json-path>": { "matchers": [ { "match": "type" } ] } } },
    "generators": {}
  },
  "actual": { /* the actual JSON-RPC result (or {"error": {...}}) received from the server/mock */ },
  "expected": {
    "match": true,                            // did the comparison pass?
    "mismatchPaths": []                       // sorted JSON paths that must be reported when match=false
  }
}
```

## Assertion contract (keep implementations honest but not brittle)

A conforming implementation, given `interaction` + `actual`, MUST return a result where:
1. `result.match === expected.match`, AND
2. the **set** of mismatch paths equals `expected.mismatchPaths` (order-independent).

Exact mismatch **message wording is NOT asserted** (engine-defined, may evolve). If a fixture wants to assert message content, it may add `"messageContains": ["substr", ...]` under `expected` — implementations then assert each substring appears in the mismatch for that path.

## Runner expectations

- Rust: a `#[test]` per fixture file (or a parametrized harness) under `rust/pact-mcp-plugin/tests/conformance.rs` that loads these files (path via a shared const or `CARGO_MANIFEST_DIR`-relative walk to `docs/spec/conformance`).
- TS: a Vitest/Jest suite under `adapters/ts/pact-mcp` that loads the same directory and drives the engine.
- Adding a new fixture here must make both suites pick it up automatically (glob the directory).

## Index

| File | Asserts |
|---|---|
| `tools-call-text-exact-pass.json` | literal text matches exactly |
| `tools-call-text-type-pass.json` | `{"match":"type"}` accepts any string |
| `tools-call-text-mismatch.json` | wrong literal text ⇒ mismatch at `$.content[0].text` |
| `tools-call-iserror-mismatch.json` | surprise `isError:true` ⇒ mismatch at `$.isError` |
| `tools-call-error-result-pass.json` | JSON-RPC error: code exact, message by type |
| `tools-call-error-data-structure-mismatch.json` | `error.data` by structure: missing specified key ⇒ mismatch |
| `tools-call-cross-shape-mismatch.json` | expected success but actual is a protocol error ⇒ mismatch at `$` |
| `tools-call-image-block-pass.json` | image: `mimeType` exact, `data` type-only ⇒ different payload passes |
| `tools-call-image-mimetype-mismatch.json` | image: wrong `mimeType` ⇒ mismatch |
| `tools-call-resource-block-pass.json` | resource: `type` + `resource.uri` exact ⇒ pass; extra fields ignored |
| `tools-call-resource-uri-mismatch.json` | resource: wrong `resource.uri` ⇒ mismatch |
| `tools-call-structured-content-pass.json` | `structuredContent`: provided keys matched, extra actual keys ignored |
| `tools-call-structured-content-mismatch.json` | `structuredContent`: wrong scalar + missing key ⇒ mismatches |
| `tools-call-extra-trailing-blocks-pass.json` | extra trailing actual content blocks ignored (consumer-driven) |
| `tools-list-subset-pass.json` | consumer asserts one tool; server exposes more ⇒ pass |
| `tools-list-missing-tool-mismatch.json` | relied-on tool absent ⇒ mismatch keyed by name |
| `tools-list-inputschema-mismatch.json` | matched tool's `inputSchema` missing a specified key ⇒ mismatch |
| `resources-read-pass.json` | resources/read: specified contents keys matched, extras ignored |
| `resources-read-text-mismatch.json` | resources/read: wrong text scalar ⇒ mismatch |
| `resources-list-subset-pass.json` | resources/list: subset by `uri`, order-independent |
| `prompts-get-pass.json` | prompts/get: structural match; type matcher on message text |
| `prompts-list-missing-mismatch.json` | prompts/list: relied-on prompt absent ⇒ mismatch by name |

Request-side matching (mock interaction selection, matching-semantics §4) is
covered by engine unit tests (`content/mod.rs`), not fixtures — the fixture
format is a response comparison.
