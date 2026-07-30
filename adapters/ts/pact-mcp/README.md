# @pactflow/pact-mcp-plugin

Ergonomic TypeScript DX for **Pact MCP** contract testing. You connect your
**real** `@modelcontextprotocol/sdk` `Client` / `Server` — no stubbing, no
service decomposition. **All matching runs in the Rust engine**; this package is
a thin orchestration + DX layer.

## Install

```sh
npm install @pactflow/pact-mcp-plugin
```

On install, a `postinstall` step downloads the matching **engine binary** (the
Pact `mcp` plugin) for your platform from GitHub releases and installs it to
`~/.pact/plugins/mcp-<version>/`, where both the Pact plugin driver and this
package look for it. The install never hard-fails: if the download can't run
(offline, air-gapped, unsupported platform) it prints guidance and you can
provision later with:

```sh
npx pact-mcp-install          # re-run the download
# or, from source:  ./scripts/install-local.sh   (in the plugin repo)
```

Opt out of the automatic download with `PACT_MCP_SKIP_INSTALL=1` (e.g. in CI
where you provision the engine yourself).

## Consumer

```ts
import { McpPact, like } from "@pactflow/pact-mcp-plugin";
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

`like`, `regex`, `number`, `integer`, `boolean`, `notEmpty` are authoring helpers
that emit the engine's inline matcher DSL. (They intentionally do **not** re-use
pact-js's own matcher objects — the MCP content matcher consumes inline-DSL leaf
strings, a different format. Matching itself is 100% in the Rust engine.)

## Provider

```ts
import { McpProviderVerifier } from "@pactflow/pact-mcp-plugin";

// stdio — the engine spawns your real server
await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: ["./pacts/weather-agent-weather-mcp.json"] })
  .withServerTransport({ type: "stdio", command: "node", args: ["dist/server.js"] })
  .verify();

// http — verify a running / deployed server, with auth
await new McpProviderVerifier({ provider: "weather-mcp", pactUrls: ["./pacts/weather-agent-weather-mcp.json"] })
  .withServerTransport({
    type: "http",
    url: "https://mcp.example.com/mcp",
    auth: { type: "bearer", token: "${MCP_TOKEN}" }, // or apiKey / headers
  })
  .verify();
```

## HTTP + auth

Both the consumer mock (`new McpPact({ …, mockTransport: "http" })`) and provider
verification support Streamable HTTP. Auth kinds: `bearer`, `apiKey { header,
value }`, `headers { … }`. Secret values may use `${ENV}` interpolation and are
**never written to the pact** — they live only on the verification/transport
config. See `docs/usage.md`.

## How it works — the consumer flow (Option B)

pact-js's synchronous-message plugin flow does **not** expose a live in-memory
mock transport that the consumer's real `Client` connects to:
`withPluginContents(...).executeTest(cb)` hands the callback the *configured
contents* and writes the pact — there is no live consumer mock (the only
live-transport path, `startTransport`, needs a listening-socket mock, i.e. the
HTTP transport, which is Phase 2). See `docs/decisions/0006-ts-adapter-consumer-flow.md`.

So this adapter:

1. Authors + emits a **real pact** via the pact-js V4 plugin flow (which invokes
   the Rust engine's `ConfigureInteraction`).
2. Spawns the Rust engine's **stdio mock** (`pact-mcp-plugin mock`, reusing the
   engine matcher) reading that pact, and hands your real `Client` a
   `StdioClientTransport` connected to it. stdio is a first-class MCP transport,
   so this is a genuine real-Client ↔ real-mock exchange — matching stays in Rust.
3. On teardown, asserts the mock recorded no mismatches.

The provider verifier and the conformance gate delegate to the engine's
`verify` / `compare` CLI subcommands (same Rust functions as the gRPC
`VerifyInteraction` / `CompareContents`), so there is **no TypeScript-side
matching** anywhere.

## Engine binary resolution

`resolveEngine()` looks for the `pact-mcp-plugin` binary (`.exe` on Windows) in order:
1. `PACT_MCP_ENGINE` env var,
2. `~/.pact/plugins/mcp-<version>/` (provisioned by the postinstall / `pact-mcp-install`, or a release install),
3. a local `rust/target/{release,debug}` dev build.

## Test

```sh
npm install
npm test   # consumer + provider + conformance gate (drives the Rust engine)
```

Requires `node`, and the engine binary resolvable as above.

## Not yet implemented
- In-memory `withServer(inMemoryFactory)` provider verification (needs an
  in-memory→engine bridge; the stdio/http transport paths are provided instead).
- OAuth2 auth (Phase 4). The `AuthProvider` seam is ready for it.
