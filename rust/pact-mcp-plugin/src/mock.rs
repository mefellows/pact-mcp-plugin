//! stdio mock mode (plan task 1.8).
//!
//! `pact-mcp-plugin mock --pact <file> [--results <file>]` runs a REAL MCP
//! server over this process's own stdio, synthesized from the interactions in a
//! pact file. A real MCP client (e.g. `@modelcontextprotocol/sdk`) spawns it as
//! its stdio server, does the `initialize` handshake, lists tools, and calls
//! them. Incoming `tools/call`s are matched (by name + arguments, reusing the
//! engine matcher) against the configured interactions; a match returns the
//! configured response, a miss is recorded as a mismatch. On shutdown the
//! results are flushed to the `--results` file so `GetMockServerResults` /
//! `ShutdownMockServer` can report them (§7.2).

use crate::content::{match_tools_call_request, Rules};
use crate::mcp::model::{McpInteraction, Operation};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ErrorData, Implementation, InitializeResult,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// A single matched-or-mismatched request the mock received.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MockResult {
    /// The MCP method, e.g. "tools/call".
    pub path: String,
    /// Error message if the request could not be handled / matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Mismatch paths (for a partial-match failure), if any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mismatches: Vec<String>,
}

/// A configured interaction plus its request-side matching rules (used to
/// decide which interaction an incoming `tools/call` matches).
struct MockInteraction {
    mcp: McpInteraction,
    /// `{"$.<path>": {"matchers":[{"match":"type"}]}}` rooted at the request body.
    request_rules: Option<Value>,
}

/// The parsed interactions + a shared results sink.
#[derive(Clone)]
pub struct MockServer {
    interactions: Arc<Vec<MockInteraction>>,
    results: Arc<Mutex<Vec<MockResult>>>,
    /// If set, results are flushed to this file after EVERY recorded request, so
    /// a consumer (e.g. the TS adapter) can read them reliably without waiting
    /// for the mock process to exit.
    results_path: Arc<Mutex<Option<std::path::PathBuf>>>,
}

impl MockServer {
    pub fn new(interactions: Vec<McpInteraction>) -> Self {
        Self {
            interactions: Arc::new(
                interactions
                    .into_iter()
                    .map(|mcp| MockInteraction { mcp, request_rules: None })
                    .collect(),
            ),
            results: Arc::new(Mutex::new(Vec::new())),
            results_path: Arc::new(Mutex::new(None)),
        }
    }

    /// Flush results to `path` after each recorded request (live).
    pub fn with_live_results_path(self, path: impl Into<std::path::PathBuf>) -> Self {
        *self.results_path.lock().unwrap() = Some(path.into());
        self
    }

    pub fn results_handle(&self) -> Arc<Mutex<Vec<MockResult>>> {
        Arc::clone(&self.results)
    }

    /// Load interactions from a pact-as-JSON document, keeping only the `mcp`
    /// plugin interactions (tolerates the single-fragment and two-part shapes)
    /// AND their request-side matching rules (real
    /// `request.matchingRules.body`, so authored `matching(...)` matchers on
    /// request arguments are honored — matching-semantics §4).
    pub fn from_pact_json(pact_json: &str) -> anyhow::Result<Self> {
        let pact: Value = serde_json::from_str(pact_json)?;
        let interactions = pact
            .get("interactions")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("pact has no interactions array"))?;

