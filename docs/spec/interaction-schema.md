# MCP ↔ Pact Interaction Schema (v0)

> Normative. The Rust engine and every adapter MUST produce/consume exactly this shape.
> Versioned: bump `schemaVersion` on any breaking change and keep old conformance fixtures.

## 1. Model

Each MCP operation is modelled as a **Pact V4 synchronous message** (request → response). This is transport-agnostic: the same interaction verifies over in-memory, stdio, or Streamable HTTP. The wire transport matters only at verification time.

The plugin owns everything protocol-level that the user should never hand-author:
- the JSON-RPC envelope (`jsonrpc: "2.0"`, `id`, `method`),
- the `initialize` → `initialized` handshake and capability/protocol-version negotiation (connection-level, once per session — **never** a per-interaction artifact).

The user authors only the **semantic** request and expected response.

## 2. Persisted interaction fragment

Stored in the pact file under the interaction's plugin-specific contents. `pact:content-type` routes it to this plugin.

```jsonc
{
  "pact:content-type": "application/mcp+json",
  "mcp": {
    "schemaVersion": "0",
    "operation": "tools/call",            // enum, see §3
    "request":  { /* operation-specific params, see §3 */ },
    "response": { /* operation-specific result, see §3 */ },
    "server":   {                          // OPTIONAL provider-verification hints
      "transport": "stdio"                 // "stdio" | "http" ; absent ⇒ adapter decides (in-memory/loopback)
    }
  }
}
```

Matching rules and generators are **not** nested inside `mcp`; they live in Pact's standard `matchingRules` / `generators` maps on the interaction, keyed by JSON path **relative to the `request`/`response` objects above**. Example:

```jsonc
"matchingRules": {
  "response": { "$.content[0].text": { "matchers": [ { "match": "type" } ] } }
}
```

Path roots:
- `request` rules are rooted at the `mcp.request` object.
- `response` rules are rooted at the `mcp.response` object.

## 3. Operations (v0)

Method is derived from `operation`; the engine synthesizes the JSON-RPC envelope. `request`/`response` below are the JSON-RPC `params` / `result` respectively.

### 3.1 `tools/call`  *(Phase 1 — build first)*
```jsonc
"request":  { "name": "get_weather", "arguments": { "city": "Melbourne" } }
"response": {
  "content": [ { "type": "text", "text": "Sunny, 22°C" } ],   // required (unless error)
  "isError": false,                                            // optional, default false
  "structuredContent": { /* optional arbitrary JSON */ }
}
```
Content block types: `text` (`{type,text}`), `image` (`{type,data,mimeType}`), `resource` (`{type,resource:{uri,...}}`). **VERIFY UPSTREAM** against the pinned MCP spec version; add block types as fixtures.

**JSON-RPC error result** (tool/protocol error) is represented as:
```jsonc
"response": { "error": { "code": -32602, "message": "Invalid params", "data": { /* optional */ } } }
```
`isError: true` (a *tool-level* error with content) is distinct from a JSON-RPC `error` (a *protocol-level* failure). Both are supported; fixtures cover each.

### 3.2 `tools/list`  *(Phase 1)*
```jsonc
"request":  {}                                  // or { "cursor": "..." }
"response": { "tools": [ { "name": "get_weather", "inputSchema": { /* JSON Schema */ } } ] }
```
Consumer-driven: the consumer asserts only the tools it uses; the server may expose more (subset match — see matching-semantics §4).

### 3.3 `resources/read`, `resources/list`  *(Phase 3.5 — implemented)*
```jsonc
// resources/read
"request":  { "uri": "weather://melbourne/report" }
"response": { "contents": [ { "uri": "weather://melbourne/report", "mimeType": "text/plain", "text": "..." } ] }
// resources/list
"request":  {}
"response": { "resources": [ { "uri": "weather://melbourne/report" } ] }   // subset by uri
```

### 3.4 `prompts/get`, `prompts/list`  *(Phase 3.5 — implemented)*
```jsonc
// prompts/get
"request":  { "name": "weather-report", "arguments": { "city": "Melbourne" } }
"response": { "description": "...", "messages": [ { "role": "user", "content": { "type": "text", "text": "..." } } ] }
// prompts/list
"request":  {}
"response": { "prompts": [ { "name": "weather-report" } ] }                // subset by name
```

A JSON-RPC error result (`"response": {"error": {...}}`) is supported for
`resources/read` / `prompts/get` exactly as for `tools/call` (§3.1).

## 4. Invariants

1. `operation` is a closed enum; unknown ⇒ hard error at `ConfigureInteraction`.
2. Exactly one of `response.content` / `response.error` is present for `tools/call` (plus optional `structuredContent`, `isError`).
3. Secrets never appear here — auth config lives on the verification/transport config, not the pact, and uses `${ENV}` interpolation.
4. The engine must round-trip this fragment losslessly (serialize → deserialize → equal). Covered by a unit test.
