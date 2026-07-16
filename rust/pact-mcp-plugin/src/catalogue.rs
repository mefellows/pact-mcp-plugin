//! InitPlugin catalogue entries this plugin provides.

use crate::proto::catalogue_entry::EntryType;
use crate::proto::CatalogueEntry;
use std::collections::HashMap;

/// Catalogue entries advertised on `InitPlugin`.
///
/// Phase 1 scope: a content matcher/generator for `application/mcp+json`, and a
/// `mcp-stdio` transport entry. `mcp-http` and resources/prompts follow in later
/// phases (see docs/plans/pact-mcp-plugin-implementation-plan.md §11).
/// The content type this plugin owns, registered so the pact plugin driver
/// routes it to us. The driver matches this value as a **regex anchored at both
/// ends** (`^(?:<value>)$`) against the incoming content type, so regex
/// metacharacters must be escaped for a literal match — critically the `+` in
/// the `+json` structured-syntax suffix, which would otherwise be a
/// one-or-more quantifier (`mcp+` = "mc" then one-or-more "p") and never match
/// the literal `application/mcp+json`. This was the root cause of pact-core not
/// invoking ConfigureInteraction (see ADR 0004). pact-protobuf-plugin never hit
/// this because its content types (`application/protobuf;application/grpc`)
/// contain no regex metacharacters.
pub const CONTENT_TYPE_PATTERN: &str = r"application/mcp\+json";

pub fn entries() -> Vec<CatalogueEntry> {
    let mut content_values = HashMap::new();
    content_values.insert("content-types".to_string(), CONTENT_TYPE_PATTERN.to_string());

    vec![
        CatalogueEntry {
            r#type: EntryType::ContentMatcher as i32,
            key: "mcp".to_string(),
            values: content_values.clone(),
        },
        CatalogueEntry {
            r#type: EntryType::ContentGenerator as i32,
            key: "mcp".to_string(),
            values: content_values,
        },
        CatalogueEntry {
            r#type: EntryType::Transport as i32,
            key: "mcp-stdio".to_string(),
            values: HashMap::new(),
        },
    ]
}
