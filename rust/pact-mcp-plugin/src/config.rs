//! `ConfigureInteraction` glue: turn a consumer-authored `contentsConfig`
//! (the JSON a pact-js `.withPluginContents(json, "application/mcp+json")` call
//! passes through) into the persisted MCP interaction fragment + wire body +
//! matching rules + generators.
//!
//! ## Authoring convention (VERIFIED against pact-protobuf-plugin)
//! Matchers are authored **inline** as Pact matcher-definition DSL strings in
//! the leaf values of `request`/`response`, exactly like pact-protobuf-plugin —
//! e.g. `"text": "matching(type, 'Sunny, 22C')"`, `"name": "notEmpty('x')"`,
//! `"lat": "matching(number, 42)"`. There is NO separate `matchingRules` block
//! and NO `{"pact:matcher:type": ...}` marker object. We parse these with the
//! SAME `pact_models::matchingrules::expressions::{is_matcher_def,
//! parse_matcher_def}` functions pact-protobuf-plugin uses (see ADR 0004 for
//! the research trail), so the input contract matches the real ecosystem.
//!
//! Each matcher leaf is (a) recorded as a rule at its JSON-path `DocPath`, and
//! (b) replaced by its example value in the persisted body — again mirroring
//! pact-protobuf-plugin's `build_proto_value`.
//!
//! ## Rule-key namespacing (best-effort; see residual risk in ADR 0004)
//! Returned rule keys are `$.request.<path>` / `$.response.<path>` so our engine
//! can split them by part and match per docs/spec/interaction-schema.md §2. The
//! EXACT category pact core persists plugin sync-message rules under (our
//! `request`/`response` roots vs a standard `body` category + interaction part)
//! was not confirmed by a live pact-js FFI run in this task — flagged in
//! ADR 0004.

use crate::mcp::model::{McpFragment, McpInteraction, Operation};
use crate::proto::{Body, Generator, MatchingRule, MatchingRules};
use itertools::Either;
use pact_models::matchingrules::expressions::{is_matcher_def, parse_matcher_def, MatchingRuleDefinition, ValueType};
use pact_models::matchingrules::{MatchingRuleCategory, RuleLogic};
use pact_models::path_exp::DocPath;
use serde_json::Value;
use std::collections::HashMap;

/// One part (request or response) of a configured synchronous-message
/// interaction, as pact core requires two `InteractionResponse`s per plugin
/// sync message (VERIFIED via the live pact-js round trip — see ADR 0004).
#[derive(Debug)]
pub struct ConfiguredPart {
    /// The stripped (example-value) body for this part, serialized as JSON.
    pub body_bytes: Vec<u8>,
    /// The stripped body as a `Value` (for our own fragment reconstruction/tests).
    pub body: serde_json::Value,
    /// Matching rules keyed by `$.<path>` rooted at THIS part's body.
    pub rules: HashMap<String, MatchingRules>,
    /// Generators keyed by `$.<path>` rooted at THIS part's body.
    pub generators: HashMap<String, Generator>,
}

