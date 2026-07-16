//! Task 1.8 demo: a REAL `@modelcontextprotocol/sdk` client (Node) spawns our
//! plugin's `mock` stdio mode as its MCP server, does the handshake, lists
//! tools, and calls a tool. Asserts:
//!  - a configured call returns the configured response, and
//!  - an unexpected call is reported as a protocol error AND recorded as a
//!    mismatch in the results file.
//!
//! Requires `node` on PATH and `examples/consumer-stdio-mock/node_modules`
//! installed (`cd examples/consumer-stdio-mock && npm install`).

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run_client(city: &str, results_path: &PathBuf) -> Value {
    run_client_with_pact("pacts/weather-agent-weather-mcp.json", city, results_path)
}

fn run_client_with_pact(pact_rel: &str, city: &str, results_path: &PathBuf) -> Value {
    let root = repo_root();
    let binary = env!("CARGO_BIN_EXE_pact-mcp-plugin");
    let client = root.join("examples/consumer-stdio-mock/client.mjs");
    let pact = root.join("examples/consumer-stdio-mock").join(pact_rel);

    let output = Command::new("node")
        .arg(&client)
        .arg(binary)
        .arg(&pact)
        .arg(results_path)
        .arg(city)
        .current_dir(root.join("examples/consumer-stdio-mock"))
        .output()
        .expect("failed to run node client");

    assert!(
        output.status.success(),
        "node client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().last().expect("client produced no output");
    serde_json::from_str(line).unwrap_or_else(|e| panic!("client output not JSON: {line:?}: {e}"))
}

fn read_results(path: &PathBuf) -> Value {
    let raw = std::fs::read_to_string(path).unwrap_or_else(|_| "[]".to_string());
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn real_mcp_client_gets_configured_response_from_the_stdio_mock() {
    let results = std::env::temp_dir().join(format!("mcp-mock-pos-{}.json", std::process::id()));
    let out = run_client("Melbourne", &results);

    // Tool was advertised via tools/list.
    assert_eq!(out["tools"], serde_json::json!(["get_weather"]));

    // The configured response came back.
    assert_eq!(
        out["call"],
        serde_json::json!({ "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false })
    );

    // Results recorded a successful tools/call.
    let recorded = read_results(&results);
    let call = recorded
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["path"] == "tools/call")
        .expect("a tools/call result");
    assert!(call.get("error").is_none(), "expected no error, got {call:?}");
    let _ = std::fs::remove_file(&results);
}

#[test]
fn unexpected_call_is_reported_and_recorded_as_a_mismatch() {
    let results = std::env::temp_dir().join(format!("mcp-mock-neg-{}.json", std::process::id()));
    let out = run_client("Atlantis", &results);

    // The client received a protocol error rather than a response.
    assert!(out.get("call").is_none(), "did not expect a successful call: {out:?}");
    assert!(
        out["error"].as_str().unwrap_or_default().contains("no matching interaction"),
        "expected a no-match error, got {out:?}"
    );

    // The mock recorded a mismatch at the differing argument.
    let recorded = read_results(&results);
    let call = recorded
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["path"] == "tools/call")
        .expect("a tools/call result");
    assert!(call["error"].is_string(), "expected an error to be recorded");
    let mismatches = call["mismatches"].as_array().cloned().unwrap_or_default();
    assert!(
        mismatches.iter().any(|m| m == "$.arguments.city"),
        "expected a mismatch at $.arguments.city, got {mismatches:?}"
    );
    let _ = std::fs::remove_file(&results);
}

#[test]
fn request_side_type_matcher_lets_the_real_client_call_with_a_different_city() {
    // The anycity pact carries a request-side `matching(type)` on `city`, so a
    // real client calling with a city other than the example still matches and
    // gets the configured response.
    let results = std::env::temp_dir().join(format!("mcp-mock-anycity-{}.json", std::process::id()));
    let out = run_client_with_pact("pacts/weather-agent-anycity.json", "Reykjavik", &results);

    assert!(out.get("error").is_none(), "expected a match via the type matcher, got {out:?}");
    assert_eq!(
        out["call"],
        serde_json::json!({ "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false })
    );
    let _ = std::fs::remove_file(&results);
}
