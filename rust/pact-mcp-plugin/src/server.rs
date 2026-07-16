//! `PactPlugin` gRPC service dispatch.
//!
//! Phase 1 scope: `InitPlugin`, `UpdateCatalogue`, `ConfigureInteraction`,
//! `CompareContents`, `GenerateContent`, `PrepareInteractionForVerification`,
//! `VerifyInteraction` (stdio only) are implemented for real.
//!
//! `StartMockServer` / `ShutdownMockServer` / `GetMockServerResults` implement
//! the stdio mock handoff (plan task 1.8 / §7.2): StartMockServer persists the
//! pact and returns a spawnable `{command, args, env}` (the client execs the
//! `mock` CLI subcommand as its stdio server); the mock writes results to a
//! file that Get/Shutdown read back.

use crate::catalogue;
use crate::config::{configure_interaction, rules_value};
use crate::content::{compare_response, Rules};
use crate::proto::pact_plugin_server::PactPlugin;
use crate::proto::*;
use crate::verify::{verify_interaction_stdio, StdioServerConfig};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tonic::{Request, Response, Status};

/// A prepared stdio mock session: where its pact + results files live so the
/// spawnable mock CLI can write results and Get/Shutdown can read them.
#[derive(Clone)]
struct MockSession {
    dir: PathBuf,
    results_path: PathBuf,
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
        // stdio MCP mocks are SPAWNED by the client (there is no listening
        // socket), so instead of a running server we return a spawnable handoff
        // (§7.2): we persist the pact to a session dir, and return the
        // `{command, args, env}` the client should exec, encoded as JSON in the
        // `address` field (the proto has no first-class field for it — see
        // ADR 0005 note). The spawned `mock` CLI writes results to a file that
        // Get/ShutdownMockServer read back.
        let req = request.into_inner();
        let key = uuid::Uuid::new_v4().to_string();
        let dir = std::env::temp_dir().join(format!("pact-mcp-mock-{key}"));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Ok(Response::new(StartMockServerResponse {
                response: Some(start_mock_server_response::Response::Error(e.to_string())),
            }));
        }
        let pact_path = dir.join("pact.json");
        let results_path = dir.join("results.json");
        if let Err(e) = std::fs::write(&pact_path, &req.pact) {
            return Ok(Response::new(StartMockServerResponse {
                response: Some(start_mock_server_response::Response::Error(e.to_string())),
            }));
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

        self.sessions.lock().unwrap().insert(
            key.clone(),
            MockSession { dir, results_path },
        );

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
        let results = match &session {
            Some(s) => read_mock_results(&s.results_path),
            None => vec![],
        };
        let ok = results.iter().all(|r| r.error.is_empty() && r.mismatches.is_empty());
        if let Some(s) = session {
            let _ = std::fs::remove_dir_all(&s.dir);
        }
        Ok(Response::new(ShutdownMockServerResponse { ok, results }))
    }

    async fn get_mock_server_results(
        &self,
        request: Request<MockServerRequest>,
    ) -> Result<Response<MockServerResults>, Status> {
        let key = request.into_inner().server_key;
        let results = match self.sessions.lock().unwrap().get(&key) {
            Some(s) => read_mock_results(&s.results_path),
            None => return Err(Status::not_found(format!("no mock session for key `{key}`"))),
        };
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

/// Read the mock CLI's results file (JSON array of `mock::MockResult`) and map
/// each to the proto `MockServerResult`. A missing file (mock still running /
/// no requests yet) yields an empty list.
fn read_mock_results(path: &std::path::Path) -> Vec<MockServerResult> {
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let parsed: Vec<crate::mock::MockResult> = serde_json::from_str(&raw).unwrap_or_default();
    parsed
        .into_iter()
        .map(|r| MockServerResult {
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
        })
        .collect()
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
///  2. The real pact-core two-part sync-message shape (VERIFIED requirement,
///     though the live FFI round trip was blocked — see ADR 0004):
///     `request.contents` + `response.contents` are the per-part bodies, and
///     `operation` lives in the interaction's `pluginConfiguration`
///     (`interactionConfiguration.operation`) or is inferred from the response
///     body shape.
pub(crate) fn interaction_from_value(interaction: &serde_json::Value) -> Result<crate::mcp::model::McpInteraction, String> {
    use crate::mcp::model::{McpInteraction, Operation, ServerHint};

    // Shape 1: single merged fragment.
    if let Some(mcp) = interaction.pointer("/contents/mcp").or_else(|| interaction.get("mcp")) {
        return serde_json::from_value(mcp.clone()).map_err(|e| e.to_string());
    }

    // Shape 2: two-part sync message.
    let request = interaction
        .pointer("/request/contents")
        .or_else(|| interaction.pointer("/request/contents/content"))
        .cloned()
        .ok_or("interaction has neither contents.mcp nor request.contents")?;
    let response = interaction
        .pointer("/response/0/contents")
        .or_else(|| interaction.pointer("/response/contents"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    // operation: from pluginConfiguration, else inferred from response shape.
    let operation = interaction
        .pointer("/pluginConfiguration")
        .and_then(|pc| pc.as_object())
        .and_then(|m| m.values().next())
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

    let server = interaction
        .pointer("/pluginConfiguration")
        .and_then(|pc| pc.as_object())
        .and_then(|m| m.values().next())
        .and_then(|v| v.get("server"))
        .and_then(|s| s.get("transport"))
        .and_then(serde_json::Value::as_str)
        .map(|t| ServerHint { transport: t.to_string() });

    let mut mcp = McpInteraction::new(operation, request, response);
    mcp.server = server;
    Ok(mcp)
}