#[derive(Debug)]
pub struct ConfiguredInteraction {
    pub operation: Operation,
    pub server: Option<crate::mcp::model::ServerHint>,
    pub request: ConfiguredPart,
    pub response: ConfiguredPart,
    /// The full single-fragment view (both parts merged) — used by our own
    /// engine tests / the internal round-trip. NOT what pact core persists.
    pub fragment: McpFragment,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigureError {
    #[error("missing required field `{0}` in contentsConfig")]
    MissingField(&'static str),
    #[error("unknown mcp operation `{0}`")]
    UnknownOperation(String),
    #[error("invalid matcher definition: {0}")]
    Matcher(String),
}

/// Build the persisted MCP interaction fragment + wire body + matching rules
/// from a consumer-authored `contentsConfig` (task 1.3, corrected in this run).
///
/// Accepts either the wrapped form (`{ "pact:content-type": "...", "mcp": {...} }`,
/// the full withPluginContents JSON) or a bare `mcp`-object form
/// (`{ operation, request, response, server }`).
pub fn configure_interaction(contents_config: &Value) -> Result<ConfiguredInteraction, ConfigureError> {
    // Unwrap the `mcp` object if the full fragment was passed.
    let mcp = contents_config.get("mcp").unwrap_or(contents_config);

    let operation_str = mcp
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ConfigureError::MissingField("operation"))?;
    let operation = Operation::parse(operation_str)
        .ok_or_else(|| ConfigureError::UnknownOperation(operation_str.to_string()))?;

    let raw_request = mcp.get("request").cloned().ok_or(ConfigureError::MissingField("request"))?;
    let raw_response = mcp.get("response").cloned().ok_or(ConfigureError::MissingField("response"))?;

    // Each part's rules/generators are rooted at `$` of that part's own body
    // (pact core convention: the request part body IS the request message,
    // rules keyed `$.<field>`), collected into separate maps.
    let request = build_part(&raw_request)?;
    let response = build_part(&raw_response)?;

    let server = mcp
        .get("server")
        .and_then(|s| s.get("transport"))
        .and_then(Value::as_str)
        .map(|t| crate::mcp::model::ServerHint { transport: t.to_string() });

    let mut interaction = McpInteraction::new(operation, request.body.clone(), response.body.clone());
    interaction.server = server.clone();
    let fragment = McpFragment::new(interaction);

    Ok(ConfiguredInteraction { operation, server, request, response, fragment })
}

fn build_part(raw: &Value) -> Result<ConfiguredPart, ConfigureError> {
    let mut rules: HashMap<String, MatchingRules> = HashMap::new();
    let mut generators: HashMap<String, Generator> = HashMap::new();
    let root = DocPath::root();
    let body = walk(raw, &root, &mut rules, &mut generators)?;
    let body_bytes = serde_json::to_vec(&body).expect("part body always serializes");
    Ok(ConfiguredPart { body_bytes, body, rules, generators })
}

/// Recursively walk a request/response JSON tree. Any string leaf that is a
/// matcher-definition DSL is (a) recorded as a rule/generator keyed by its
/// `$.<path>` DocPath (rooted at the part body) and (b) replaced by its example
/// value in the returned (stripped) JSON. Non-matcher values pass through.
fn walk(
    value: &Value,
    path: &DocPath,
    rules: &mut HashMap<String, MatchingRules>,
    generators: &mut HashMap<String, Generator>,
) -> Result<Value, ConfigureError> {
    match value {
        Value::String(s) if is_matcher_def(s) => {
            let mrd = parse_matcher_def(s).map_err(|e| ConfigureError::Matcher(e.to_string()))?;
            record_matcher(&mrd, path, rules, generators)?;
            Ok(example_value(&mrd))
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, child) in map {
                let child_path = path.join(key);
                out.insert(key.clone(), walk(child, &child_path, rules, generators)?);
            }
            Ok(Value::Object(out))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, child) in items.iter().enumerate() {
                let child_path = path.join_index(i);
                out.push(walk(child, &child_path, rules, generators)?);
            }
            Ok(Value::Array(out))
        }
        other => Ok(other.clone()),
    }
}

fn record_matcher(
    mrd: &MatchingRuleDefinition,
    path: &DocPath,
    rules: &mut HashMap<String, MatchingRules>,
    generators: &mut HashMap<String, Generator>,
) -> Result<(), ConfigureError> {
    let key = path.to_string();
    let mut proto_rules = Vec::new();
    for rule in &mrd.rules {
        match rule {
            Either::Left(rule) => {
                let json = rule.to_json();
                proto_rules.push(matching_rule_to_proto(&json));
            }
            Either::Right(reference) => {
                return Err(ConfigureError::Matcher(format!(
                    "matching references are not supported yet: {reference:?}"
                )));
            }
        }
    }
    if !proto_rules.is_empty() {
        rules.insert(key.clone(), MatchingRules { rule: proto_rules });
    }
    if let Some(generator) = &mrd.generator {
        if let Some(json) = generator.to_json() {
            generators.insert(key, generator_to_proto(&json));
        }
    }
    Ok(())
}

/// Convert a `pact_models` matching-rule JSON (`{"match":"type", ...extra}`)
/// into the proto `MatchingRule { type, values }`.
fn matching_rule_to_proto(json: &Value) -> MatchingRule {
    let match_type = json.get("match").and_then(Value::as_str).unwrap_or("equality").to_string();
    let mut extra = json.clone();
    if let Some(obj) = extra.as_object_mut() {
        obj.remove("match");
    }
    MatchingRule {
        r#type: match_type,
        values: json_to_prost_struct(&extra),
    }
}

