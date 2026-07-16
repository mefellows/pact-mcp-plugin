//! InitPlugin catalogue entries this plugin provides.

use crate::proto::catalogue_entry::EntryType;
use crate::proto::CatalogueEntry;
use std::collections::HashMap;

/// Catalogue entries advertised on `InitPlugin`.
///
/// Phase 1 scope: a content matcher/generator for `application/mcp+json`, and a
/// `mcp-stdio` transport entry. `mcp-http` and resources/prompts follow in later
/// phases (see docs/plans/pact-mcp-plugin-implementation-plan.md §11).
pub fn entries() -> Vec<CatalogueEntry> {
    let mut content_values = HashMap::new();
    content_values.insert("content-types".to_string(), "application/mcp+json".to_string());

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
