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
