# PactFlow walkthrough — publish, verify, can-i-deploy

How MCP contracts flow through PactFlow (or any Pact Broker). This is the
CDCT half of the strategic triangle (plan §0/§13); the provider-side schema
contract (Drift-MCP) is a separate track.

## 1. Publish the consumer pact

The adapter's `executeTest` writes `./pacts/<consumer>-<provider>.json`.
Publish it with the standard broker CLI:

```sh
pact-broker publish ./pacts \
  --broker-base-url "$PACT_BROKER_BASE_URL" \
  --broker-token "$PACT_BROKER_TOKEN" \
  --consumer-app-version "$(git rev-parse --short HEAD)" \
  --branch "$(git branch --show-current)"
```

## 2. Verify + publish results from the provider build

With the plugin installed (`scripts/install-plugin.sh`), the stock pact-js
`Verifier` fetches pacts from the broker, verifies over the interaction's
transport (ADR 0008), and publishes results:

```sh
export PACT_MCP_SERVER_COMMAND=node PACT_MCP_SERVER_ARGS=dist/server.js  # stdio providers
```

```ts
await new Verifier({
  provider: "weather-mcp",
  providerBaseUrl: "http://127.0.0.1:65500",
  pactBrokerUrl: process.env.PACT_BROKER_BASE_URL,
  pactBrokerToken: process.env.PACT_BROKER_TOKEN,
  consumerVersionSelectors: [{ mainBranch: true }, { deployedOrReleased: true }],
  publishVerificationResult: true,
  providerVersion: process.env.GIT_SHA,
  providerVersionBranch: process.env.GIT_BRANCH,
}).verifyProvider();
```

For HTTP providers use `transports: [{ protocol: "mcp-http", port }]` and
`PACT_MCP_AUTH` (see `usage.md`).

## 3. Gate deploys

```sh
pact-broker can-i-deploy \
  --pacticipant weather-agent --version "$GIT_SHA" \
  --to-environment production \
  --broker-base-url "$PACT_BROKER_BASE_URL" --broker-token "$PACT_BROKER_TOKEN"

# after deploying:
pact-broker record-deployment --pacticipant weather-agent --version "$GIT_SHA" \
  --environment production
```

`can-i-deploy` answers *"can this agent safely deploy against the MCP servers
it depends on?"* from the verification matrix — the question schema-only
testing cannot answer.
