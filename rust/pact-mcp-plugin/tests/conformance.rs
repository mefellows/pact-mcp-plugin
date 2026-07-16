//! Conformance harness: loads every fixture under docs/spec/conformance/*.json
//! and asserts the engine's matcher agrees with the pinned expected result.
//! See docs/spec/conformance/README.md (assertion contract) — this is the
//! anti-divergence gate; a failure here is a release blocker.

use pact_mcp_plugin::content::{compare_response, Rules};
use pact_mcp_plugin::mcp::model::Operation;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn conformance_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/conformance")
}

#[test]
fn all_conformance_fixtures_pass() {
    let dir = conformance_dir();
    let pattern = dir.join("*.json");
    let files: Vec<PathBuf> = glob::glob(pattern.to_str().unwrap())
        .expect("valid glob pattern")
        .filter_map(Result::ok)
        .collect();

    assert!(!files.is_empty(), "expected to find conformance fixtures at {}", dir.display());

    let mut failures = Vec::new();

    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let raw = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("reading {name}: {e}"));
        let fixture: Value = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parsing {name}: {e}"));

        let operation_str = fixture["operation"].as_str().expect("operation field");
        let operation = Operation::parse(operation_str)
            .unwrap_or_else(|| panic!("{name}: unknown operation {operation_str}"));

        let expected_response = &fixture["interaction"]["mcp"]["response"];
        let actual = &fixture["actual"];
        let response_rules = fixture["interaction"]["matchingRules"].get("response");
        let rules = Rules::new(response_rules);

        let result = compare_response(operation, expected_response, actual, &rules);

        let expected_match = fixture["expected"]["match"].as_bool().expect("expected.match");
        let expected_paths: BTreeSet<String> = fixture["expected"]["mismatchPaths"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
            .unwrap_or_default();

        let actual_match = result.is_match();
        let actual_paths = result.mismatch_paths();

        if actual_match != expected_match || actual_paths != expected_paths {
            failures.push(format!(
                "{name}: expected match={expected_match} paths={expected_paths:?}, got match={actual_match} paths={actual_paths:?} mismatches={:?}",
                result.mismatches
            ));
        }

        // Optional messageContains assertions, if a fixture opts in.
        if let Some(message_contains) = fixture["expected"].get("messageContains").and_then(Value::as_array) {
            for substr in message_contains {
                let substr = substr.as_str().unwrap();
                let found = result.mismatches.iter().any(|m| m.message.contains(substr));
                if !found {
                    failures.push(format!("{name}: expected some mismatch message to contain {substr:?}"));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "conformance failures ({} of {} fixtures):\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
