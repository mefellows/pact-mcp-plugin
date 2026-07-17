//! `PactPlugin` gRPC service dispatch.
//!
//! Phase 1 scope: `InitPlugin`, `UpdateCatalogue`, `ConfigureInteraction`,
//! `CompareContents`, `GenerateContent`, `PrepareInteractionForVerification`,
//! `VerifyInteraction` (stdio only) are implemented for real.
//!
//! `StartMockServer` / `ShutdownMockServer` / `GetMockServerResults` support
//! BOTH mock transports (§7.2):
//!  - stdio (default): persist the pact and return a spawnable `{command, args,
//!    env}` handoff (the client execs the `mock` CLI as its stdio server); the
//!    mock writes results to a file Get/Shutdown read back.
//!  - http (`testContext.transport == "http"`): stand up a loopback Streamable
//!    HTTP MCP mock on an ephemeral port and return its URL; a real HTTP client
//!    connects to it. Results are held in-memory.

use crate::catalogue;
use crate::config::{configure_interaction, rules_value};
use crate::content::{compare_response, Rules};
use crate::mock::MockResult;
use crate::proto::pact_plugin_server::PactPlugin;
use crate::proto::*;
use crate::verify::{verify_interaction_stdio, StdioServerConfig};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tonic::{Request, Response, Status};

/// A running mock session, keyed by server key.
enum MockSession {
    /// stdio: pact + results files the spawnable mock CLI writes to.
    Stdio { dir: PathBuf, results_path: PathBuf },
    /// http: a live loopback HTTP mock, its results handle + shutdown token.
    Http {
        results: Arc<Mutex<Vec<MockResult>>>,
        shutdown: tokio_util::sync::CancellationToken,
    },
}

#[derive(Default, Clone)]
pub struct McpPlugin {
    sessions: Arc<Mutex<HashMap<String, MockSession>>>,
}

fn struct_to_value(s: &prost_types::Struct) -> serde_json::Value {
    // prost_types::Struct <-> serde_json::Value conversion. prost-types 0.13
    // Struct/Value implement serde via the `Serialize`/`Deserialize` derived
    // for the generated types when using tonic-build's default codec (JSON
    // pb well-known type mapping is NOT automatic for prost, so we convert
    // field-by-field).
    let mut map = serde_json::Map::new();
    for (k, v) in &s.fields {
        map.insert(k.clone(), prost_value_to_json(v));
    }
    serde_json::Value::Object(map)
}

fn prost_value_to_json(v: &prost_types::Value) -> serde_json::Value {
    use prost_types::value::Kind;
    match &v.kind {
        None | Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::NumberValue(n)) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Some(Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(Kind::StructValue(s)) => struct_to_value(s),
        Some(Kind::ListValue(l)) => serde_json::Value::Array(l.values.iter().map(prost_value_to_json).collect()),
    }
}

#[tonic::async_trait]
impl PactPlugin for McpPlugin {
    async fn init_plugin(
        &self,
        _request: Request<InitPluginRequest>,
    ) -> Result<Response<InitPluginResponse>, Status> {
        Ok(Response::new(InitPluginResponse {
            catalogue: catalogue::entries(),
        }))
    }

    async fn update_catalogue(
        &self,
        _request: Request<Catalogue>,
    ) -> Result<Response<()>, Status> {
        // No cross-plugin catalogue dependencies in Phase 1.
        Ok(Response::new(()))
    }

