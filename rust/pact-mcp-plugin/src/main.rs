//! pact-mcp-plugin entry point.
//!
//! Two modes:
//! - default (no subcommand): the Pact plugin gRPC server. Mirrors the
//!   pact-protobuf-plugin bootstrap (docs/decisions/0001, plan §6): bind an
//!   ephemeral TCP port, print exactly one stdout line
//!   `{"port":<n>, "serverKey":"<key>"}`, then serve `PactPlugin` with every
//!   call's `authorization` metadata validated against the printed serverKey.
//! - `mock --pact <file> [--results <file>]`: run a REAL MCP server over this
//!   process's own stdio, synthesized from a pact file (plan task 1.8 / §7.2).
//!   A real MCP client spawns this as its stdio server.

use pact_mcp_plugin::mock::{write_results, MockServer};
use pact_mcp_plugin::proto::pact_plugin_server::PactPluginServer;
use pact_mcp_plugin::server::McpPlugin;
use rmcp::transport::io::stdio;
use rmcp::ServiceExt;
use tonic::transport::Server;
use uuid::Uuid;

#[derive(Clone)]
struct AuthInterceptor {
    server_key: String,
}

impl tonic::service::Interceptor for AuthInterceptor {
    fn call(&mut self, request: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        let provided = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok());

        match provided {
            Some(key) if key == self.server_key => Ok(request),
            _ => Err(tonic::Status::unauthenticated("invalid or missing serverKey")),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("mock") => return run_mock(&args[1..]).await,
        Some("verify") => return pact_mcp_plugin::cli::run_verify(&args[1..]).await,
        Some("compare") => return pact_mcp_plugin::cli::run_compare(&args[1..]),
        _ => {}
    }

    run_plugin_server().await
}

/// `mock --pact <file> [--results <file>]` — serve a synthesized MCP server over
/// stdio. Blocks until the client disconnects, then flushes results.
async fn run_mock(args: &[String]) -> anyhow::Result<()> {
    let mut pact_path: Option<String> = None;
    let mut results_path: Option<String> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--pact" => pact_path = it.next().cloned(),
            "--results" => results_path = it.next().cloned(),
            other => anyhow::bail!("unknown mock argument: {other}"),
        }
    }
    let pact_path = pact_path.ok_or_else(|| anyhow::anyhow!("mock mode requires --pact <file>"))?;

    let pact_json = std::fs::read_to_string(&pact_path)?;
    let mut mock = MockServer::from_pact_json(&pact_json)?;
    if let Some(path) = &results_path {
        // Flush results after each request so a consumer can read them without
        // racing the process exit.
        mock = mock.with_live_results_path(path.clone());
    }
    let results = mock.results_handle();

    // Serve MCP over this process's stdio; the client drives initialize/list/call.
    let running = mock.serve(stdio()).await?;
    let _ = running.waiting().await;

    // Final flush (also covers the no-request case).
    if let Some(path) = results_path {
        let snapshot = results.lock().map(|g| g.clone()).unwrap_or_default();
        write_results(&path, &snapshot)?;
    }
    Ok(())
}

async fn run_plugin_server() -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server_key = Uuid::new_v4().to_string();

    // The ONE stdout line the pact-plugins driver parses. Everything else goes
    // to stderr (tracing above is configured with_writer(stderr)).
    println!("{{\"port\":{}, \"serverKey\":\"{}\"}}", addr.port(), server_key);

    let plugin = McpPlugin::default();
    let interceptor = AuthInterceptor { server_key };

    Server::builder()
        .add_service(PactPluginServer::with_interceptor(plugin, interceptor))
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
        .await?;

    Ok(())
}
