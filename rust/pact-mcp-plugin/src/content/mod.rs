//! MCP-aware content matching. See docs/spec/matching-semantics.md (normative).
//!
//! This intentionally does not implement a general-purpose JSON matching engine
//! (e.g. full `pact_matching`) — the MCP interaction shapes are a small closed
//! vocabulary (`tools/call` result, `tools/list` result), so the comparator is
//! written directly against that vocabulary. matching-semantics.md is the source
//! of truth; docs/spec/conformance/*.json pins exact behavior.

use crate::mcp::model::Operation;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Mismatch {
    pub path: String,
    pub expected: Value,
    pub actual: Value,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub mismatches: Vec<Mismatch>,
}

impl MatchResult {
    pub fn is_match(&self) -> bool {
        self.mismatches.is_empty()
    }

    pub fn mismatch_paths(&self) -> BTreeSet<String> {
        self.mismatches.iter().map(|m| m.path.clone()).collect()
    }
}

/// Matching rule lookup: `matchingRules.response` (or `.request`), keyed by JSON
/// path rooted at the `response`/`request` object (docs/spec/interaction-schema.md §2).
///
/// Shape (per docs/spec/conformance fixtures):
/// ```jsonc
/// { "$.content[0].text": { "matchers": [ { "match": "type" } ] } }
/// ```
#[derive(Debug, Clone, Default)]
pub struct Rules<'a> {
    raw: Option<&'a Value>,
}

impl<'a> Rules<'a> {
    pub fn new(raw: Option<&'a Value>) -> Self {
        Self { raw }
    }

    /// Returns the `match` type (e.g. `"type"`) for a path, if a rule exists.
    fn match_type(&self, path: &str) -> Option<&str> {
        let obj = self.raw?.as_object()?;
        let rule = obj.get(path)?.as_object()?;
        let matchers = rule.get("matchers")?.as_array()?;
        matchers.first()?.as_object()?.get("match")?.as_str()
    }

    fn is_type_matcher(&self, path: &str) -> bool {
        self.match_type(path) == Some("type")
    }
}

/// Compare an `actual` JSON-RPC response value (a `result`, or `{"error": {...}}`)
/// against the expected `mcp.response` fragment, per the interaction's `operation`.
pub fn compare_response(
    operation: Operation,
    expected: &Value,
    actual: &Value,
    rules: &Rules,
) -> MatchResult {
    match operation {
        Operation::ToolsCall => compare_tools_call(expected, actual, rules),
        Operation::ToolsList => compare_tools_list(expected, actual, rules),
        // resources/read + prompts/get: structural comparison rooted at $ —
        // keys the consumer specified must match (exact scalars unless ruled),
        // extra actual keys ignored. Cross-shape (success vs protocol error)
        // handled like tools/call.
        Operation::ResourcesRead | Operation::PromptsGet => compare_structural_result(expected, actual, rules),
        // Subset lists, keyed like tools/list (matching-semantics §3).
        Operation::ResourcesList => compare_subset_list(expected, actual, rules, "resources", "uri"),
        Operation::PromptsList => compare_subset_list(expected, actual, rules, "prompts", "name"),
    }
}

/// Shared success-vs-protocol-error shape check; pushes a `$` mismatch and
/// returns true when the shapes diverge (further comparison is pointless).
fn cross_shape_mismatch(expected: &Value, actual: &Value, mismatches: &mut Vec<Mismatch>) -> bool {
    let expected_is_error = expected.get("error").is_some();
    let actual_is_error = actual.get("error").is_some();
    if expected_is_error != actual_is_error {
        mismatches.push(Mismatch {
            path: "$".to_string(),
            expected: expected.clone(),
            actual: actual.clone(),
            message: format!(
                "expected a {} result but got a {} result",
                if expected_is_error { "protocol error" } else { "success" },
                if actual_is_error { "protocol error" } else { "success" },
            ),
        });
        return true;
    }
    false
}

