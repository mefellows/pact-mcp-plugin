# Pact MCP Plugin

Consumer-driven **contract testing for [Model Context Protocol (MCP)](https://modelcontextprotocol.io)** servers and clients, built as a [Pact](https://pact.io) plugin.

Test that an AI agent (MCP **client**) and the MCP **server** it depends on agree on the shape of `tools/call`, `tools/list`, `resources/read|list`, `prompts/get|list`, and their results — without spinning up the whole stack. You author expectations against your **real** `@modelcontextprotocol/sdk` `Client`, and verify them against your **real** MCP server. No stubbing, no service decomposition.

> **Status:** MVP+. stdio + Streamable HTTP transports, HTTP auth (bearer / API key / custom headers), provider verification **through the stock pact-js `Verifier`**, provider states, consumer mocks, multi-interaction pacts, and a TypeScript adapter — all proven end-to-end against real MCP clients/servers and the real Pact toolchain. See the [roadmap](#roadmap).

## Why

- **MCP has no contract-testing story.** Agents break when a server changes a tool's arguments or result shape; servers break their consumers without knowing.
- Core Pact can't test MCP: it has **no stdio transport** (and most MCP servers are stdio-first), no Streamable HTTP/SSE handling, and no knowledge of the MCP handshake or tool semantics. This plugin adds all of that.
- **One matching engine, every language.** Matching lives once in a Rust engine; language adapters are thin DX layers over it, so pacts are portable (author in TS, verify anywhere).

## Capabilities

| | stdio | Streamable HTTP |
|---|---|---|
| Provider verification | ✅ | ✅ (+ bearer / API key / custom headers) |
| Standard pact-js `Verifier` support (ADR 0008) | ✅ | ✅ |
| Provider states (`given(...)` + `stateHandlers`, ADR 0009) | ✅ | ✅ |
| Consumer mock | ✅ | ✅ (loopback) |
| `tools/call|list`, `resources/read|list`, `prompts/get|list` | ✅ | ✅ |
| Auto `initialize` handshake + capability negotiation | ✅ | ✅ |
| Matching in the shared Rust engine | ✅ | ✅ |
| TypeScript adapter DX | ✅ | ✅ |

Auth secrets use `${ENV}` interpolation and are **never written to the pact**.

## Quick start (TypeScript)

Install the adapter and the engine plugin:

```sh
npm install @pact-mcp/adapter
# engine: from a release…
curl -fsSL https://raw.githubusercontent.com/mefellows/pact-mcp-plugin/main/scripts/install-plugin.sh | bash
# …or from source:
./scripts/install-local.sh
```

**Consumer** — drive your real MCP client against a Pact-synthesized mock:

```ts
import { McpPact, like } from "@pact-mcp/adapter";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";

await new McpPact({ consumer: "weather-agent", provider: "weather-mcp" })
  .whenClientCallsTool("get_weather", { city: "Melbourne" })
  .willRespondWith({ content: [{ type: "text", text: like("Sunny, 22C") }], isError: false })
  .executeTest(async ({ transport }) => {
    const client = new Client({ name: "weather-agent", version: "1.0.0" }); // your REAL client
    await client.connect(transport);
    const res = await client.callTool({ name: "get_weather", arguments: { city: "Melbourne" } });
    expect(res.content[0].text).toContain("22C");
  });
// -> writes ./pacts/weather-agent-weather-mcp.json
```

**Provider** — verify that pact against your real server, over stdio or HTTP:

```ts
import { McpProviderVerifier } from "@pact-mcp/adapter";

await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: ["./pacts/weather-agent-weather-mcp.json"] })
  .withServerTransport({ type: "stdio", command: "node", args: ["dist/server.js"] })
  // or: { type: "http", url: "https://mcp.example.com/mcp", auth: { type: "bearer", token: "${MCP_TOKEN}" } }
  .verify();
```

Pacts also verify through the **stock pact-js `Verifier`** (broker fetch, result publishing, `can-i-deploy` — see [`docs/usage.md`](docs/usage.md) and [`docs/bdct-walkthrough.md`](docs/bdct-walkthrough.md)). Provider states, `tools/list` / `resources/*` / `prompts/*` expectations, and multi-interaction tests are covered in [`adapters/ts/pact-mcp/README.md`](adapters/ts/pact-mcp/README.md).

## Architecture

```
Shared contract spec (docs/spec/)  ── the pact schema + MCP matching semantics
        │ implemented by
Rust engine (rust/pact-mcp-plugin/) ── Pact plugin over gRPC: matching, generation,
        │                              stdio + Streamable HTTP transports, auth, mocks
        ├── TypeScript adapter (adapters/ts/pact-mcp/) ── thin DX; matching stays in the engine
        └── any Pact language (Java/.NET/Go/…) via the engine over a real transport
```

The engine is the universal backbone; each language connects its **real** MCP client/server to the engine's mock/verifier over a real transport (stdio or loopback HTTP). This is the one approach that works across every MCP SDK (all have stdio + HTTP client transports), including Java/.NET. See [`docs/plans/pact-mcp-plugin-implementation-plan.md`](docs/plans/pact-mcp-plugin-implementation-plan.md) and the [ADRs](docs/decisions/).

## Repository layout

```
docs/
  spec/           # shared contract spec + conformance fixtures (the anti-divergence gate)
  plans/          # implementation plan (source of truth)
  decisions/      # ADRs
  usage.md        # HTTP + auth usage
rust/
  pact-mcp-plugin/ # the engine (single binary Pact plugin)
adapters/
  ts/pact-mcp/    # TypeScript adapter (@pact-mcp/adapter)
examples/         # runnable consumer/provider examples + fixture MCP servers
pact-plugin.json  # Pact plugin manifest (name: mcp)
```

## Build & test

```sh
# Rust engine
cd rust && cargo test -p pact-mcp-plugin

# TypeScript adapter (drives the engine)
cd adapters/ts/pact-mcp && npm install && npm test
```

## Roadmap

- OAuth2 dynamic client registration (the `AuthProvider` seam is ready)
- Python / Go adapters + Java/.NET loopback examples (the shared spec + engine make these additive)
- Optional in-memory adapter DX for TS/Python/Go

## License

[MIT](LICENSE) © 2026 Matt Fellows
