//! `ConfigureInteraction` / `CompareContents` / `GenerateContent` glue between
//! the proto wire shapes and our internal `mcp::model` + `content` types.
//!
//! NOTE on matching-rule path convention (best-effort, unverified against a
//! real pact-js/pact-jvm consumer round trip in this task): the vendored
//! `InteractionResponse.rules` map is `map<string, MatchingRules>` — a flat
//! map, not the nested `{request:{...}, response:{...}}` shape our own spec
//! (docs/spec/interaction-schema.md §2) uses for the *persisted pact file*.
//! We assume pact core namespaces plugin-returned rule keys under
//! `$.request.*` / `$.response.*` and persists a `matchingRules.request` /
//! `matchingRules.response` map from that, mirroring the path convention the
//! conformance fixtures use once rooted at `request`/`response`. This has
//! **not** been verified against real pact-js/pact-jvm output — see
//! docs/decisions/0004-configure-interaction-assumptions.md.

use crate::mcp::model::{McpFragment, McpInteraction, Operation};
use crate::proto::{Body, MatchingRule, MatchingRules};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug)]
pub struct ConfiguredInteraction {
    pub fragment: McpFragment,
    pub body_bytes: Vec<u8>,
    pub rules: HashMap<String, MatchingRules>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigureError {
    #[error("missing required field `{0}` in contentsConfig")]
    MissingField(&'static str),
    #[error("unknown mcp operation `{0}`")]
    UnknownOperation(String),
}

/// Build the persisted MCP interaction fragment + wire body + matching rules
/// from a consumer-authored `contentsConfig` (task 1.3).
///
/// Expected `contentsConfig` shape (a pragmatic MVP shortcut — see module docs):
/// ```jsonc
/// {
///   "operation": "tools/call",
///   "request": { "name": "get_weather", "arguments": { "city": "Melbourne" } },
///   "response": { "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false },
///   "matchingRules": { "request": {}, "response": { "$.content[0].text": { "matchers": [...] } } },
///   "server": { "transport": "stdio" }
/// }
/// ```
pub fn configure_interaction(contents_config: &Value) -> Result<ConfiguredInteraction, ConfigureError> {
    let operation_str = contents_config
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ConfigureError::MissingField("operation"))?;
    let operation = Operation::parse(operation_str)
        .ok_or_else(|| ConfigureError::UnknownOperation(operation_str.to_string()))?;

    let request = contents_config
        .get("request")
        .cloned()
        .ok_or(ConfigureError::MissingField("request"))?;
    let response = contents_config
        .get("response")
        .cloned()
        .ok_or(ConfigureError::MissingField("response"))?;

    let mut interaction = McpInteraction::new(operation, request, response);
    if let Some(server) = contents_config.get("server") {
        if let Some(transport) = server.get("transport").and_then(Value::as_str) {
            interaction.server = Some(crate::mcp::model::ServerHint { transport: transport.to_string() });
        }
    }

    let fragment = McpFragment::new(interaction);
    let body_bytes = serde_json::to_vec(&fragment).expect("McpFragment always serializes");

    let mut rules = HashMap::new();
    if let Some(request_rules) = contents_config.pointer("/matchingRules/request").and_then(Value::as_object) {
        for (path, rule) in request_rules {
            rules.insert(format!("$.request.{}", strip_root(path)), to_proto_rules(rule));
        }
    }
    if let Some(response_rules) = contents_config.pointer("/matchingRules/response").and_then(Value::as_object) {
        for (path, rule) in response_rules {
            rules.insert(format!("$.response.{}", strip_root(path)), to_proto_rules(rule));
        }
    }

    Ok(ConfiguredInteraction { fragment, body_bytes, rules })
}

fn strip_root(path: &str) -> &str {
    path.strip_prefix("$.").unwrap_or_else(|| path.strip_prefix('$').unwrap_or(path))
}

fn to_proto_rules(rule: &Value) -> MatchingRules {
    let mut out = Vec::new();
    if let Some(matchers) = rule.get("matchers").and_then(Value::as_array) {
        for matcher in matchers {
            if let Some(match_type) = matcher.get("match").and_then(Value::as_str) {
                out.push(MatchingRule {
                    r#type: match_type.to_string(),
                    values: None,
                });
            }
        }
    }
    MatchingRules { rule: out }
}

/// Reconstruct our internal `Rules` JSON shape (`{"<path>": {"matchers":[{"match":"..."}]}}`)
/// from the proto `CompareContentsRequest.rules` map, filtered to a single root
/// (`request` or `response`) and re-rooted at `$.` for `content::compare_response`.
pub fn rules_value_for_root(proto_rules: &HashMap<String, MatchingRules>, root: &str) -> Value {
    let prefix = format!("$.{root}.");
    let mut obj = serde_json::Map::new();
    for (path, rules) in proto_rules {
        if let Some(rest) = path.strip_prefix(&prefix) {
            let matchers: Vec<Value> = rules
                .rule
                .iter()
                .map(|r| serde_json::json!({ "match": r.r#type }))
                .collect();
            obj.insert(format!("$.{rest}"), serde_json::json!({ "matchers": matchers }));
        }
    }
    Value::Object(obj)
}

pub fn body_content_type(body: &Body) -> &str {
    &body.content_type
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configures_a_tools_call_interaction_and_round_trips_through_our_own_matcher() {
        let contents_config = serde_json::json!({
            "operation": "tools/call",
            "request": { "name": "get_weather", "arguments": { "city": "Melbourne" } },
            "response": { "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false },
            "matchingRules": { "response": { "$.content[0].text": { "matchers": [ { "match": "type" } ] } } }
        });

        let configured = configure_interaction(&contents_config).expect("valid config");
        assert_eq!(configured.fragment.mcp.operation, Operation::ToolsCall);
        assert!(configured.rules.contains_key("$.response.content[0].text"));

        let response_rules_value = rules_value_for_root(&configured.rules, "response");
        assert_eq!(
            response_rules_value,
            serde_json::json!({ "$.content[0].text": { "matchers": [ { "match": "type" } ] } })
        );
    }

    #[test]
    fn rejects_an_unknown_operation() {
        let contents_config = serde_json::json!({
            "operation": "prompts/summon",
            "request": {},
            "response": {}
        });
        let err = configure_interaction(&contents_config).unwrap_err();
        assert!(matches!(err, ConfigureError::UnknownOperation(_)));
    }
}
