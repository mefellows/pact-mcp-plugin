//! Task 0.3: a driver-style test that spawns the real compiled plugin binary,
//! parses its one-line stdout startup handshake (`{"port":<n>,"serverKey":"<key>"}`
//! — mirroring pact-protobuf-plugin's exact wire format, see ADR 0001), connects
//! a tonic client, and calls `InitPlugin` over real gRPC with the serverKey set
//! as the `authorization` metadata. Also asserts an invalid key is rejected.

use serde::Deserialize;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

#[derive(Deserialize)]
struct Handshake {
    port: u16,
    #[serde(rename = "serverKey")]
    server_key: String,
}

async fn spawn_plugin() -> (Child, Handshake) {
    let exe = env!("CARGO_BIN_EXE_pact-mcp-plugin");
    let mut child = Command::new(exe)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn pact-mcp-plugin binary");

    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .expect("timed out waiting for startup handshake line")
        .expect("reading stdout")
        .expect("plugin produced no stdout line");

    let handshake: Handshake = serde_json::from_str(&line)
        .unwrap_or_else(|e| panic!("startup line was not valid JSON handshake: {line:?}: {e}"));

    (child, handshake)
}

#[tokio::test]
async fn init_plugin_over_real_grpc_returns_the_mcp_catalogue() {
    let (mut child, handshake) = spawn_plugin().await;

    let endpoint = format!("http://127.0.0.1:{}", handshake.port);
    let channel = tonic::transport::Endpoint::new(endpoint)
        .expect("valid endpoint")
        .connect()
        .await
        .expect("failed to connect to spawned plugin");

    let mut client = pact_mcp_plugin::proto::pact_plugin_client::PactPluginClient::with_interceptor(
        channel,
        move |mut req: tonic::Request<()>| {
            req.metadata_mut().insert(
                "authorization",
                handshake.server_key.parse().unwrap(),
            );
            Ok(req)
        },
    );

    let response = client
        .init_plugin(pact_mcp_plugin::proto::InitPluginRequest {
            implementation: "conformance-test-driver".to_string(),
            version: "0.0.0".to_string(),
        })
        .await
        .expect("InitPlugin call failed")
        .into_inner();

    assert!(!response.catalogue.is_empty(), "expected a non-empty catalogue");
    assert!(response.catalogue.iter().any(|e| e.key == "mcp"), "expected an mcp catalogue entry");

    let _ = child.kill().await;
}

#[tokio::test]
async fn init_plugin_rejects_an_invalid_server_key() {
    let (mut child, handshake) = spawn_plugin().await;

    let endpoint = format!("http://127.0.0.1:{}", handshake.port);
    let channel = tonic::transport::Endpoint::new(endpoint)
        .expect("valid endpoint")
        .connect()
        .await
        .expect("failed to connect to spawned plugin");

    let mut client = pact_mcp_plugin::proto::pact_plugin_client::PactPluginClient::with_interceptor(
        channel,
        move |mut req: tonic::Request<()>| {
            req.metadata_mut().insert("authorization", "wrong-key".parse().unwrap());
            Ok(req)
        },
    );

    let result = client
        .init_plugin(pact_mcp_plugin::proto::InitPluginRequest {
            implementation: "conformance-test-driver".to_string(),
            version: "0.0.0".to_string(),
        })
        .await;

    assert!(result.is_err(), "expected an invalid serverKey to be rejected");
    assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);

    let _ = child.kill().await;
}