fn generator_to_proto(json: &Value) -> Generator {
    let gen_type = json.get("type").and_then(Value::as_str).unwrap_or("").to_string();
    let mut extra = json.clone();
    if let Some(obj) = extra.as_object_mut() {
        obj.remove("type");
    }
    Generator {
        r#type: gen_type,
        values: json_to_prost_struct(&extra),
    }
}

fn json_to_prost_struct(value: &Value) -> Option<prost_types::Struct> {
    match value {
        Value::Object(map) if !map.is_empty() => Some(prost_types::Struct {
            fields: map.iter().map(|(k, v)| (k.clone(), json_to_prost_value(v))).collect(),
        }),
        _ => None,
    }
}

fn json_to_prost_value(v: &Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match v {
        Value::Null => Kind::NullValue(0),
        Value::Bool(b) => Kind::BoolValue(*b),
        Value::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => Kind::StringValue(s.clone()),
        Value::Array(a) => Kind::ListValue(prost_types::ListValue {
            values: a.iter().map(json_to_prost_value).collect(),
        }),
        Value::Object(_) => Kind::StructValue(json_to_prost_struct(v).unwrap_or_default()),
    };
    prost_types::Value { kind: Some(kind) }
}

/// Produce the example value for a matcher, respecting its declared value type
/// (mirrors pact-protobuf-plugin's `build_proto_value` value_type handling).
fn example_value(mrd: &MatchingRuleDefinition) -> Value {
    match mrd.value_type {
        ValueType::Number | ValueType::Decimal => mrd
            .value
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(mrd.value.clone())),
        ValueType::Integer => mrd
            .value
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(mrd.value.clone())),
        ValueType::Boolean => mrd
            .value
            .parse::<bool>()
            .map(Value::Bool)
            .unwrap_or_else(|_| Value::String(mrd.value.clone())),
        ValueType::Unknown | ValueType::String => Value::String(mrd.value.clone()),
    }
}

/// Reconstruct our internal `Rules` JSON shape (`{"<path>": {"matchers":[{"match":"..."}]}}`)
/// from a part's proto rules map (already keyed `$.<path>`) for
/// `content::compare_response` / `match_tools_call_request`.
pub fn rules_value(proto_rules: &HashMap<String, MatchingRules>) -> Value {
    let mut obj = serde_json::Map::new();
    for (path, rules) in proto_rules {
        let matchers: Vec<Value> = rules
            .rule
            .iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                m.insert("match".to_string(), Value::String(r.r#type.clone()));
                Value::Object(m)
            })
            .collect();
        obj.insert(path.clone(), serde_json::json!({ "matchers": matchers }));
    }
    Value::Object(obj)
}

/// Render exactly what pact core would persist under a part's
/// `matchingRules.body`, using pact_models' own serialization — for the
/// round-trip validation test / ADR 0004 evidence.
pub fn persisted_body_category(proto_rules: &HashMap<String, MatchingRules>) -> Value {
    let mut category = MatchingRuleCategory::empty("body");
    for (path, rules) in proto_rules {
        if let Ok(doc) = DocPath::new(path.clone()) {
            for rule in &rules.rule {
                let mr = match rule.r#type.as_str() {
                    "type" => pact_models::matchingrules::MatchingRule::Type,
                    "number" => pact_models::matchingrules::MatchingRule::Number,
                    "integer" => pact_models::matchingrules::MatchingRule::Integer,
                    "boolean" => pact_models::matchingrules::MatchingRule::Boolean,
                    "equality" => pact_models::matchingrules::MatchingRule::Equality,
                    "not-empty" | "notEmpty" => pact_models::matchingrules::MatchingRule::NotEmpty,
                    _ => pact_models::matchingrules::MatchingRule::Type,
                };
                category.add_rule(doc.clone(), mr, RuleLogic::And);
            }
        }
    }
    category.to_v3_json()
}

pub fn body_content_type(body: &Body) -> &str {
    &body.content_type
}