        let mut parsed = Vec::new();
        for interaction in interactions {
            if let Ok(mcp) = crate::server::interaction_from_value(interaction) {
                let request_rules = crate::server::request_matching_rules(interaction);
                parsed.push(MockInteraction { mcp, request_rules });
            }
        }
        if parsed.is_empty() {
            anyhow::bail!("no mcp interactions found in pact");
        }
        Ok(Self {
            interactions: Arc::new(parsed),
            results: Arc::new(Mutex::new(Vec::new())),
            results_path: Arc::new(Mutex::new(None)),
        })
    }

    fn record(&self, result: MockResult) {
        if let Ok(mut guard) = self.results.lock() {
            guard.push(result);
            // Flush live if a path is configured, so consumers don't race the
            // process exit to read results.
            if let Ok(path_guard) = self.results_path.lock() {
                if let Some(path) = path_guard.as_ref() {
                    let _ = write_results(path.to_string_lossy().as_ref(), &guard);
                }
            }
        }
    }

    /// The tools advertised via `tools/list`: any `tools/list` interaction's
    /// tools, plus a synthesized minimal tool for each distinct `tools/call`
    /// name the pact expects.
    fn advertised_tools(&self) -> Vec<Tool> {
        let mut tools: Vec<Tool> = Vec::new();
        let mut seen: Vec<String> = Vec::new();

        for entry in self.interactions.iter() {
            let interaction = &entry.mcp;
            match interaction.operation {
                Operation::ToolsList => {
                    if let Some(list) = interaction.response.get("tools").and_then(Value::as_array) {
                        for tool in list {
                            if let Ok(t) = serde_json::from_value::<Tool>(tool.clone()) {
                                if !seen.contains(&t.name.to_string()) {
                                    seen.push(t.name.to_string());
                                    tools.push(t);
                                }
                            }
                        }
                    }
                }
                Operation::ToolsCall => {
                    if let Some(name) = interaction.request.get("name").and_then(Value::as_str) {
                        if !seen.contains(&name.to_string()) {
                            seen.push(name.to_string());
                            let schema = serde_json::json!({ "type": "object" });
                            let schema_obj = schema.as_object().cloned().unwrap_or_default();
                            tools.push(Tool::new(
                                name.to_string(),
                                "Synthesized by the pact mock",
                                std::sync::Arc::new(schema_obj),
                            ));
                        }
                    }
                }
            }
        }
        tools
    }

    /// Select the `tools/call` interaction matching an incoming call and return
    /// its configured response as a `CallToolResult`, or an error describing the
    /// mismatch. Records the outcome either way.
    fn handle_call(&self, params: &CallToolRequestParams) -> Result<CallToolResult, ErrorData> {
        let incoming = serde_json::json!({
            "name": params.name,
            "arguments": params.arguments.clone().unwrap_or_default(),
        });

        let mut best_mismatch: Option<Vec<String>> = None;

        for entry in self.interactions.iter() {
            let interaction = &entry.mcp;
            if interaction.operation != Operation::ToolsCall {
                continue;
            }
            // Honor any request-side matchers authored in the pact (e.g.
            // `matching(type, ...)` on an argument), so a call whose arguments
            // differ but satisfy the matcher still selects this interaction.
            let rules = Rules::new(entry.request_rules.as_ref());
            let result = match_tools_call_request(&interaction.request, &incoming, &rules);
            if result.is_match() {
                // Matched — return the configured response.
                self.record(MockResult {
                    path: "tools/call".to_string(),
                    error: None,
                    mismatches: vec![],
                });
                return response_to_call_result(&interaction.response);
            }
            // Track the closest miss for reporting (name-level misses excepted).
            let paths: Vec<String> = result.mismatch_paths().into_iter().collect();
            if best_mismatch.is_none() || paths.iter().any(|p| p != "$.name") {
                best_mismatch = Some(paths);
            }
        }

        // No interaction matched — record a mismatch and return a protocol error.
        let mismatches = best_mismatch.unwrap_or_default();
        self.record(MockResult {
            path: "tools/call".to_string(),
            error: Some(format!(
                "unexpected tools/call: no configured interaction matches tool {:?}",
                params.name
            )),
            mismatches: mismatches.clone(),
        });
        Err(ErrorData::invalid_params(
            format!("no matching interaction for tools/call {:?}", params.name),
            Some(serde_json::json!({ "mismatches": mismatches })),
        ))
    }
}

fn response_to_call_result(response: &Value) -> Result<CallToolResult, ErrorData> {
    serde_json::from_value::<CallToolResult>(response.clone())
        .map_err(|e| ErrorData::internal_error(format!("configured response is not a valid tools/call result: {e}"), None))
}