/// resources/read + prompts/get results: full structural comparison.
fn compare_structural_result(expected: &Value, actual: &Value, rules: &Rules) -> MatchResult {
    let mut mismatches = Vec::new();
    if cross_shape_mismatch(expected, actual, &mut mismatches) {
        return MatchResult { mismatches };
    }
    if expected.get("error").is_some() {
        compare_error(expected, actual, rules, &mut mismatches);
        return MatchResult { mismatches };
    }
    compare_structured("$", expected, actual, rules, &mut mismatches);
    MatchResult { mismatches }
}

/// Subset list matching (resources/list, prompts/list): every expected item
/// must be present in the actual list, keyed by `key_field`, order-independent;
/// other specified keys on a matched item are compared structurally.
fn compare_subset_list(
    expected: &Value,
    actual: &Value,
    rules: &Rules,
    list_field: &str,
    key_field: &str,
) -> MatchResult {
    let mut mismatches = Vec::new();
    let expected_items = expected.get(list_field).and_then(Value::as_array).cloned().unwrap_or_default();
    let actual_items = actual.get(list_field).and_then(Value::as_array).cloned().unwrap_or_default();

    for expected_item in &expected_items {
        let key = expected_item.get(key_field).and_then(Value::as_str).unwrap_or("");
        let found = actual_items
            .iter()
            .find(|i| i.get(key_field).and_then(Value::as_str) == Some(key));
        let path = format!("$.{list_field}[?(@.{key_field}=='{key}')]");
        match found {
            None => mismatches.push(Mismatch {
                path,
                expected: expected_item.clone(),
                actual: Value::Null,
                message: format!("expected {list_field} item \"{key}\" not found in actual {list_field}[]"),
            }),
            Some(actual_item) => compare_structured(&path, expected_item, actual_item, rules, &mut mismatches),
        }
    }
    MatchResult { mismatches }
}

/// Request matching for resources/read (uri) and prompts/get (name +
/// arguments), used by the mock to select an interaction.
pub fn match_structural_request(expected: &Value, actual: &Value, rules: &Rules) -> MatchResult {
    let mut mismatches = Vec::new();
    compare_structured("$", expected, actual, rules, &mut mismatches);
    MatchResult { mismatches }
}

/// Request matching for the mock server (docs/spec/matching-semantics.md §4):
/// select the interaction whose `mcp.request.name` equals the incoming tool
/// name AND whose `arguments` match under `matchingRules.request` (default
/// exact for literals, matchers as authored). Returns the match result; an
/// empty mismatch list means the incoming call matches this interaction.
pub fn match_tools_call_request(expected: &Value, actual: &Value, rules: &Rules) -> MatchResult {
    let mut mismatches = Vec::new();

    let expected_name = expected.get("name").and_then(Value::as_str).unwrap_or("");
    let actual_name = actual.get("name").and_then(Value::as_str).unwrap_or("");
    if expected_name != actual_name {
        mismatches.push(Mismatch {
            path: "$.name".to_string(),
            expected: Value::from(expected_name),
            actual: Value::from(actual_name),
            message: format!("expected tool name \"{expected_name}\" but got \"{actual_name}\""),
        });
        // Name mismatch is decisive — don't bother diffing arguments.
        return MatchResult { mismatches };
    }

    let expected_args = expected.get("arguments").cloned().unwrap_or(Value::Null);
    let actual_args = actual.get("arguments").cloned().unwrap_or(Value::Null);
    compare_structured("$.arguments", &expected_args, &actual_args, rules, &mut mismatches);

    MatchResult { mismatches }
}

