# Gap Analysis & Next-Phase Plan

> **Audience:** implementing engineer/agent (Sonnet).
> **Date:** 2026-07-24. **Baseline:** commit `a2e725f`; all suites green (53 Rust tests / 10 suites, 14 TS tests / 4 files).
> **Companion:** `pact-mcp-plugin-implementation-plan.md` (the master plan) — this doc records what is DONE vs that plan and prescribes the remaining work in priority order. Follow the same rules: TDD, bite-sized commits, VERIFY UPSTREAM where marked, record decisions in `docs/decisions/`.

---

## 1. Status vs the master plan

### Done (verified against code + passing tests)

| Phase | Evidence |
|---|---|
| **0 — Spec + scaffolding** | `docs/spec/` (schema, matching semantics, 6 conformance fixtures); Cargo workspace; vendored `plugin.proto`; bootstrap handshake; ADRs 0001–0005. |
| **1 — stdio vertical slice** | model/jsonrpc/handshake; `ConfigureInteraction` (two-part sync message, inline DSL — ADR 0004, incl. the `application/mcp\+json` regex-escape fix); MCP-aware `CompareContents` passing conformance; stdio transport (rmcp) vs fixture server; provider verify over stdio; stdio mock mode (`pact-mcp-plugin mock`); `examples/provider-stdio` green; **live pact-js round trip** (`tests/live_roundtrip_verify.rs`, committed evidence pact). |
| **2 — Streamable HTTP + auth** | rmcp Streamable HTTP client (session id, SSE/JSON — ADR 0007); `AuthProvider` (bearer/apiKey/headers + `${ENV}`, secrets-never-persisted test); HTTP provider verify incl. auth-protected fixture (`weather-http-server.mjs`); loopback HTTP consumer mock + `mcp-http` catalogue entry; `docs/usage.md`. |
| **3 — TS adapter (partial)** | `adapters/ts/pact-mcp`: `McpPact` (consumer, stdio + HTTP mock via engine — Option B, ADR 0006), `McpProviderVerifier` (`withServerTransport` stdio + HTTP w/ auth), matcher helpers, conformance suite driving the engine via `compare` CLI. |

Full `PactPlugin` gRPC surface is implemented in `server.rs` (all 10 methods incl. `PrepareInteractionForVerification`/`VerifyInteraction`).

### Gaps (everything below is outstanding)

**G1 — Standard-verifier E2E is unproven.** The plan (§8) requires verification to run from the *standard* Pact verifier (pact CLI / pact-js `Verifier` / JVM) once the plugin is installed. `live_roundtrip_verify.rs` verifies via the engine's own functions, not through pact-core's verifier driving our gRPC `PrepareInteractionForVerification`/`VerifyInteraction`. Those methods exist but have never been exercised by a real external verifier — and open item §16.5 (how provider transport config + provider states reach a plugin transport) was never resolved.

**G2 — Provider states are absent entirely.** No `providerStates` in the persisted fragment, no application in `verify.rs`/CLI, no `stateHandlers` in the TS adapter (the §10 API sketch shows them). Phase 3.5 requires the provider-state contract finalized + documented.

**G3 — Conformance coverage is thin.** 6 fixtures vs the matching-semantics §0 claim "every rule here has a conformance fixture". Missing fixtures for: `image` and `resource` content blocks; `structuredContent` (pass + mismatch); cross-shape mismatch (expected `content`, actual `error`, and vice-versa); `error.data` structure matching; extra-trailing-content-blocks semantics (§2.1); `tools/list` `inputSchema` structure mismatch and missing-tool mismatch; request-side matching (mock interaction selection, §4).

**G4 — TS consumer DSL covers only `tools/call`.** Single interaction per test (`toolName` is scalar); no `tools/list` authoring (engine already matches it); no multi-interaction test.

**G5 — `resources/*` + `prompts/*` not implemented** (schema §3.3 reserved only). Phase 3.5 scope: model, matching defaults, mock, verify, fixtures, adapter DX.

**G6 — No CI, no packaging, no release path.** No `.github/`, no `scripts/` (plan §12: install.sh, cross-platform archives, `pact-plugin-cli install` compat). `README.md` and `LICENSE` are **untracked** — commit them first.