    async fn compare_contents(
        &self,
        request: Request<CompareContentsRequest>,
    ) -> Result<Response<CompareContentsResponse>, Status> {
        let req = request.into_inner();

        let expected = req.expected.as_ref().ok_or_else(|| Status::invalid_argument("missing expected body"))?;
        let actual = req.actual.as_ref().ok_or_else(|| Status::invalid_argument("missing actual body"))?;

        // The expected body is a single part (the response object) per the
        // two-part sync-message model; operation comes from the interaction's
        // pluginConfiguration. Rules are already keyed `$.<path>`.
        let expected_value: serde_json::Value = decode_body(expected)
            .map_err(|e| Status::invalid_argument(format!("invalid expected mcp body: {e}")))?;
        let actual_value: serde_json::Value = decode_body(actual)
            .map_err(|e| Status::invalid_argument(format!("invalid actual mcp body: {e}")))?;

        let operation = req
            .plugin_configuration
            .as_ref()
            .and_then(|pc| pc.interaction_configuration.as_ref())
            .and_then(|s| s.fields.get("operation"))
            .and_then(|v| match &v.kind {
                Some(prost_types::value::Kind::StringValue(s)) => crate::mcp::model::Operation::parse(s),
                _ => None,
            })
            .unwrap_or(crate::mcp::model::Operation::ToolsCall);

        let rules_value = rules_value(&req.rules);
        let rules = Rules::new(Some(&rules_value));

        let result = compare_response(operation, &expected_value, &actual_value, &rules);

        let mut results = HashMap::new();
        if !result.mismatches.is_empty() {
            let mismatches: Vec<ContentMismatch> = result
                .mismatches
                .iter()
                .map(|m| ContentMismatch {
                    expected: Some(serde_json::to_vec(&m.expected).unwrap_or_default().into()),
                    actual: Some(serde_json::to_vec(&m.actual).unwrap_or_default().into()),
                    mismatch: m.message.clone(),
                    path: m.path.clone(),
                    diff: String::new(),
                    mismatch_type: "body".to_string(),
                })
                .collect();
            results.insert("$".to_string(), ContentMismatches { mismatches });
        }

        Ok(Response::new(CompareContentsResponse {
            error: String::new(),
            type_mismatch: None,
            results,
        }))
    }

    async fn configure_interaction(
        &self,
        request: Request<ConfigureInteractionRequest>,
    ) -> Result<Response<ConfigureInteractionResponse>, Status> {
        let req = request.into_inner();
        let contents_config = req
            .contents_config
            .as_ref()
            .map(struct_to_value)
            .unwrap_or(serde_json::Value::Null);

        let configured = match configure_interaction(&contents_config) {
            Ok(c) => c,
            Err(e) => {
                return Ok(Response::new(ConfigureInteractionResponse {
                    error: e.to_string(),
                    interaction: vec![],
                    plugin_configuration: None,
                }))
            }
        };

        // A synchronous-message plugin MUST return two InteractionResponse parts
        // (request + response), each with part_name set and its own body/rules
        // rooted at `$`. This was VERIFIED via the live pact-js round trip: a
        // single merged part made pact core report "Retrieved an empty message"
        // for the request. See ADR 0004.
        let interaction_config = crate::config::interaction_config_struct(configured.operation, &configured.server);
        let plugin_configuration = Some(PluginConfiguration {
            interaction_configuration: Some(interaction_config),
            pact_configuration: None,
        });

        let request_part = InteractionResponse {
            contents: Some(Body {
                content_type: crate::mcp::model::CONTENT_TYPE.to_string(),
                content: Some(configured.request.body_bytes),
                content_type_hint: body::ContentTypeHint::Text as i32,
            }),
            rules: configured.request.rules,
            generators: configured.request.generators,
            message_metadata: None,
            plugin_configuration: plugin_configuration.clone(),
            interaction_markup: String::new(),
            interaction_markup_type: 0,
            part_name: "request".to_string(),
            metadata_rules: HashMap::new(),
            metadata_generators: HashMap::new(),
        };

        let response_part = InteractionResponse {
            contents: Some(Body {
                content_type: crate::mcp::model::CONTENT_TYPE.to_string(),
                content: Some(configured.response.body_bytes),
                content_type_hint: body::ContentTypeHint::Text as i32,
            }),
            rules: configured.response.rules,
            generators: configured.response.generators,
            message_metadata: None,
            plugin_configuration: plugin_configuration.clone(),
            interaction_markup: String::new(),
            interaction_markup_type: 0,
            part_name: "response".to_string(),
            metadata_rules: HashMap::new(),
            metadata_generators: HashMap::new(),
        };

        Ok(Response::new(ConfigureInteractionResponse {
            error: String::new(),
            interaction: vec![request_part, response_part],
            plugin_configuration,
        }))
    }

    async fn generate_content(
        &self,
        request: Request<GenerateContentRequest>,
    ) -> Result<Response<GenerateContentResponse>, Status> {
        // No generators are supported in Phase 1 (docs/spec/interaction-schema.md
        // doesn't define any MCP-specific generators yet) — pass contents through
        // unchanged.
        let req = request.into_inner();
        Ok(Response::new(GenerateContentResponse {
            contents: req.contents,
        }))
    }

