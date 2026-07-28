# MCP Matching Semantics (v0)

> Normative. Defines how the engine compares an **actual** MCP payload against the **expected** one in an interaction. Reuses Pact's JSON matching engine; adds MCP-aware defaults. Every rule here has a conformance fixture (`./conformance/`).

## 0. Principles

- **Consumer-driven:** assert only what the consumer relies on. Unspecified fields on the actual payload are *ignored* unless a matcher says otherwise.
- **Matchers over equality:** when the interaction carries a matching rule for a path, apply it; otherwise fall back to the default for that path (below), which is usually **exact** for scalars the consumer wrote literally.
- **One implementation:** matching lives only in the Rust engine. Adapters marshal payloads to it and render results; they never re-implement these rules.

## 1. Matching direction & rooting

- Compare `mcp.response` (expected) against the actual JSON-RPC `result` (or `error`).
- Rules in `matchingRules.response` are rooted at the `mcp.response` object (see interaction-schema §2). `$` = the response object.
- (Request matching, used by the mock to decide which interaction an incoming call matches, roots `matchingRules.request` at `mcp.request`.)

## 2. `tools/call` result

### 2.1 `content[]`
- Default: **type-aware, ordered** match. Each expected block must match the actual block at the same index.
  - `text`: match `type` exactly; match `text` per its rule, default **exact**. A `{"match":"type"}` rule ⇒ any string.
  - `image`: match `type` + `mimeType` exact (default); `data` per rule, default type-only (don't force byte-equality unless asked).
  - `resource`: match `type` + `resource.uri` (default exact); other fields per rule.
- The actual array MUST have **at least** the expected length; extra trailing blocks are a mismatch only if the consumer used an exact-length matcher. (Default: expected blocks must all be present in order; extra actual blocks are ignored — consumer-driven.) **Encode this precisely in fixtures; if Pact's array semantics differ, the fixtures are the source of truth and the engine conforms to them.**

### 2.2 `isError`
- Matched **exactly** by default. If expected omits `isError`, treat expected as `false` and match exactly (a surprise `isError: true` is a mismatch).

### 2.3 `structuredContent`
- Full JSON matching with the consumer's matchers; default exact for provided scalars, keys not mentioned are ignored (Pact "type"/partial object semantics). No special MCP treatment.

### 2.4 JSON-RPC `error` result
- If expected `response.error` is present: match `error.code` **exact**, `error.message` **by type** (default), `error.data` **by structure** (keys present matched by type). 
- Cross-shape mismatch is an error: expected `content` but actual is `error` (or vice-versa) ⇒ mismatch with a clear message.

## 3. `tools/list` result

- **Subset match** on `tools[]` keyed by `name`: every expected tool must be present in the actual list (by `name`); actual may contain more. Order-independent.
- For each matched tool, compare `inputSchema` by **structure** (JSON Schema shape) using the consumer's matchers; default: keys the consumer specified must be present and type-compatible; extra keys ignored.

## 3.5 `resources/read`, `prompts/get` results

Structural comparison rooted at `$`: keys the consumer specified must be
present and match (scalars **exact** unless a matcher rules otherwise); extra
actual keys/array-tail items are ignored (consumer-driven). Cross-shape
(success vs JSON-RPC `error`) mismatches at `$` as in §2.4.

## 3.6 `resources/list`, `prompts/list` results

Subset match like §3: every expected item must be present in the actual list —
`resources[]` keyed by `uri`, `prompts[]` keyed by `name` — order-independent;
other specified keys on a matched item are compared structurally.

## 4. Request matching (mock server)

When the mock receives a `tools/call`, select the interaction whose `mcp.request.name` equals the incoming tool name AND whose `arguments` match under `matchingRules.request` (default exact for literals, matchers as authored). No match ⇒ record a mismatch and return a protocol error to the client. Ambiguous match (>1) ⇒ first wins, but log a warning.

## 5. Mismatch reporting

A comparison returns `{ match: bool, mismatches: [ { path, expected, actual, message } ] }`. `path` is a JSON path rooted as in §1. The **conformance fixtures assert `match` and the set of mismatch `path`s** (and optionally a substring of `message`); exact message wording is engine-defined and may evolve without breaking conformance.

## 6. Defaults summary

| Field | Default matcher |
|---|---|
| `content[i].type` | exact |
| `content[i].text` | exact (literal) / type (if ruled) |
| `content[i].data` (image) | type |
| `content[i].mimeType` | exact |
| `content[i].resource.uri` | exact |
| `isError` | exact (missing ⇒ false) |
| `structuredContent.*` | exact for provided scalars; extra keys ignored |
| `error.code` | exact |
| `error.message` | type |
| `error.data.*` | structure/type |
| `tools[]` (list) | subset by `name`, order-independent |
| `inputSchema` | structure |