**G7 — PactFlow/BDCT walkthrough missing** (Phase 3.5: publish + `can-i-deploy` — the strategic moat framing of §0/§13).

**G8 — Minor hardening.** `handshake.rs` is 14 lines — confirm capability assertion ("server must advertise `tools`") is actually enforced pre-verification with a clear failure; §14 negative tests for process crash/timeout on transports; ambiguous-match warning (matching-semantics §4) coverage.

Deliberately deferred (unchanged, do NOT build): OAuth2 (Phase 4), true in-memory linked pair (ADR 0006 trade-off), Python/Go adapters, Java loopback example, deprecated HTTP+SSE transport.

---

## 2. Work plan (priority order)

Each task: failing test first → implement → commit. Do not reorder P1.

### P1 — Prove the core promise

- [x] **1.1 Standard-verifier E2E (G1).** Install the plugin locally (`~/.pact/plugins/mcp-0.1.0/` = binary + `pact-plugin.json`), then drive verification of `examples/*/pacts/*.json` through **pact-js `VerifierV3`** (and/or `pact_verifier_cli`). VERIFY UPSTREAM how the standard verifier supplies plugin-transport config (stdio command/args, HTTP url+auth) to `VerifyInteraction` — resolve open item §16.5, record an ADR (0008). Fix `server.rs` verify path as reality dictates. Add as an integration test + document in `usage.md`.
- [x] **1.2 Provider states (G2).** Design the state contract (ADR 0009): persist `providerStates` per interaction (standard Pact field); delivery = env var / `--state` arg for stdio spawn, HTTP state-change POST for http, `stateHandlers` callbacks in the TS adapter (in-process). Implement engine + CLI + adapter + fixture server support; example with a seeded state.
- [x] **1.3 Conformance completion (G3).** Add the ~10 missing fixtures listed above; make engine + TS suites pass them (both glob the directory, so fixtures auto-run). Where engine behavior is currently undefined (e.g. cross-shape, trailing blocks), the fixture defines it — fix the engine to conform. Update the README index table.

### P2 — Complete the authoring surface

- [x] **2.1 TS DSL (G4):** `expectsToolsList([...])` (or similar) + multiple interactions per `McpPact` test; verify multi-interaction pacts mock + verify correctly end-to-end.
- [x] **2.2 `resources/read` + `prompts/get` (+ their lists) (G5):** extend interaction-schema §3.3 + matching-semantics (mirror the tools defaults table), model/enum, mock handlers, verify, fixtures, adapter DX, fixture-server support. Same vertical-slice discipline as Phase 1.

### P3 — Make it shippable

- [x] **3.1 Commit `README.md` + `LICENSE`** (currently untracked). Review README for accuracy against current state first.
- [x] **3.2 CI (G6):** GitHub Actions — Rust build+test (linux/macos/windows), TS adapter test, conformance as an explicit named gate. Cache cargo + npm.
- [x] **3.3 Release packaging (G6):** `scripts/install.sh` + release workflow producing per-platform archives (binary + manifest, checksums) laid out for `pact-plugin-cli install` (VERIFY UPSTREAM its expected asset naming) and manual install.
- [x] **3.4 BDCT walkthrough (G7):** example/doc publishing the consumer pact to PactFlow + `can-i-deploy`; ties to the §13 Drift-MCP positioning.
- [x] **3.5 Hardening (G8):** capability-assertion failure test; transport crash/timeout negative tests; docs pass over `usage.md` (add stdio quickstart + provider states; currently HTTP/auth-only).

### P4 — Unchanged deferrals
OAuth2 dynamic client registration; in-memory linked pair; Python/Go adapters; Java loopback example.

---

## 3. Definition of done for this phase

1. An MCP pact authored by the TS adapter verifies via the **stock pact-js verifier** with the plugin installed — no bespoke runner (G1).
2. A stateful interaction round-trips: `given(...)` in the consumer test → state applied at the provider → green verify (G2).
3. Every row of the matching-semantics §6 defaults table has at least one conformance fixture, and both suites pass them (G3).
4. `tools/list`, `resources/read`, `prompts/get` all authorable, mockable, verifiable (G4/G5).
5. `main` is green in CI; a tagged release produces installable per-platform artifacts (G6).
