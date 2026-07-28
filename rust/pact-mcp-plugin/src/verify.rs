//! Provider verification over stdio: connect to a real MCP server, replay the
//! interaction's request, and `CompareContents` the response.
//! See docs/plans/pact-mcp-plugin-implementation-plan.md §8.

use crate::auth::{AuthError, ResolvedAuth};
use crate::content::{compare_response, MatchResult, Rules};
use crate::mcp::model::McpInteraction;
use crate::transport::http::HttpClient;
use crate::transport::stdio::StdioClient;
use crate::transport::TransportError;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VerifyError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    Auth(#[from] AuthError),
}

/// Configuration for spawning the provider under test over stdio.
#[derive(Debug, Clone)]
pub struct StdioServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

/// Verify a single `mcp` interaction against a real MCP server spawned over
/// stdio: connect + handshake, perform the interaction's request, compare the
/// actual response against the expected one using the same matcher the mock
/// and the conformance harness use.
pub async fn verify_interaction_stdio(
    interaction: &McpInteraction,
    server: &StdioServerConfig,
    matching_rules_response: Option<&serde_json::Value>,
) -> Result<MatchResult, VerifyError> {
    // Provider states (ADR 0009): passed to the spawned server via env; one
    // spawn per interaction so states cannot leak across interactions.
    let mut env = server.env.clone();
    if let Some(states) = &interaction.provider_states {
        env.insert(
            "PACT_MCP_PROVIDER_STATES".to_string(),
            serde_json::to_string(states).unwrap_or_else(|_| "[]".to_string()),
        );
    }
    let client = StdioClient::connect(&server.command, &server.args, &env).await?;
    let actual = client.perform(interaction.operation, &interaction.request).await?;
    client.close().await?;

    let rules = Rules::new(matching_rules_response);
    Ok(compare_response(interaction.operation, &interaction.response, &actual, &rules))
}

