//! Task A (RESOLVED): full live round trip — a pact authored by a REAL pact-js
//! V4 plugin consumer test (emitted by `examples/ts-roundtrip/generate-pact.mjs`,
//! committed verbatim as evidence at
//! `examples/ts-roundtrip/pacts-committed/weather-agent-weather-mcp.json`) is
//! verified by THIS engine against the real fixture MCP server
//! (`examples/fixtures/weather-server.mjs`), reusing the same
//! `interaction_from_value` + `response_matching_rules` + `verify_interaction_stdio`
//! path the gRPC `VerifyInteraction` RPC uses.
//!
//! This proves the persisted pact-core shape (two-part sync message; body under
//! `contents.content`; rules under `response[0].matchingRules.body` keyed
//! `$.<path>`; operation in `pluginConfiguration.mcp`) is consumed correctly —
//! closing the residual risk in ADR 0004.

use pact_mcp_plugin::server::{interaction_from_value, response_matching_rules};
use pact_mcp_plugin::verify::{verify_interaction_stdio, StdioServerConfig};
use std::collections::HashMap;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[tokio::test]
async fn engine_verifies_a_pact_authored_by_real_pact_js_against_the_fixture_server() {
    let pact_path = repo_root().join("examples/ts-roundtrip/pacts-committed/weather-agent-weather-mcp.json");
    let raw = std::fs::read_to_string(&pact_path)
        .unwrap_or_else(|e| panic!("reading committed pact-js evidence pact {}: {e}", pact_path.display()));
    let pact: serde_json::Value = serde_json::from_str(&raw).expect("valid pact json");

    // Sanity-check this really is the pact-js-emitted two-part shape, not our
    // hand-written single-fragment examples shape.
    let first = &pact["interactions"][0];
    assert!(first.pointer("/request/contents/content").is_some(), "expected pact-js two-part request body");
    assert!(
        first.pointer("/response/0/matchingRules/body").is_some(),
        "expected pact-js response[0].matchingRules.body"
    );
    assert_eq!(pact["metadata"]["pact-js"]["version"], "17.0.1", "expected a real pact-js-authored pact");

    let server = StdioServerConfig {
        command: "node".to_string(),
        args: vec![repo_root().join("examples/fixtures/weather-server.mjs").to_string_lossy().to_string()],
        env: HashMap::new(),
    };

    let interactions = pact["interactions"].as_array().expect("interactions");
    assert!(!interactions.is_empty());

    for interaction in interactions {
        let mcp = interaction_from_value(interaction).expect("reconstruct McpInteraction from real pact");
        let rules = response_matching_rules(interaction);

        let result = verify_interaction_stdio(&mcp, &server, rules.as_ref())
            .await
            .expect("verification ran without a transport error");

        assert!(
            result.is_match(),
            "expected the pact-js-authored interaction to verify against the fixture server; mismatches: {:?}",
            result.mismatches
        );
    }
}