    async fn start_mock_server(
        &self,
        request: Request<StartMockServerRequest>,
    ) -> Result<Response<StartMockServerResponse>, Status> {
        let req = request.into_inner();
        let key = uuid::Uuid::new_v4().to_string();

        let transport = req
            .test_context
            .as_ref()
            .map(struct_to_value)
            .and_then(|v| v.get("transport").and_then(|t| t.as_str()).map(str::to_string))
            .unwrap_or_else(|| "stdio".to_string());

        if transport == "http" {
            // Stand up a loopback Streamable HTTP MCP mock on an ephemeral port.
            let mock = match crate::mock::MockServer::from_pact_json(&req.pact) {
                Ok(m) => m,
                Err(e) => return Ok(mock_start_error(e.to_string())),
            };
            match crate::mock::serve_http(mock).await {
                Ok(handle) => {
                    let url = format!("http://{}/", handle.addr);
                    let port = handle.addr.port() as u32;
                    self.sessions.lock().unwrap().insert(
                        key.clone(),
                        MockSession::Http { results: handle.results, shutdown: handle.shutdown },
                    );
                    return Ok(Response::new(StartMockServerResponse {
                        response: Some(start_mock_server_response::Response::Details(MockServerDetails {
                            key,
                            port,
                            address: url,
                        })),
                    }));
                }
                Err(e) => return Ok(mock_start_error(e.to_string())),
            }
        }

        // stdio: return a spawnable `{command, args, env}` handoff (§7.2). The
        // proto has no first-class field for a spawn handoff, so it is JSON in
        // `address`; `port` is 0.
        let dir = std::env::temp_dir().join(format!("pact-mcp-mock-{key}"));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Ok(mock_start_error(e.to_string()));
        }
        let pact_path = dir.join("pact.json");
        let results_path = dir.join("results.json");
        if let Err(e) = std::fs::write(&pact_path, &req.pact) {
            return Ok(mock_start_error(e.to_string()));
        }

        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "pact-mcp-plugin".to_string());
        let handoff = serde_json::json!({
            "transport": "stdio",
            "command": exe,
            "args": ["mock", "--pact", pact_path.to_string_lossy(), "--results", results_path.to_string_lossy()],
            "env": {},
        });

        self.sessions.lock().unwrap().insert(key.clone(), MockSession::Stdio { dir, results_path });

        Ok(Response::new(StartMockServerResponse {
            response: Some(start_mock_server_response::Response::Details(MockServerDetails {
                key,
                port: 0,
                address: handoff.to_string(),
            })),
        }))
    }

    async fn shutdown_mock_server(
        &self,
        request: Request<ShutdownMockServerRequest>,
    ) -> Result<Response<ShutdownMockServerResponse>, Status> {
        let key = request.into_inner().server_key;
        let session = self.sessions.lock().unwrap().remove(&key);
        let results = session_results(&session);
        let ok = results.iter().all(|r| r.error.is_empty() && r.mismatches.is_empty());
        match session {
            Some(MockSession::Stdio { dir, .. }) => {
                let _ = std::fs::remove_dir_all(&dir);
            }
            Some(MockSession::Http { shutdown, .. }) => shutdown.cancel(),
            None => {}
        }
        Ok(Response::new(ShutdownMockServerResponse { ok, results }))
    }

    async fn get_mock_server_results(
        &self,
        request: Request<MockServerRequest>,
    ) -> Result<Response<MockServerResults>, Status> {
        let key = request.into_inner().server_key;
        let guard = self.sessions.lock().unwrap();
        let session = guard
            .get(&key)
            .ok_or_else(|| Status::not_found(format!("no mock session for key `{key}`")))?;
        let results = session_ref_results(session);
        let ok = results.iter().all(|r| r.error.is_empty() && r.mismatches.is_empty());
        Ok(Response::new(MockServerResults { ok, results }))
    }

    async fn prepare_interaction_for_verification(
        &self,
        request: Request<VerificationPreparationRequest>,
    ) -> Result<Response<VerificationPreparationResponse>, Status> {
        let req = request.into_inner();
        let interaction = extract_interaction(&req.pact, &req.interaction_key)
            .map_err(|e| Status::invalid_argument(e))?;

        let body_bytes = serde_json::to_vec(&interaction.request)
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(VerificationPreparationResponse {
            response: Some(verification_preparation_response::Response::InteractionData(InteractionData {
                body: Some(Body {
                    content_type: crate::mcp::model::CONTENT_TYPE.to_string(),
                    content: Some(body_bytes),
                    content_type_hint: body::ContentTypeHint::Text as i32,
                }),
                metadata: HashMap::new(),
            })),
        }))
    }

    async fn verify_interaction(
        &self,
        request: Request<VerifyInteractionRequest>,
    ) -> Result<Response<VerifyInteractionResponse>, Status> {
        let req = request.into_inner();
        let interaction = extract_interaction(&req.pact, &req.interaction_key)
            .map_err(|e| Status::invalid_argument(e))?;

        let config = req.config.as_ref().map(struct_to_value).unwrap_or(serde_json::Value::Null);
        let command = config
            .get("command")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Status::invalid_argument("verification config.command is required for stdio verification"))?
            .to_string();
        let args: Vec<String> = config
            .get("args")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();

        let server_config = StdioServerConfig { command, args, env: HashMap::new() };

        let match_result = verify_interaction_stdio(&interaction, &server_config, None)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let success = match_result.is_match();
        let mismatches = match_result
            .mismatches
            .iter()
            .map(|m| VerificationResultItem {
                result: Some(verification_result_item::Result::Mismatch(ContentMismatch {
                    expected: Some(serde_json::to_vec(&m.expected).unwrap_or_default().into()),
                    actual: Some(serde_json::to_vec(&m.actual).unwrap_or_default().into()),
                    mismatch: m.message.clone(),
                    path: m.path.clone(),
                    diff: String::new(),
                    mismatch_type: "body".to_string(),
                })),
            })
            .collect();

        Ok(Response::new(VerifyInteractionResponse {
            response: Some(verify_interaction_response::Response::Result(VerificationResult {
                success,
                response_data: None,
                mismatches,
                output: vec![],
            })),
        }))
    }
}