/// Verify a single `mcp` interaction against a real Streamable HTTP MCP server:
/// connect + handshake (with `auth` injected on every request), perform the
/// request, compare the response. A transport/handshake failure (e.g. a 401
/// from missing/invalid auth) surfaces as `VerifyError::Transport`.
pub async fn verify_interaction_http(
    interaction: &McpInteraction,
    url: &str,
    auth: &ResolvedAuth,
    matching_rules_response: Option<&serde_json::Value>,
) -> Result<MatchResult, VerifyError> {
    let client = HttpClient::connect(url, auth).await?;
    let actual = client.perform(interaction.operation, &interaction.request).await?;
    client.close().await?;

    let rules = Rules::new(matching_rules_response);
    Ok(compare_response(interaction.operation, &interaction.response, &actual, &rules))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::model::Operation;

    fn fixture_server() -> StdioServerConfig {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture = format!("{manifest_dir}/../../examples/fixtures/weather-server.mjs");
        StdioServerConfig {
            command: "node".to_string(),
            args: vec![fixture],
            env: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn verifies_a_passing_tools_call_interaction_against_the_real_fixture_server() {
        let interaction = McpInteraction::new(
            Operation::ToolsCall,
            serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } }),
            serde_json::json!({ "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false }),
        );

        let result = verify_interaction_stdio(&interaction, &fixture_server(), None)
            .await
            .expect("verification should run without a transport error");

        assert!(result.is_match(), "expected a match, got mismatches: {:?}", result.mismatches);
    }

    #[tokio::test]
    async fn reports_a_mismatch_when_the_real_server_returns_different_content() {
        let interaction = McpInteraction::new(
            Operation::ToolsCall,
            serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } }),
            serde_json::json!({ "content": [ { "type": "text", "text": "Raining, 9C" } ], "isError": false }),
        );

        let result = verify_interaction_stdio(&interaction, &fixture_server(), None)
            .await
            .expect("verification should run without a transport error");

        assert!(!result.is_match());
        assert_eq!(result.mismatch_paths(), ["$.content[0].text".to_string()].into_iter().collect());
    }

    #[tokio::test]
    async fn seeds_provider_state_into_the_spawned_server() {
        // Hobart is unknown to the fixture unless the provider state seeds it
        // (ADR 0009: PACT_MCP_PROVIDER_STATES on the child env).
        let mut interaction = McpInteraction::new(
            Operation::ToolsCall,
            serde_json::json!({ "name": "get_weather", "arguments": { "city": "Hobart" } }),
            serde_json::json!({ "content": [ { "type": "text", "text": "Windy, 12C" } ], "isError": false }),
        );
        interaction.provider_states = Some(serde_json::json!([
            { "name": "the Hobart weather is known", "params": { "city": "Hobart", "weather": "Windy, 12C" } }
        ]));

        let result = verify_interaction_stdio(&interaction, &fixture_server(), None)
            .await
            .expect("verification should run without a transport error");

        assert!(result.is_match(), "expected state-seeded match, got: {:?}", result.mismatches);
    }

    #[tokio::test]
    async fn a_nonexistent_server_command_is_a_clear_transport_error_not_a_hang() {
        let server = StdioServerConfig {
            command: "definitely-not-a-real-command-xyz".to_string(),
            args: vec![],
            env: HashMap::new(),
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            verify_interaction_stdio(&weather_interaction_melbourne(), &server, None),
        )
        .await
        .expect("must fail fast, not hang");
        assert!(matches!(result, Err(VerifyError::Transport(_))), "got: {result:?}");
    }

    #[tokio::test]
    async fn a_server_that_exits_immediately_is_a_transport_error_not_a_hang() {
        let server = StdioServerConfig {
            command: "true".to_string(),
            args: vec![],
            env: HashMap::new(),
        };
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            verify_interaction_stdio(&weather_interaction_melbourne(), &server, None),
        )
        .await
        .expect("must fail fast, not hang");
        assert!(matches!(result, Err(VerifyError::Transport(_))), "got: {result:?}");
    }

    fn weather_interaction_melbourne() -> McpInteraction {
        McpInteraction::new(
            Operation::ToolsCall,
            serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } }),
            serde_json::json!({ "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false }),
        )
    }

    #[tokio::test]
    async fn verifies_resources_read_against_the_real_fixture_server() {
        let interaction = McpInteraction::new(
            Operation::ResourcesRead,
            serde_json::json!({ "uri": "weather://melbourne/report" }),
            serde_json::json!({ "contents": [ { "uri": "weather://melbourne/report", "text": "Sunny all week" } ] }),
        );
        let result = verify_interaction_stdio(&interaction, &fixture_server(), None)
            .await
            .expect("verification should run without a transport error");
        assert!(result.is_match(), "mismatches: {:?}", result.mismatches);
    }

    #[tokio::test]
    async fn verifies_prompts_get_against_the_real_fixture_server() {
        let interaction = McpInteraction::new(
            Operation::PromptsGet,
            serde_json::json!({ "name": "weather-report", "arguments": { "city": "Melbourne" } }),
            serde_json::json!({
                "messages": [ { "role": "user", "content": { "type": "text", "text": "Write a weather report for Melbourne" } } ]
            }),
        );
        let result = verify_interaction_stdio(&interaction, &fixture_server(), None)
            .await
            .expect("verification should run without a transport error");
        assert!(result.is_match(), "mismatches: {:?}", result.mismatches);
    }

    #[tokio::test]
    async fn verifies_resources_and_prompts_list_subsets_against_the_real_fixture_server() {
        let resources = McpInteraction::new(
            Operation::ResourcesList,
            serde_json::json!({}),
            serde_json::json!({ "resources": [ { "uri": "weather://melbourne/report" } ] }),
        );
        let prompts = McpInteraction::new(
            Operation::PromptsList,
            serde_json::json!({}),
            serde_json::json!({ "prompts": [ { "name": "weather-report" } ] }),
        );
        for interaction in [resources, prompts] {
            let result = verify_interaction_stdio(&interaction, &fixture_server(), None)
                .await
                .expect("verification should run without a transport error");
            assert!(result.is_match(), "mismatches: {:?}", result.mismatches);
        }
    }

    #[tokio::test]
    async fn reports_a_mismatch_for_a_wrong_resource_text() {
        let interaction = McpInteraction::new(
            Operation::ResourcesRead,
            serde_json::json!({ "uri": "weather://melbourne/report" }),
            serde_json::json!({ "contents": [ { "uri": "weather://melbourne/report", "text": "Raining all week" } ] }),
        );
        let result = verify_interaction_stdio(&interaction, &fixture_server(), None)
            .await
            .expect("verification should run without a transport error");
        assert!(!result.is_match());
        assert_eq!(
            result.mismatch_paths(),
            ["$.contents[0].text".to_string()].into_iter().collect()
        );
    }

    #[tokio::test]
    async fn reports_isError_mismatch_for_an_unknown_city() {
        let interaction = McpInteraction::new(
            Operation::ToolsCall,
            serde_json::json!({ "name": "get_weather", "arguments": { "city": "Atlantis" } }),
            serde_json::json!({ "content": [ { "type": "text", "text": "does not matter", "matchOnlyType": true } ], "isError": false }),
        );

        let rules = serde_json::json!({ "$.content[0].text": { "matchers": [ { "match": "type" } ] } });
        let result = verify_interaction_stdio(&interaction, &fixture_server(), Some(&rules))
            .await
            .expect("verification should run without a transport error");

        assert!(!result.is_match());
        assert_eq!(result.mismatch_paths(), ["$.isError".to_string()].into_iter().collect());
    }
}
