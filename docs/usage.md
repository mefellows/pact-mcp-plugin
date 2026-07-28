# Usage — HTTP transport & auth

The engine supports two MCP transports for both consumer mocks and provider
verification: **stdio** and **Streamable HTTP**. Matching always runs in the Rust
engine.

## Secrets: `${ENV}`, never in the pact

Auth material lives ONLY on the verification/transport config — it is **never**
written into the pact file. Reference secrets with `${ENV}` interpolation,
resolved from the process environment at verification time:

```jsonc
{ "type": "bearer",  "token": "${MCP_TOKEN}" }
{ "type": "apiKey",  "header": "X-API-Key", "value": "${MCP_API_KEY}" }
{ "type": "headers", "headers": { "X-Tenant": "acme", "X-Sig": "${SIG}" } }
```

A missing env var is a hard error (never a silent empty header). The invariant
"secrets never land in the persisted pact" is enforced by construction
(`config.rs` has no auth input) and covered by a test
(`auth.rs::secrets_never_land_in_the_persisted_pact_fragment`).

Auth is injected on **every** HTTP request, including the `initialize` handshake.
stdio transports pass auth via env/args (no HTTP headers).

## Provider verification — standard pact-js Verifier (recommended)

With the plugin installed (`scripts/install-local.sh`, or a release install
under `~/.pact/plugins/mcp-<version>/`), MCP pacts verify through the **stock
pact-js `Verifier`** — no bespoke runner (ADR 0008). Pacts emitted by
`McpPact` are already stamped with the transport routing the verifier needs.

```ts
import { Verifier } from "@pact-foundation/pact";

// Streamable HTTP provider (running/deployed server):
await new Verifier({
  provider: "weather-mcp",
  providerBaseUrl: `http://127.0.0.1:${port}`,
  pactUrls: ["./pacts/weather-agent-weather-mcp.json"],
  transports: [{ protocol: "mcp-http", port }],
}).verifyProvider();

// stdio provider — the verifier can't carry a spawn spec, so pass it via env:
process.env.PACT_MCP_SERVER_COMMAND = "node";
process.env.PACT_MCP_SERVER_ARGS = "dist/server.js";
await new Verifier({
  provider: "weather-mcp",
  providerBaseUrl: "http://127.0.0.1:65500", // unused for plugin transports
  pactUrls: ["./pacts/weather-agent-weather-mcp.json"],
}).verifyProvider();
```

For `mcp-http`, auth comes from `PACT_MCP_AUTH` (a JSON auth config as below,
`${ENV}` interpolation applies) and a non-root endpoint path from
`PACT_MCP_SERVER_PATH`.

## Provider verification — adapter / engine CLI

### stdio (spawn the real server)

```ts
import { McpProviderVerifier } from "@pact-mcp/adapter";

await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: ["./pacts/…json"] })
  .withServerTransport({ type: "stdio", command: "node", args: ["dist/server.js"] })
  .verify();
```

### HTTP (verify a running / deployed server)

```ts
await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: ["./pacts/…json"] })
  .withServerTransport({
    type: "http",
    url: "https://mcp.example.com/mcp",
    auth: { type: "bearer", token: "${MCP_TOKEN}" }, // apiKey / headers also supported
  })
  .verify();
```

Engine CLI equivalents:

```sh
# stdio
pact-mcp-plugin verify --pact pact.json --command node --arg dist/server.js
# http + auth
pact-mcp-plugin verify --pact pact.json --url https://host/mcp \
  --auth '{"type":"bearer","token":"${MCP_TOKEN}"}'
```

The Streamable HTTP client handles `Mcp-Session-Id`, `Accept: application/json,
text/event-stream`, and both JSON-body and SSE response modes automatically
(rmcp — see ADR 0007). A 401 from missing/invalid auth surfaces as a clear
verification failure.

## Consumer mock over HTTP

```ts
import { McpPact, like } from "@pact-mcp/adapter";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";

await new McpPact({ consumer: "weather-agent", provider: "weather-mcp", mockTransport: "http" })
  .whenClientCallsTool("get_weather", { city: "Melbourne" })
  .willRespondWith({ content: [{ type: "text", text: like("Sunny, 22C") }], isError: false })
  .executeTest(async ({ transport }) => {
    const client = new Client({ name: "weather-agent", version: "1.0.0" });
    await client.connect(transport); // real Streamable HTTP client -> loopback mock
    const res = await client.callTool({ name: "get_weather", arguments: { city: "Melbourne" } });
    expect(res.content[0].text).toContain("22C");
  });
```

`mockTransport: "http"` stands up a loopback Streamable HTTP MCP mock on an
ephemeral port; the default (`"stdio"`) uses the engine mock over a stdio pipe.
Matching is identical (Rust engine) for both.
