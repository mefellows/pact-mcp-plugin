//! `McpTransport` — how the engine reaches a real MCP server for verification.
//! Phase 1 implements `stdio` only (see docs/plans §9). Streamable HTTP is Phase 2.

pub mod stdio;

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