fn compare_tools_call(expected: &Value, actual: &Value, rules: &Rules) -> MatchResult {
    let mut mismatches = Vec::new();

    let expected_is_error_result = expected.get("error").is_some();
    let actual_is_error_result = actual.get("error").is_some();

    if expected_is_error_result != actual_is_error_result {
        mismatches.push(Mismatch {
            path: "$".to_string(),
            expected: expected.clone(),
            actual: actual.clone(),
            message: format!(
                "expected a {} result but got a {} result",
                if expected_is_error_result { "protocol error" } else { "success" },
                if actual_is_error_result { "protocol error" } else { "success" },
            ),
        });
        return MatchResult { mismatches };
    }

    if expected_is_error_result {
        compare_error(expected, actual, rules, &mut mismatches);
        return MatchResult { mismatches };
    }

    // content[]
    let expected_content = expected.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
    let actual_content = actual.get("content").and_then(Value::as_array).cloned().unwrap_or_default();

    if actual_content.len() < expected_content.len() {
        mismatches.push(Mismatch {
            path: "$.content".to_string(),
            expected: Value::from(expected_content.len()),
            actual: Value::from(actual_content.len()),
            message: format!(
                "expected at least {} content block(s), got {}",
                expected_content.len(),
                actual_content.len()
            ),
        });
    } else {
        for (i, expected_block) in expected_content.iter().enumerate() {
            let actual_block = &actual_content[i];
            compare_content_block(i, expected_block, actual_block, rules, &mut mismatches);
        }
    }

    // isError: exact, missing expected treated as false
    let expected_is_error = expected.get("isError").and_then(Value::as_bool).unwrap_or(false);
    let actual_is_error = actual.get("isError").and_then(Value::as_bool).unwrap_or(false);
    if expected_is_error != actual_is_error {
        mismatches.push(Mismatch {
            path: "$.isError".to_string(),
            expected: Value::from(expected_is_error),
            actual: Value::from(actual_is_error),
            message: format!("expected isError={} but got isError={}", expected_is_error, actual_is_error),
        });
    }

    // structuredContent: full JSON match, exact for provided scalars, extra keys ignored
    if let Some(expected_sc) = expected.get("structuredContent") {
        let actual_sc = actual.get("structuredContent").cloned().unwrap_or(Value::Null);
        compare_structured(
            "$.structuredContent",
            expected_sc,
            &actual_sc,
            rules,
            &mut mismatches,
        );
    }

    MatchResult { mismatches }
}

fn compare_content_block(
    index: usize,
    expected: &Value,
    actual: &Value,
    rules: &Rules,
    mismatches: &mut Vec<Mismatch>,
) {
    let base = format!("$.content[{index}]");
    let expected_type = expected.get("type").and_then(Value::as_str).unwrap_or("");
    let actual_type = actual.get("type").and_then(Value::as_str).unwrap_or("");

    let type_path = format!("{base}.type");
    if !values_match(&type_path, &Value::from(expected_type), &Value::from(actual_type), rules) {
        mismatches.push(Mismatch {
            path: type_path,
            expected: Value::from(expected_type),
            actual: Value::from(actual_type),
            message: format!("expected content[{index}].type = \"{expected_type}\" but got \"{actual_type}\""),
        });
        return;
    }

    match expected_type {
        "text" => {
            let path = format!("{base}.text");
            let expected_text = expected.get("text").cloned().unwrap_or(Value::Null);
            let actual_text = actual.get("text").cloned().unwrap_or(Value::Null);
            if !values_match(&path, &expected_text, &actual_text, rules) {
                mismatches.push(Mismatch {
                    path: path.clone(),
                    expected: expected_text.clone(),
                    actual: actual_text.clone(),
                    message: format!(
                        "expected content[{index}].text = {expected_text} but got {actual_text}"
                    ),
                });
            }
        }
        "image" => {
            let mime_path = format!("{base}.mimeType");
            let expected_mime = expected.get("mimeType").cloned().unwrap_or(Value::Null);
            let actual_mime = actual.get("mimeType").cloned().unwrap_or(Value::Null);
            if !values_match(&mime_path, &expected_mime, &actual_mime, rules) {
                mismatches.push(Mismatch {
                    path: mime_path.clone(),
                    expected: expected_mime,
                    actual: actual_mime,
                    message: format!("mimeType mismatch at content[{index}]"),
                });
            }
            // data: default type-only (don't force byte equality unless ruled exact)
            let data_path = format!("{base}.data");
            let expected_data = expected.get("data").cloned().unwrap_or(Value::Null);
            let actual_data = actual.get("data").cloned().unwrap_or(Value::Null);
            let ok = if rules.raw.is_some() && rules.match_type(&data_path).is_some() {
                values_match(&data_path, &expected_data, &actual_data, rules)
            } else {
                same_json_type(&expected_data, &actual_data)
            };
            if !ok {
                mismatches.push(Mismatch {
                    path: data_path.clone(),
                    expected: expected_data,
                    actual: actual_data,
                    message: format!("data type mismatch at content[{index}]"),
                });
            }
        }
        "resource" => {
            let uri_path = format!("{base}.resource.uri");
            let expected_uri = expected.pointer("/resource/uri").cloned().unwrap_or(Value::Null);
            let actual_uri = actual.pointer("/resource/uri").cloned().unwrap_or(Value::Null);
            if !values_match(&uri_path, &expected_uri, &actual_uri, rules) {
                mismatches.push(Mismatch {
                    path: uri_path.clone(),
                    expected: expected_uri,
                    actual: actual_uri,
                    message: format!("resource.uri mismatch at content[{index}]"),
                });
            }
        }
        other => {
            mismatches.push(Mismatch {
                path: format!("{base}.type"),
                expected: Value::from(other),
                actual: Value::from(actual_type),
                message: format!("unsupported content block type \"{other}\""),
            });
        }
    }
}

