//! stdio transport: spawn an MCP server subprocess, do the `initialize` /
//! `initialized` handshake via `rmcp`, and drive `tools/call` / `tools/list`.
//! See docs/decisions/0003-rmcp-vs-raw-jsonrpc.md for why `rmcp` was chosen.

use super::{perform_on_service, McpResponse, TransportError};
use crate::mcp::model::Operation;
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

        let transport = TokioChildProcess::new(cmd).map_err(|e| TransportError::Connect(e.to_string()))?;

        let service = ()
            .serve(transport)
            .await
            .map_err(|e| TransportError::Handshake(e.to_string()))?;

        Ok(Self { service })
    }

    /// Perform the interaction's request against the real server, reshaped into
    /// the MCP<->Pact response shape.
    pub async fn perform(&self, operation: Operation, request: &Value) -> Result<McpResponse, TransportError> {
        perform_on_service(&self.service, operation, request).await
    }

    pub async fn close(self) -> Result<(), TransportError> {
        self.service
            .cancel()
            .await
            .map_err(|e| TransportError::Request(e.to_string()))?;
        Ok(())
    }
}