/// Build the `interaction_configuration` Struct persisted per part in the
/// pact's `pluginConfiguration` — carries the `operation` and optional `server`
/// hint that are NOT part of the per-part body but are needed to reconstruct
/// the `McpInteraction` at verification time.
pub fn interaction_config_struct(
    operation: Operation,
    server: &Option<crate::mcp::model::ServerHint>,
) -> prost_types::Struct {
    let mut obj = serde_json::Map::new();
    obj.insert("operation".to_string(), Value::String(operation.method().to_string()));
    if let Some(server) = server {
        obj.insert("server".to_string(), serde_json::json!({ "transport": server.transport }));
    }
    json_to_prost_struct(&Value::Object(obj)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_type_matcher_and_strips_to_example() {
        let contents_config = serde_json::json!({
            "pact:content-type": "application/mcp+json",
            "mcp": {
                "operation": "tools/call",
                "request": { "name": "get_weather", "arguments": { "city": "Melbourne" } },
                "response": { "content": [ { "type": "text", "text": "matching(type, 'Sunny, 22C')" } ], "isError": false }
            }
        });

        let configured = configure_interaction(&contents_config).expect("valid config");

        // Response part body has the DSL stripped to the example value.
        assert_eq!(
            configured.response.body,
            serde_json::json!({ "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false })
        );

        // Rule recorded at the DocPath, rooted at the response part body ($.).
        assert!(configured.response.rules.contains_key("$.content[0].text"));
        assert_eq!(configured.response.rules["$.content[0].text"].rule[0].r#type, "type");
        // The request part carries no rules for this interaction.
        assert!(configured.request.rules.is_empty());

        // Round-trips into the internal Rules shape our engine reads.
        let response_rules_value = rules_value(&configured.response.rules);
        assert_eq!(
            response_rules_value,
            serde_json::json!({ "$.content[0].text": { "matchers": [ { "match": "type" } ] } })
        );
    }

    #[test]
    fn parses_number_matcher_and_produces_numeric_example() {
        let contents_config = serde_json::json!({
            "operation": "tools/call",
            "request": { "name": "get_weather", "arguments": { "zoom": "matching(number, 5)" } },
            "response": { "content": [], "isError": false }
        });
        let configured = configure_interaction(&contents_config).expect("valid config");
        assert_eq!(
            configured.request.body,
            serde_json::json!({ "name": "get_weather", "arguments": { "zoom": 5.0 } })
        );
        assert_eq!(configured.request.rules["$.arguments.zoom"].rule[0].r#type, "number");
    }

    #[test]
    fn regex_matcher_carries_the_regex_in_proto_values() {
        let contents_config = serde_json::json!({
            "operation": "tools/call",
            "request": { "name": "get_weather", "arguments": {} },
            "response": { "content": [ { "type": "text", "text": "matching(regex, '^[A-Z].*', 'Sunny')" } ], "isError": false }
        });
        let configured = configure_interaction(&contents_config).expect("valid config");
        let rule = &configured.response.rules["$.content[0].text"].rule[0];
        assert_eq!(rule.r#type, "regex");
        let values = rule.values.as_ref().expect("regex carries values");
        assert!(values.fields.contains_key("regex"));
    }

    #[test]
    fn literal_values_produce_no_rules() {
        let contents_config = serde_json::json!({
            "operation": "tools/call",
            "request": { "name": "get_weather", "arguments": { "city": "Melbourne" } },
            "response": { "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false }
        });
        let configured = configure_interaction(&contents_config).expect("valid config");
        assert!(configured.request.rules.is_empty(), "literal request produces no rules");
        assert!(configured.response.rules.is_empty(), "literal response produces no rules");
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

    #[test]
    fn persisted_body_category_matches_pact_core_shape() {
        let contents_config = serde_json::json!({
            "operation": "tools/call",
            "request": { "name": "get_weather", "arguments": {} },
            "response": { "content": [ { "type": "text", "text": "matching(type, 'x')" } ], "isError": false }
        });
        let configured = configure_interaction(&contents_config).expect("valid config");
        let body_category = persisted_body_category(&configured.response.rules);
        // Shape: { "$.content[0].text": { "combine": "AND", "matchers": [ { "match": "type" } ] } }
        let rule = &body_category["$.content[0].text"];
        assert_eq!(rule["combine"], "AND");
        assert_eq!(rule["matchers"][0]["match"], "type");
    }
}