fn compare_error(expected: &Value, actual: &Value, rules: &Rules, mismatches: &mut Vec<Mismatch>) {
    let expected_err = expected.get("error").cloned().unwrap_or(Value::Null);
    let actual_err = actual.get("error").cloned().unwrap_or(Value::Null);

    let code_path = "$.error.code";
    let expected_code = expected_err.get("code").cloned().unwrap_or(Value::Null);
    let actual_code = actual_err.get("code").cloned().unwrap_or(Value::Null);
    if expected_code != actual_code {
        mismatches.push(Mismatch {
            path: code_path.to_string(),
            expected: expected_code,
            actual: actual_code,
            message: "error.code mismatch".to_string(),
        });
    }

    // message: by type by default (unless the interaction ruled it exact/other)
    let message_path = "$.error.message";
    let expected_message = expected_err.get("message").cloned().unwrap_or(Value::Null);
    let actual_message = actual_err.get("message").cloned().unwrap_or(Value::Null);
    let ok = if rules.match_type(message_path).is_some() {
        values_match(message_path, &expected_message, &actual_message, rules)
    } else {
        same_json_type(&expected_message, &actual_message)
    };
    if !ok {
        mismatches.push(Mismatch {
            path: message_path.to_string(),
            expected: expected_message,
            actual: actual_message,
            message: "error.message type mismatch".to_string(),
        });
    }

    // data: by structure (keys present matched by type)
    if let Some(expected_data) = expected_err.get("data") {
        let actual_data = actual_err.get("data").cloned().unwrap_or(Value::Null);
        compare_structured("$.error.data", expected_data, &actual_data, rules, mismatches);
    }
}

fn compare_tools_list(expected: &Value, actual: &Value, _rules: &Rules) -> MatchResult {
    let mut mismatches = Vec::new();
    let expected_tools = expected.get("tools").and_then(Value::as_array).cloned().unwrap_or_default();
    let actual_tools = actual.get("tools").and_then(Value::as_array).cloned().unwrap_or_default();

    for expected_tool in &expected_tools {
        let name = expected_tool.get("name").and_then(Value::as_str).unwrap_or("");
        let found = actual_tools
            .iter()
            .find(|t| t.get("name").and_then(Value::as_str) == Some(name));

        match found {
            None => mismatches.push(Mismatch {
                path: format!("$.tools[?(@.name=='{name}')]"),
                expected: expected_tool.clone(),
                actual: Value::Null,
                message: format!("expected tool \"{name}\" not found in actual tools[]"),
            }),
            Some(actual_tool) => {
                if let Some(expected_schema) = expected_tool.get("inputSchema") {
                    let actual_schema = actual_tool.get("inputSchema").cloned().unwrap_or(Value::Null);
                    let path = format!("$.tools[?(@.name=='{name}')].inputSchema");
                    let mut schema_mismatches = Vec::new();
                    compare_structured(&path, expected_schema, &actual_schema, &Rules::default(), &mut schema_mismatches);
                    mismatches.extend(schema_mismatches);
                }
            }
        }
    }

    MatchResult { mismatches }
}

