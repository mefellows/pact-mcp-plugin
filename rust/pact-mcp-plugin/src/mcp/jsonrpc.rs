//! JSON-RPC 2.0 envelope synthesis. The envelope (`jsonrpc`, `id`, `method`) is
//! never authored by the user — the engine derives `method` from `operation` and
//! assigns `id`s itself (see docs/spec/interaction-schema.md §2).

use super::model::Operation;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicI64, Ordering};

static NEXT_ID: AtomicI64 = AtomicI64::new(1);

/// Allocate the next monotonic request id for this process.
pub fn next_id() -> i64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// Build a request envelope for a pact `mcp` interaction's request params.
    pub fn for_operation(operation: Operation, id: i64, params: Value) -> Self {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Value::from(id),
            method: operation.method().to_string(),
            params: Some(params),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Extract the "MCP-shape" response value that the engine matches against:
    /// either the `result` object, or `{"error": {...}}` mirroring the pact
    /// fragment's `response.error` shape (docs/spec/interaction-schema.md §3.1).
    pub fn to_mcp_response_value(&self) -> Value {
        if let Some(err) = &self.error {
            serde_json::json!({ "error": { "code": err.code, "message": err.message, "data": err.data } })
        } else {
            self.result.clone().unwrap_or(Value::Null)
        }
    }

    /// Does this response's `id` correlate with the given request id?
    pub fn correlates(&self, id: i64) -> bool {
        self.id == Value::from(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::model::Operation;

    #[test]
    fn synthesizes_tools_call_envelope_from_operation_and_request() {
        let req = JsonRpcRequest::for_operation(
            Operation::ToolsCall,
            7,
            serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } }),
        );
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "tools/call");
        assert_eq!(req.id, serde_json::json!(7));
        assert_eq!(
            req.params.unwrap(),
            serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } })
        );
    }

    #[test]
    fn ids_are_monotonically_increasing() {
        let a = next_id();
        let b = next_id();
        assert!(b > a);
    }

    #[test]
    fn error_response_maps_to_mcp_error_shape() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Invalid params".to_string(),
                data: None,
            }),
        };
        assert_eq!(
            resp.to_mcp_response_value(),
            serde_json::json!({ "error": { "code": -32602, "message": "Invalid params", "data": null } })
        );
    }

    #[test]
    fn result_response_maps_to_the_result_value_directly() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            result: Some(serde_json::json!({ "content": [], "isError": false })),
            error: None,
        };
        assert_eq!(
            resp.to_mcp_response_value(),
            serde_json::json!({ "content": [], "isError": false })
        );
    }
}
