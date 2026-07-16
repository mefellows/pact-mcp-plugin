//! Task 1.9 — Phase 1 demo: `examples/provider-stdio` runs green end-to-end.
//!
//! Loads examples/provider-stdio/pacts/weather-agent-weather-mcp.json (a Pact
//! V4-shaped file carrying `mcp` plugin interactions per
//! docs/spec/interaction-schema.md) and verifies every interaction against the
//! REAL fixture MCP server (examples/fixtures/weather-server.mjs, a genuine
//! @modelcontextprotocol/sdk stdio server, spawned as a real subprocess) using
//! the same `verify_interaction_stdio` / `content::compare_response` machinery
//! the gRPC `VerifyInteraction` RPC uses.

use pact_mcp_plugin::mcp::model::McpInteraction;
use pact_mcp_plugin::verify::{verify_interaction_stdio, StdioServerConfig};
use std::collections::HashMap;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_server() -> StdioServerConfig {
    let server = repo_root().join("examples/fixtures/weather-server.mjs");
    assert!(server.exists(), "fixture server not found at {}", server.display());
    StdioServerConfig {
        command: "node".to_string(),
        args: vec![server.to_string_lossy().to_string()],
        env: HashMap::new(),
    }
}

fn load_pact() -> serde_json::Value {
    let path = repo_root().join("examples/provider-stdio/pacts/weather-agent-weather-mcp.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("valid pact json")
}

#[tokio::test]
async fn provider_stdio_example_verifies_every_interaction_against_the_real_fixture_server() {
    let pact = load_pact();
    let interactions = pact["interactions"].as_array().expect("interactions array");
    assert!(!interactions.is_empty());

    let server = fixture_server();
    let mut failures = Vec::new();

    for interaction_json in interactions {
        let description = interaction_json["description"].as_str().unwrap_or("<no description>").to_string();
        let mcp_value = interaction_json.pointer("/contents/mcp").expect("interaction has contents.mcp");
        let interaction: McpInteraction = serde_json::from_value(mcp_value.clone())
            .unwrap_or_else(|e| panic!("{description}: invalid mcp fragment: {e}"));

        let response_rules = interaction_json.pointer("/matchingRules/response");

        let result = verify_interaction_stdio(&interaction, &server, response_rules)
            .await
            .unwrap_or_else(|e| panic!("{description}: transport error: {e}"));

        if !result.is_match() {
            failures.push(format!("{description}: mismatches={:?}", result.mismatches));
        }
    }

    assert!(failures.is_empty(), "provider verification failures:\n{}", failures.join("\n"));
}
