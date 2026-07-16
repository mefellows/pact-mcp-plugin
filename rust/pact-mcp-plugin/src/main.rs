//! pact-mcp-plugin entry point.
//!
//! Mirrors the pact-protobuf-plugin bootstrap pattern (see
//! docs/decisions/0001-vendored-plugin-proto.md and the plan §6):
//! bind an ephemeral TCP port, print exactly one stdout line
//! `{"port":<n>, "serverKey":"<key>"}`, then serve the PactPlugin gRPC service
//! over that port with every call's `authorization` metadata validated against
//! the printed serverKey.

use pact_mcp_plugin::proto::pact_plugin_server::PactPluginServer;
use pact_mcp_plugin::server::McpPlugin;
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

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server_key = Uuid::new_v4().to_string();

    // The ONE stdout line the pact-plugins driver parses. Everything else goes
    // to stderr (tracing above is configured with_writer(stderr)).
    println!("{{\"port\":{}, \"serverKey\":\"{}\"}}", addr.port(), server_key);

    let plugin = McpPlugin;
    let interceptor = AuthInterceptor { server_key };

    Server::builder()
        .add_service(PactPluginServer::with_interceptor(plugin, interceptor))
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
        .await?;

    Ok(())
}