/// Task 1.8 — StartMockServer returns a spawnable stdio handoff, and
/// GetMockServerResults reads back results by key.
#[tokio::test]
async fn start_mock_server_returns_a_spawnable_stdio_handoff() {
    use pact_mcp_plugin::proto::*;
    let (mut child, handshake) = spawn_plugin().await;
    let channel = tonic::transport::Endpoint::new(format!("http://127.0.0.1:{}", handshake.port))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = pact_mcp_plugin::proto::pact_plugin_client::PactPluginClient::with_interceptor(
        channel,
        move |mut req: tonic::Request<()>| {
            req.metadata_mut().insert("authorization", handshake.server_key.parse().unwrap());
            Ok(req)
        },
    );

    let pact = serde_json::json!({
        "interactions": [ { "description": "x", "contents": { "mcp": {
            "operation": "tools/call",
            "request": { "name": "get_weather", "arguments": { "city": "Melbourne" } },
            "response": { "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false }
        }}}]
    })
    .to_string();

    let resp = client
        .start_mock_server(StartMockServerRequest {
            host_interface: String::new(),
            port: 0,
            tls: false,
            pact,
            test_context: None,
        })
        .await
        .expect("StartMockServer failed")
        .into_inner();

    let details = match resp.response {
        Some(start_mock_server_response::Response::Details(d)) => d,
        other => panic!("expected mock server details, got {other:?}"),
    };
    // The `address` carries the spawnable {command, args, env} handoff.
    let handoff: serde_json::Value = serde_json::from_str(&details.address).expect("handoff is JSON");
    assert_eq!(handoff["transport"], "stdio");
    assert!(handoff["args"].as_array().unwrap().iter().any(|a| a == "mock"));

    // No results yet (nothing has spawned the mock).
    let results = client
        .get_mock_server_results(MockServerRequest { server_key: details.key.clone() })
        .await
        .expect("GetMockServerResults failed")
        .into_inner();
    assert!(results.ok, "empty results should be ok");
    assert!(results.results.is_empty());

    // Shutdown returns results and cleans up.
    let shutdown = client
        .shutdown_mock_server(ShutdownMockServerRequest { server_key: details.key })
        .await
        .expect("ShutdownMockServer failed")
        .into_inner();
    assert!(shutdown.ok);

    let _ = child.kill().await;
}

/// Convert a serde_json Value into a prost_types Struct/Value, mimicking exactly
/// what pact core's FFI hands the plugin as `contentsConfig` (a
/// google.protobuf.Struct) on `ConfigureInteraction`.
fn json_to_prost_struct(v: &serde_json::Value) -> prost_types::Struct {
    prost_types::Struct {
        fields: v
            .as_object()
            .map(|m| m.iter().map(|(k, val)| (k.clone(), json_to_prost_value(val))).collect())
            .unwrap_or_default(),
    }
}

fn json_to_prost_value(v: &serde_json::Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match v {
        serde_json::Value::Null => Kind::NullValue(0),
        serde_json::Value::Bool(b) => Kind::BoolValue(*b),
        serde_json::Value::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Kind::StringValue(s.clone()),
        serde_json::Value::Array(a) => Kind::ListValue(prost_types::ListValue {
            values: a.iter().map(json_to_prost_value).collect(),
        }),
        serde_json::Value::Object(_) => Kind::StructValue(json_to_prost_struct(v)),
    };
    prost_types::Value { kind: Some(kind) }
}

/// Task A — direct-gRPC ConfigureInteraction contract test.
///
/// The live pact-js FFI round trip was BLOCKED (pact-core loads the plugin and
/// calls InitPlugin, but never routes `application/mcp+json` contents to our
/// ConfigureInteraction — see ADR 0004). This test instead exercises the exact
/// gRPC call pact core would make: it hands the plugin a `contentsConfig`
/// Struct (with an inline `matching(type, ...)` matcher) and asserts the plugin
/// returns the correct TWO-part synchronous-message response (request +
/// response), each with its own body/rules — the shape verified against
/// pact-protobuf-plugin's source.
#[tokio::test]
async fn configure_interaction_returns_two_part_sync_message_with_stripped_matchers() {
    let (mut child, handshake) = spawn_plugin().await;

    let endpoint = format!("http://127.0.0.1:{}", handshake.port);
    let channel = tonic::transport::Endpoint::new(endpoint)
        .expect("valid endpoint")
        .connect()
        .await
        .expect("failed to connect to spawned plugin");

    let mut client = pact_mcp_plugin::proto::pact_plugin_client::PactPluginClient::with_interceptor(
        channel,
        move |mut req: tonic::Request<()>| {
            req.metadata_mut().insert("authorization", handshake.server_key.parse().unwrap());
            Ok(req)
        },
    );

    let contents_config = serde_json::json!({
        "pact:content-type": "application/mcp+json",
        "mcp": {
            "operation": "tools/call",
            "request": { "name": "get_weather", "arguments": { "city": "Melbourne" } },
            "response": { "content": [ { "type": "text", "text": "matching(type, 'Sunny, 22C')" } ], "isError": false },
            "server": { "transport": "stdio" }
        }
    });

    let response = client
        .configure_interaction(pact_mcp_plugin::proto::ConfigureInteractionRequest {
            content_type: "application/mcp+json".to_string(),
            contents_config: Some(json_to_prost_struct(&contents_config)),
        })
        .await
        .expect("ConfigureInteraction call failed")
        .into_inner();

    assert_eq!(response.error, "", "expected no configure error");
    assert_eq!(response.interaction.len(), 2, "expected request + response parts");

    let request_part = response.interaction.iter().find(|i| i.part_name == "request").expect("request part");
    let response_part = response.interaction.iter().find(|i| i.part_name == "response").expect("response part");

    // Request part body carries the tools/call params, no rules.
    let request_body: serde_json::Value =
        serde_json::from_slice(request_part.contents.as_ref().unwrap().content.as_ref().unwrap()).unwrap();
    assert_eq!(request_body, serde_json::json!({ "name": "get_weather", "arguments": { "city": "Melbourne" } }));
    assert!(request_part.rules.is_empty(), "request part has no matchers");

    // Response part body has the DSL stripped to the example value, plus a rule.
    let response_body: serde_json::Value =
        serde_json::from_slice(response_part.contents.as_ref().unwrap().content.as_ref().unwrap()).unwrap();
    assert_eq!(
        response_body,
        serde_json::json!({ "content": [ { "type": "text", "text": "Sunny, 22C" } ], "isError": false })
    );
    assert!(response_part.rules.contains_key("$.content[0].text"), "expected a type matcher rule at $.content[0].text");
    assert_eq!(response_part.rules["$.content[0].text"].rule[0].r#type, "type");

    // operation is persisted in the interaction's pluginConfiguration.
    let op = response_part
        .plugin_configuration
        .as_ref()
        .and_then(|pc| pc.interaction_configuration.as_ref())
        .and_then(|s| s.fields.get("operation"))
        .and_then(|v| match &v.kind {
            Some(prost_types::value::Kind::StringValue(s)) => Some(s.clone()),
            _ => None,
        });
    assert_eq!(op.as_deref(), Some("tools/call"));

    let _ = child.kill().await;
}
