//! Serde types for the persisted MCP<->Pact interaction fragment.
//! See docs/spec/interaction-schema.md (normative).

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CONTENT_TYPE: &str = "application/mcp+json";
pub const SCHEMA_VERSION: &str = "0";

/// The full persisted plugin-specific interaction contents.
///
/// ```jsonc
/// {
///   "pact:content-type": "application/mcp+json",
///   "mcp": { ... }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpFragment {
    #[serde(rename = "pact:content-type")]
    pub pact_content_type: String,
    pub mcp: McpInteraction,
}

impl McpFragment {
    pub fn new(mcp: McpInteraction) -> Self {
        Self {
            pact_content_type: CONTENT_TYPE.to_string(),
            mcp,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    #[serde(rename = "tools/call")]
    ToolsCall,
    #[serde(rename = "tools/list")]
    ToolsList,
    #[serde(rename = "resources/read")]
    ResourcesRead,
    #[serde(rename = "resources/list")]
    ResourcesList,
    #[serde(rename = "prompts/get")]
    PromptsGet,
    #[serde(rename = "prompts/list")]
    PromptsList,
}

impl Operation {
    /// The JSON-RPC method name synthesized for this operation.
    pub fn method(&self) -> &'static str {
        match self {
            Operation::ToolsCall => "tools/call",
            Operation::ToolsList => "tools/list",
            Operation::ResourcesRead => "resources/read",
            Operation::ResourcesList => "resources/list",
            Operation::PromptsGet => "prompts/get",
            Operation::PromptsList => "prompts/list",
        }
    }

    pub fn parse(s: &str) -> Option<Operation> {
        match s {
            "tools/call" => Some(Operation::ToolsCall),
            "tools/list" => Some(Operation::ToolsList),
            "resources/read" => Some(Operation::ResourcesRead),
            "resources/list" => Some(Operation::ResourcesList),
            "prompts/get" => Some(Operation::PromptsGet),
            "prompts/list" => Some(Operation::PromptsList),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerHint {
    pub transport: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpInteraction {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    pub operation: Operation,
    /// Operation-specific JSON-RPC params (the semantic request the consumer authored).
    pub request: Value,
    /// Operation-specific JSON-RPC result (the semantic response the consumer expects).
    pub response: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerHint>,
    /// Provider states from the interaction's standard V4 `providerStates`
    /// field (ADR 0009). Engine-internal (applied at verification time); never
    /// part of the persisted `mcp` fragment.
    #[serde(skip)]
    pub provider_states: Option<Value>,
}

impl McpInteraction {
    pub fn new(operation: Operation, request: Value, response: Value) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            operation,
            request,
            response,
            server: None,
            provider_states: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_tools_call_fragment_losslessly() {
        let fragment = McpFragment::new(McpInteraction::new(
            Operation::ToolsCall,
            serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } }),
            serde_json::json!({ "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false }),
        ));

        let json = serde_json::to_string(&fragment).unwrap();
        let round_tripped: McpFragment = serde_json::from_str(&json).unwrap();
        assert_eq!(fragment, round_tripped);
    }

    #[test]
    fn round_trips_tools_list_fragment_losslessly() {
        let fragment = McpFragment::new(McpInteraction::new(
            Operation::ToolsList,
            serde_json::json!({}),
            serde_json::json!({ "tools": [ { "name": "get_weather", "inputSchema": { "type": "object" } } ] }),
        ));

        let json = serde_json::to_string(&fragment).unwrap();
        let round_tripped: McpFragment = serde_json::from_str(&json).unwrap();
        assert_eq!(fragment, round_tripped);
    }

    #[test]
    fn operation_serializes_to_the_wire_method_name() {
        assert_eq!(
            serde_json::to_value(Operation::ToolsCall).unwrap(),
            serde_json::json!("tools/call")
        );
        assert_eq!(Operation::parse("tools/call"), Some(Operation::ToolsCall));
        assert_eq!(Operation::parse("bogus"), None);
    }

    #[test]
    fn unknown_operation_is_a_hard_deserialize_error() {
        let bad = serde_json::json!({
            "pact:content-type": "application/mcp+json",
            "mcp": {
                "schemaVersion": "0",
                "operation": "prompts/summon",
                "request": {},
                "response": {}
            }
        });
        let result: Result<McpFragment, _> = serde_json::from_value(bad);
        assert!(result.is_err());
    }
}
