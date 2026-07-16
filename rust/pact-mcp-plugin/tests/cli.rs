//! Contract tests for the `compare` / `verify` CLI subcommands the TS adapter
//! delegates to. Asserts the JSON stdout shape (logs go to stderr).

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_pact-mcp-plugin")
}

fn last_json_line(stdout: &[u8]) -> serde_json::Value {
    let s = String::from_utf8_lossy(stdout);
    let line = s.lines().filter(|l| l.trim_start().starts_with('{')).last().expect("a JSON line");
    serde_json::from_str(line).expect("valid JSON")
}

#[test]
fn compare_cli_reports_a_pass_fixture() {
    let fixture = repo_root().join("docs/spec/conformance/tools-call-text-type-pass.json");
    let out = Command::new(bin())
        .args(["compare", "--fixture", fixture.to_str().unwrap()])
        .output()
        .expect("run compare");
    assert!(out.status.success());
    let v = last_json_line(&out.stdout);
    assert_eq!(v["match"], true);
    assert_eq!(v["mismatchPaths"], serde_json::json!([]));
}

#[test]
fn compare_cli_reports_a_mismatch_fixture_with_paths() {
    let fixture = repo_root().join("docs/spec/conformance/tools-call-text-mismatch.json");
    let out = Command::new(bin())
        .args(["compare", "--fixture", fixture.to_str().unwrap()])
        .output()
        .expect("run compare");
    let v = last_json_line(&out.stdout);
    assert_eq!(v["match"], false);
    assert_eq!(v["mismatchPaths"], serde_json::json!(["$.content[0].text"]));
}

#[test]
fn verify_cli_verifies_the_real_pact_js_pact_against_the_fixture_server() {
    let pact = repo_root().join("examples/ts-roundtrip/pacts-committed/weather-agent-weather-mcp.json");
    let server = repo_root().join("examples/fixtures/weather-server.mjs");
    let out = Command::new(bin())
        .args([
            "verify",
            "--pact",
            pact.to_str().unwrap(),
            "--command",
            "node",
            "--arg",
            server.to_str().unwrap(),
        ])
        .output()
        .expect("run verify");
    let v = last_json_line(&out.stdout);
    assert_eq!(v["success"], true, "verify output: {v}");
    assert_eq!(v["interactions"][0]["success"], true);
}
