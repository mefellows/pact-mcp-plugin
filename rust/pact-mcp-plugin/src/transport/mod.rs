//! `McpTransport` — how the engine reaches a real MCP server for verification.
//! Phase 1: `stdio`. Phase 2: Streamable HTTP (see docs/decisions/0007).

pub mod http;
pub mod stdio;

use crate::mcp::model::Operation;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RoleClient, RunningService};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("failed to spawn/connect transport: {0}")]
    Connect(String),
    #[error("initialize handshake failed: {0}")]
    Handshake(String),
    #[error("request failed: {0}")]
    Request(String),
}

/// The result of a `tools/call` or `tools/list` request, already reshaped into
/// the MCP<->Pact response shape (docs/spec/interaction-schema.md §3):
/// either a success value (`{content, isError, structuredContent}` /
/// `{tools}`), or a JSON-RPC protocol error (`{error: {code, message, data}}`).
pub type McpResponse = Value;

/// Perform an interaction's request against a connected rmcp client service and
/// reshape the result into the MCP<->Pact response shape. Shared by the stdio
/// and HTTP transports (both wrap a `RunningService<RoleClient, ()>`).
pub(crate) async fn perform_on_service(
    service: &RunningService<RoleClient, ()>,
    operation: Operation,
    request: &Value,
) -> Result<McpResponse, TransportError> {
    match operation {
        Operation::ToolsCall => call_tool(service, request).await,
        Operation::ToolsList => list_tools(service).await,
        Operation::ResourcesRead => read_resource(service, request).await,
        Operation::ResourcesList => list_resources(service).await,
        Operation::PromptsGet => get_prompt(service, request).await,
        Operation::PromptsList => list_prompts(service).await,
    }
}

/// Reshape an rmcp protocol-level error into the spec's `{error: {...}}` shape.
fn protocol_error(err: rmcp::model::ErrorData) -> McpResponse {
    serde_json::json!({
        "error": { "code": err.code.0, "message": err.message.to_string(), "data": err.data }
    })
}

async fn call_tool(
    service: &RunningService<RoleClient, ()>,
    request: &Value,
) -> Result<McpResponse, TransportError> {
    let name = request
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| TransportError::Request("mcp.request.name is required for tools/call".to_string()))?
        .to_string();

    let mut params = CallToolRequestParams::new(name);
    if let Some(args) = request.get("arguments").and_then(Value::as_object) {
        params = params.with_arguments(args.clone());
    }

    match service.call_tool(params).await {
        Ok(result) => {
            let mut value = serde_json::to_value(&result).map_err(|e| TransportError::Request(e.to_string()))?;
            if value.get("isError").is_none() {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("isError".to_string(), Value::Bool(false));
                }
            }
            Ok(value)
        }
        Err(rmcp::ServiceError::McpError(err)) => Ok(serde_json::json!({
            "error": { "code": err.code.0, "message": err.message.to_string(), "data": err.data }
        })),
        Err(e) => Err(TransportError::Request(e.to_string())),
    }
}

async fn list_tools(service: &RunningService<RoleClient, ()>) -> Result<McpResponse, TransportError> {
    let tools = service
        .list_all_tools()
        .await
        .map_err(|e| TransportError::Request(e.to_string()))?;
    let value = serde_json::to_value(&tools).map_err(|e| TransportError::Request(e.to_string()))?;
    Ok(serde_json::json!({ "tools": value }))
}

async fn read_resource(
    service: &RunningService<RoleClient, ()>,
    request: &Value,
) -> Result<McpResponse, TransportError> {
    let params: rmcp::model::ReadResourceRequestParams = serde_json::from_value(request.clone())
        .map_err(|e| TransportError::Request(format!("mcp.request is not a valid resources/read request: {e}")))?;
    match service.read_resource(params).await {
        Ok(result) => serde_json::to_value(&result).map_err(|e| TransportError::Request(e.to_string())),
        Err(rmcp::ServiceError::McpError(err)) => Ok(protocol_error(err)),
        Err(e) => Err(TransportError::Request(e.to_string())),
    }
}

async fn list_resources(service: &RunningService<RoleClient, ()>) -> Result<McpResponse, TransportError> {
    let resources = service
        .list_all_resources()
        .await
        .map_err(|e| TransportError::Request(e.to_string()))?;
    let value = serde_json::to_value(&resources).map_err(|e| TransportError::Request(e.to_string()))?;
    Ok(serde_json::json!({ "resources": value }))
}

async fn get_prompt(
    service: &RunningService<RoleClient, ()>,
    request: &Value,
) -> Result<McpResponse, TransportError> {
    let params: rmcp::model::GetPromptRequestParams = serde_json::from_value(request.clone())
        .map_err(|e| TransportError::Request(format!("mcp.request is not a valid prompts/get request: {e}")))?;
    match service.get_prompt(params).await {
        Ok(result) => serde_json::to_value(&result).map_err(|e| TransportError::Request(e.to_string())),
        Err(rmcp::ServiceError::McpError(err)) => Ok(protocol_error(err)),
        Err(e) => Err(TransportError::Request(e.to_string())),
    }
}

async fn list_prompts(service: &RunningService<RoleClient, ()>) -> Result<McpResponse, TransportError> {
    let prompts = service
        .list_all_prompts()
        .await
        .map_err(|e| TransportError::Request(e.to_string()))?;
    let value = serde_json::to_value(&prompts).map_err(|e| TransportError::Request(e.to_string()))?;
    Ok(serde_json::json!({ "prompts": value }))
}
