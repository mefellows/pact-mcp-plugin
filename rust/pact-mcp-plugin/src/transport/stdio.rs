//! stdio transport: spawn an MCP server subprocess, do the `initialize` /
//! `initialized` handshake via `rmcp`, and drive `tools/call` / `tools/list`.
//! See docs/decisions/0003-rmcp-vs-raw-jsonrpc.md for why `rmcp` was chosen.

use super::{McpResponse, TransportError};
use crate::mcp::model::Operation;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::ServiceExt;
use serde_json::Value;
use std::collections::HashMap;
use tokio::process::Command;

/// A connected stdio MCP client: subprocess spawned, `initialize`/`initialized`
/// handshake complete.
pub struct StdioClient {
    service: RunningService<RoleClient, ()>,
}

impl StdioClient {
    /// Spawn `command args...` (with optional extra env vars) as an MCP server
    /// over stdio and complete the initialize handshake.
    pub async fn connect(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, TransportError> {
        let args = args.to_vec();
        let env = env.clone();
        let cmd = Command::new(command).configure(move |c| {
            c.args(&args);
            for (k, v) in &env {
                c.env(k, v);
            }
        });

        let transport = TokioChildProcess::new(cmd)
            .map_err(|e| TransportError::Connect(e.to_string()))?;

        let service = ()
            .serve(transport)
            .await
            .map_err(|e| TransportError::Handshake(e.to_string()))?;

        Ok(Self { service })
    }

    /// Perform the interaction's request against the real server and return the
    /// response reshaped into the MCP<->Pact response shape for the given operation.
    pub async fn perform(&self, operation: Operation, request: &Value) -> Result<McpResponse, TransportError> {
        match operation {
            Operation::ToolsCall => self.call_tool(request).await,
            Operation::ToolsList => self.list_tools().await,
        }
    }

    async fn call_tool(&self, request: &Value) -> Result<McpResponse, TransportError> {
        let name = request
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| TransportError::Request("mcp.request.name is required for tools/call".to_string()))?
            .to_string();

        let mut params = CallToolRequestParams::new(name);
        if let Some(args) = request.get("arguments").and_then(Value::as_object) {
            params = params.with_arguments(args.clone());
        }

        match self.service.call_tool(params).await {
            Ok(result) => {
                let mut value = serde_json::to_value(&result)
                    .map_err(|e| TransportError::Request(e.to_string()))?;
                // Our schema treats a missing isError as false explicitly is handled
                // by the matcher, but normalize here too for readability/logging.
                if value.get("isError").is_none() {
                    if let Some(obj) = value.as_object_mut() {
                        obj.insert("isError".to_string(), Value::Bool(false));
                    }
                }
                Ok(value)
            }
            Err(rmcp::ServiceError::McpError(err)) => Ok(serde_json::json!({
                "error": {
                    "code": err.code.0,
                    "message": err.message.to_string(),
                    "data": err.data,
                }
            })),
            Err(e) => Err(TransportError::Request(e.to_string())),
        }
    }

    async fn list_tools(&self) -> Result<McpResponse, TransportError> {
        let tools = self
            .service
            .list_all_tools()
            .await
            .map_err(|e| TransportError::Request(e.to_string()))?;
        let value = serde_json::to_value(&tools).map_err(|e| TransportError::Request(e.to_string()))?;
        Ok(serde_json::json!({ "tools": value }))
    }

    pub async fn close(self) -> Result<(), TransportError> {
        self.service
            .cancel()
            .await
            .map_err(|e| TransportError::Request(e.to_string()))?;
        Ok(())
    }
}
