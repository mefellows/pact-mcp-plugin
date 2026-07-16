//! `initialize` -> `initialized` handshake + capability/protocol-version
//! negotiation is connection-level (once per session), never a per-interaction
//! artifact (docs/spec/interaction-schema.md §1). For the stdio transport this
//! is fully handled by `rmcp::ServiceExt::serve` inside
//! `transport::stdio::StdioClient::connect` — there is no separate handshake
//! step to drive here for Phase 1.
//!
//! This module exists as the documented seam for Phase 2 (Streamable HTTP),
//! where the negotiation needs auth headers injected and may need to be driven
//! more explicitly (docs/plans §9).

/// The MCP protocol version this engine has been verified against.
/// **VERIFY UPSTREAM** before bumping: rmcp 2.2.0 pins/negotiates this itself.
pub const SUPPORTED_PROTOCOL_VERSION: &str = "2025-06-18";