/// Structural JSON comparison: keys present in `expected` must be present and
/// type-compatible (recursively) in `actual`; extra keys on `actual` are ignored.
/// Scalars are matched exactly unless ruled otherwise.
fn compare_structured(path: &str, expected: &Value, actual: &Value, rules: &Rules, mismatches: &mut Vec<Mismatch>) {
    match expected {
        Value::Object(expected_map) => {
            let actual_map = actual.as_object();
            for (key, expected_val) in expected_map {
                let child_path = format!("{path}.{key}");
                match actual_map.and_then(|m| m.get(key)) {
                    None => mismatches.push(Mismatch {
                        path: child_path,
                        expected: expected_val.clone(),
                        actual: Value::Null,
                        message: format!("missing key \"{key}\""),
                    }),
                    Some(actual_val) => compare_structured(&child_path, expected_val, actual_val, rules, mismatches),
                }
            }
        }
        Value::Array(expected_arr) => {
            let actual_arr = actual.as_array().cloned().unwrap_or_default();
            for (i, expected_item) in expected_arr.iter().enumerate() {
                let child_path = format!("{path}[{i}]");
                match actual_arr.get(i) {
                    None => mismatches.push(Mismatch {
                        path: child_path,
                        expected: expected_item.clone(),
                        actual: Value::Null,
                        message: "missing array element".to_string(),
                    }),
                    Some(actual_item) => compare_structured(&child_path, expected_item, actual_item, rules, mismatches),
                }
            }
        }
        scalar => {
            if !values_match(path, scalar, actual, rules) {
                mismatches.push(Mismatch {
                    path: path.to_string(),
                    expected: scalar.clone(),
                    actual: actual.clone(),
                    message: format!("expected {scalar} but got {actual}"),
                });
            }
        }
    }
}

fn values_match(path: &str, expected: &Value, actual: &Value, rules: &Rules) -> bool {
    if rules.is_type_matcher(path) {
        same_json_type(expected, actual)
    } else {
        expected == actual
    }
}

fn same_json_type(a: &Value, b: &Value) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_matches_on_name_and_exact_arguments() {
        let expected = serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } });
        let actual = serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } });
        let result = match_tools_call_request(&expected, &actual, &Rules::default());
        assert!(result.is_match());
    }

    #[test]
    fn request_mismatch_on_wrong_tool_name() {
        let expected = serde_json::json!({ "name": "get_weather", "arguments": {} });
        let actual = serde_json::json!({ "name": "list_stations", "arguments": {} });
        let result = match_tools_call_request(&expected, &actual, &Rules::default());
        assert!(!result.is_match());
        assert!(result.mismatch_paths().contains("$.name"));
    }

    #[test]
    fn request_mismatch_on_wrong_argument_value() {
        let expected = serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } });
        let actual = serde_json::json!({ "name": "get_weather", "arguments": { "city": "Sydney" } });
        let result = match_tools_call_request(&expected, &actual, &Rules::default());
        assert!(!result.is_match());
        assert!(result.mismatch_paths().contains("$.arguments.city"));
    }

    #[test]
    fn request_argument_type_matcher_accepts_any_string() {
        let expected = serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } });
        let actual = serde_json::json!({ "name": "get_weather", "arguments": { "city": "Perth" } });
        let rules_value = serde_json::json!({ "$.arguments.city": { "matchers": [ { "match": "type" } ] } });
        let rules = Rules::new(Some(&rules_value));
        let result = match_tools_call_request(&expected, &actual, &rules);
        assert!(result.is_match(), "type matcher should accept a different string: {:?}", result.mismatches);
    }
}
