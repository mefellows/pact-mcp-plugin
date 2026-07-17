//! Thin CLI subcommands the TypeScript adapter delegates to, so that ALL
//! matching/verification stays in this Rust engine (no TS-side matching) without
//! the adapter having to embed a gRPC client + the plugin proto. Each subcommand
//! calls exactly the same engine functions the gRPC `CompareContents` /
//! `VerifyInteraction` methods call.
//!
//! - `compare --fixture <file>`: run one conformance-style `{interaction, actual}`
//!   through `content::compare_response`; print `{"match":bool,"mismatchPaths":[...]}`.
//!   (Drives the §4.3 conformance gate from TypeScript.)
//! - `verify --pact <file> --command <cmd> [--arg <a>...]`: verify every mcp
//!   interaction in a pact against a real stdio MCP server, reusing
//!   `verify::verify_interaction_stdio`; print a JSON result per interaction.

use crate::auth::resolve_config;
use crate::content::{compare_response, Rules};
use crate::mcp::model::Operation;
use crate::server::{interaction_from_value, response_matching_rules};
use crate::verify::{verify_interaction_http, verify_interaction_stdio, StdioServerConfig};
use serde_json::{json, Value};
use std::collections::HashMap;

/// `compare --fixture <file>` (also accepts the fixture JSON on stdin if no file).
pub fn run_compare(args: &[String]) -> anyhow::Result<()> {
    let mut fixture_path: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--fixture" => fixture_path = it.next().cloned(),
            other => anyhow::bail!("unknown compare argument: {other}"),
        }
    }

    let raw = match fixture_path {
        Some(p) => std::fs::read_to_string(p)?,
        None => {
            use std::io::Read;
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        }
    };
    let fixture: Value = serde_json::from_str(&raw)?;

    let operation_str = fixture["operation"].as_str().unwrap_or("tools/call");
    let operation = Operation::parse(operation_str)
        .ok_or_else(|| anyhow::anyhow!("unknown operation {operation_str}"))?;
    let expected = &fixture["interaction"]["mcp"]["response"];
    let actual = &fixture["actual"];
    let response_rules = fixture["interaction"]["matchingRules"].get("response");
    let rules = Rules::new(response_rules);

    let result = compare_response(operation, expected, actual, &rules);
    let mut paths: Vec<String> = result.mismatch_paths().into_iter().collect();
    paths.sort();

    let out = json!({ "match": result.is_match(), "mismatchPaths": paths });
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

/// `verify --pact <file>` with EITHER:
///   - stdio: `--command <cmd> [--arg <a>...]`
///   - http:  `--url <url> [--auth <json>]`  (auth JSON per auth::from_config)
pub async fn run_verify(args: &[String]) -> anyhow::Result<()> {
    let mut pact_path: Option<String> = None;
    let mut command: Option<String> = None;
    let mut cmd_args: Vec<String> = Vec::new();
    let mut url: Option<String> = None;
    let mut auth_json: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--pact" => pact_path = it.next().cloned(),
            "--command" => command = it.next().cloned(),
            "--arg" => {
                if let Some(v) = it.next() {
                    cmd_args.push(v.clone());
                }
            }
            "--url" => url = it.next().cloned(),
            "--auth" => auth_json = it.next().cloned(),
            other => anyhow::bail!("unknown verify argument: {other}"),
        }
    }
    let pact_path = pact_path.ok_or_else(|| anyhow::anyhow!("verify requires --pact <file>"))?;

    let raw = std::fs::read_to_string(&pact_path)?;
    let pact: Value = serde_json::from_str(&raw)?;
    let interactions = pact["interactions"].as_array().cloned().unwrap_or_default();

    enum Target {
        Stdio(StdioServerConfig),
        Http { url: String, auth: crate::auth::ResolvedAuth },
    }
    let target = match (url, command) {
        (Some(url), _) => {
            let auth_value = match &auth_json {
                Some(s) => Some(serde_json::from_str::<Value>(s)?),
                None => None,
            };
            let auth = resolve_config(auth_value.as_ref())?;
            Target::Http { url, auth }
        }
        (None, Some(command)) => Target::Stdio(StdioServerConfig { command, args: cmd_args, env: HashMap::new() }),
        (None, None) => anyhow::bail!("verify requires either --url <url> (http) or --command <cmd> (stdio)"),
    };

    let mut results = Vec::new();
    for interaction in &interactions {
        let description = interaction["description"].as_str().unwrap_or("").to_string();
        let mcp = match interaction_from_value(interaction) {
            Ok(m) => m,
            Err(_) => continue, // not an mcp interaction
        };
        let rules = response_matching_rules(interaction);
        let verify_result = match &target {
            Target::Stdio(server) => verify_interaction_stdio(&mcp, server, rules.as_ref()).await,
            Target::Http { url, auth } => verify_interaction_http(&mcp, url, auth, rules.as_ref()).await,
        };
        match verify_result {
            Ok(match_result) => {
                let mismatches: Vec<Value> = match_result
                    .mismatches
                    .iter()
                    .map(|m| json!({ "path": m.path, "message": m.message }))
                    .collect();
                results.push(json!({
                    "description": description,
                    "success": match_result.is_match(),
                    "mismatches": mismatches,
                }));
            }
            Err(e) => results.push(json!({
                "description": description,
                "success": false,
                "error": e.to_string(),
            })),
        }
    }

    let success = results.iter().all(|r| r["success"].as_bool().unwrap_or(false));
    let out = json!({ "success": success, "interactions": results });
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}