fn mock_start_error(message: String) -> Response<StartMockServerResponse> {
    Response::new(StartMockServerResponse {
        response: Some(start_mock_server_response::Response::Error(message)),
    })
}

/// Map a `mock::MockResult` to the proto `MockServerResult`.
fn to_proto_result(r: MockResult) -> MockServerResult {
    MockServerResult {
        path: r.path,
        error: r.error.unwrap_or_default(),
        mismatches: r
            .mismatches
            .into_iter()
            .map(|p| ContentMismatch {
                expected: None,
                actual: None,
                mismatch: format!("no interaction matched at {p}"),
                path: p,
                diff: String::new(),
                mismatch_type: "body".to_string(),
            })
            .collect(),
    }
}

/// Results for a session (owned lookup, for shutdown which removes the entry).
fn session_results(session: &Option<MockSession>) -> Vec<MockServerResult> {
    match session {
        Some(s) => session_ref_results(s),
        None => vec![],
    }
}

/// Results for a session reference.
fn session_ref_results(session: &MockSession) -> Vec<MockServerResult> {
    match session {
        MockSession::Stdio { results_path, .. } => read_mock_results(results_path),
        MockSession::Http { results, .. } => results
            .lock()
            .map(|g| g.iter().cloned().map(to_proto_result).collect())
            .unwrap_or_default(),
    }
}

/// Read the stdio mock CLI's results file and map to proto. A missing file
/// (mock still running / no requests yet) yields an empty list.
fn read_mock_results(path: &std::path::Path) -> Vec<MockServerResult> {
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let parsed: Vec<MockResult> = serde_json::from_str(&raw).unwrap_or_default();
    parsed.into_iter().map(to_proto_result).collect()
}

fn decode_body<T: serde::de::DeserializeOwned>(body: &Body) -> Result<T, String> {
    let content = body.content.as_ref().ok_or("body has no content")?;
    serde_json::from_slice(content).map_err(|e| e.to_string())
}