impl ServerHandler for MockServer {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_06_18)
            .with_server_info(Implementation::new(
                "pact-mcp-mock",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions("Pact MCP mock server (synthesized from a pact file)")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.record(MockResult { path: "tools/list".to_string(), error: None, mismatches: vec![] });
        Ok(ListToolsResult {
            tools: self.advertised_tools(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.handle_call(&request)
    }
}

/// Serialize collected results to a file (JSON array of `MockResult`).
pub fn write_results(path: &str, results: &[MockResult]) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(results)?;
    std::fs::write(path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pact_json() -> String {
        serde_json::json!({
            "interactions": [
                {
                    "description": "melbourne weather",
                    "contents": { "mcp": {
                        "schemaVersion": "0",
                        "operation": "tools/call",
                        "request": { "name": "get_weather", "arguments": { "city": "Melbourne" } },
                        "response": { "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false }
                    }}
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn loads_interactions_and_advertises_the_tool() {
        let mock = MockServer::from_pact_json(&pact_json()).unwrap();
        let tools = mock.advertised_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "get_weather");
    }

    #[test]
    fn matching_call_returns_configured_response_and_records_success() {
        let mock = MockServer::from_pact_json(&pact_json()).unwrap();
        let params = CallToolRequestParams::new("get_weather")
            .with_arguments(serde_json::json!({ "city": "Melbourne" }).as_object().unwrap().clone());
        let result = mock.handle_call(&params).expect("should match");
        assert_eq!(result.content[0].as_text().unwrap().text, "Sunny, 22C");
        let results = mock.results_handle();
        let guard = results.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert!(guard[0].error.is_none());
    }

    #[test]
    fn unexpected_call_is_recorded_as_a_mismatch() {
        let mock = MockServer::from_pact_json(&pact_json()).unwrap();
        let params = CallToolRequestParams::new("get_weather")
            .with_arguments(serde_json::json!({ "city": "Atlantis" }).as_object().unwrap().clone());
        let err = mock.handle_call(&params).expect_err("should not match");
        assert!(err.message.contains("no matching interaction"));
        let results = mock.results_handle();
        let guard = results.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert!(guard[0].error.is_some());
        assert!(guard[0].mismatches.contains(&"$.arguments.city".to_string()));
    }

    /// A pact in the real two-part shape carrying a request-side `type` matcher
    /// on the `city` argument (`request.matchingRules.body`).
    fn pact_json_with_request_matcher() -> String {
        serde_json::json!({
            "interactions": [
                {
                    "description": "weather for any city",
                    "type": "Synchronous/Messages",
                    "pluginConfiguration": { "mcp": { "operation": "tools/call" } },
                    "request": {
                        "contents": { "content": { "name": "get_weather", "arguments": { "city": "Melbourne" } } },
                        "matchingRules": { "body": { "$.arguments.city": { "combine": "AND", "matchers": [ { "match": "type" } ] } } }
                    },
                    "response": [
                        { "contents": { "content": { "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false } } }
                    ]
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn request_side_type_matcher_selects_the_interaction_for_a_different_argument() {
        let mock = MockServer::from_pact_json(&pact_json_with_request_matcher()).unwrap();

        // A call with a DIFFERENT city still matches, because the request arg
        // carries a `type` matcher (any string) — not exact equality.
        let params = CallToolRequestParams::new("get_weather")
            .with_arguments(serde_json::json!({ "city": "Reykjavik" }).as_object().unwrap().clone());
        let result = mock.handle_call(&params).expect("type matcher should accept any city");
        assert_eq!(result.content[0].as_text().unwrap().text, "Sunny, 22C");

        // But a wrong TYPE (number instead of string) must NOT match.
        let bad = CallToolRequestParams::new("get_weather")
            .with_arguments(serde_json::json!({ "city": 42 }).as_object().unwrap().clone());
        let err = mock.handle_call(&bad).expect_err("a number should not satisfy a string type matcher");
        assert!(err.message.contains("no matching interaction"));
    }
}