/// Look up an `mcp` interaction by key from a full pact-as-JSON document.
/// Phase 1 assumption: interactions are looked up by their `description` field
/// as the "interaction key" (pact core's exact key convention for V4 plugin
/// interactions was not independently re-verified in this task — see ADR 0004).
fn extract_interaction(pact_json: &str, interaction_key: &str) -> Result<crate::mcp::model::McpInteraction, String> {
    let pact: serde_json::Value = serde_json::from_str(pact_json).map_err(|e| e.to_string())?;
    let interactions = pact.get("interactions").and_then(serde_json::Value::as_array).ok_or("pact has no interactions")?;

    let interaction = interactions
        .iter()
        .find(|i| {
            i.get("description").and_then(serde_json::Value::as_str) == Some(interaction_key)
                || i.get("key").and_then(serde_json::Value::as_str) == Some(interaction_key)
        })
        .ok_or_else(|| format!("no interaction found for key `{interaction_key}`"))?;

    interaction_from_value(interaction)
}

/// Reconstruct an `McpInteraction` from a persisted interaction, tolerating two
/// shapes:
///  1. Our single-fragment `examples/` shape: `contents.mcp = { operation, request, response, ... }`.
///  2. The REAL pact-core two-part sync-message shape, now CONFIRMED by a live
///     pact-js round trip (see ADR 0004 and the committed evidence pact at
///     `examples/ts-roundtrip/pacts/`): the request body is
///     `request.contents.content`, the response body is
///     `response[0].contents.content` (response is an array of parts), and
///     `operation`/`server` live in `pluginConfiguration.<pluginName>` (e.g.
///     `pluginConfiguration.mcp.operation`).
pub fn interaction_from_value(interaction: &serde_json::Value) -> Result<crate::mcp::model::McpInteraction, String> {
    use crate::mcp::model::{McpInteraction, Operation, ServerHint};

    // Shape 1: single merged fragment.
    if let Some(mcp) = interaction.pointer("/contents/mcp").or_else(|| interaction.get("mcp")) {
        return serde_json::from_value(mcp.clone()).map_err(|e| e.to_string());
    }

    // Shape 2: real two-part sync message. Body lives under `contents.content`.
    let request = interaction
        .pointer("/request/contents/content")
        .or_else(|| interaction.pointer("/request/contents"))
        .cloned()
        .ok_or("interaction has neither contents.mcp nor request.contents.content")?;
    let response = interaction
        .pointer("/response/0/contents/content")
        .or_else(|| interaction.pointer("/response/0/contents"))
        .or_else(|| interaction.pointer("/response/contents/content"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let plugin_config = interaction
        .pointer("/pluginConfiguration")
        .and_then(|pc| pc.as_object())
        .and_then(|m| m.values().next());

    let operation = plugin_config
        .and_then(|v| v.get("operation"))
        .and_then(serde_json::Value::as_str)
        .and_then(Operation::parse)
        .unwrap_or_else(|| {
            if response.get("tools").is_some() {
                Operation::ToolsList
            } else {
                Operation::ToolsCall
            }
        });

    let server = plugin_config
        .and_then(|v| v.get("server"))
        .and_then(|s| s.get("transport"))
        .and_then(serde_json::Value::as_str)
        .map(|t| ServerHint { transport: t.to_string() });

    let mut mcp = McpInteraction::new(operation, request, response);
    mcp.server = server;
    Ok(mcp)
}

/// Extract the response-part matching rules from a persisted interaction (real
/// two-part shape: `response[0].matchingRules.body`), reshaped for the engine's
/// `content::Rules` (keys `$.<path>`, each `{matchers:[{match:...}]}`; the
/// persisted `combine` field is ignored). Returns `None` if there are none.
pub fn response_matching_rules(interaction: &serde_json::Value) -> Option<serde_json::Value> {
    let body = interaction
        .pointer("/response/0/matchingRules/body")
        .or_else(|| interaction.pointer("/matchingRules/response"))?;
    Some(body.clone())
}

/// Extract the request-part matching rules from a persisted interaction (real
/// two-part shape: `request.matchingRules.body`, keyed `$.<path>` rooted at the
/// request body, e.g. `$.arguments.city`), reshaped for `content::Rules`.
/// Returns `None` if there are none. Used by the mock to decide which
/// interaction an incoming `tools/call` matches (matching-semantics §4).
pub fn request_matching_rules(interaction: &serde_json::Value) -> Option<serde_json::Value> {
    let body = interaction
        .pointer("/request/matchingRules/body")
        .or_else(|| interaction.pointer("/matchingRules/request"))?;
    Some(body.clone())
}
