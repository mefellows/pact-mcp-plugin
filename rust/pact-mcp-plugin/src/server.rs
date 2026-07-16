//! `PactPlugin` gRPC service dispatch.
//!
//! Phase 1 scope: `InitPlugin`, `UpdateCatalogue`, `ConfigureInteraction`,
//! `CompareContents`, `GenerateContent`, `PrepareInteractionForVerification`,
//! `VerifyInteraction` (stdio only) are implemented for real.
//!
//! `StartMockServer` / `ShutdownMockServer` / `GetMockServerResults` are
//! **stubbed** (return `Status::unimplemented`) — the stdio mock-mode CLI
//! (plan task 1.8) is not wired into the gRPC surface in this run. See the
//! final report for details.

use crate::catalogue;
use crate::config::{configure_interaction, rules_value_for_root};
use crate::content::{compare_response, Rules};
use crate::mcp::model::McpFragment;
use crate::proto::pact_plugin_server::PactPlugin;
use crate::proto::*;
use crate::verify::{verify_interaction_stdio, StdioServerConfig};
use std::collections::HashMap;
use tonic::{Request, Response, Status};

#[derive(Default)]
pub struct McpPlugin;

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

        let expected_fragment: McpFragment = decode_body(expected)
            .map_err(|e| Status::invalid_argument(format!("invalid expected mcp body: {e}")))?;
        let actual_value: serde_json::Value = decode_body(actual)
            .map_err(|e| Status::invalid_argument(format!("invalid actual mcp body: {e}")))?;

        let rules_value = rules_value_for_root(&req.rules, "response");
        let rules = Rules::new(Some(&rules_value));

        let result = compare_response(
            expected_fragment.mcp.operation,
            &expected_fragment.mcp.response,
            &actual_value,
            &rules,
        );

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

        let interaction = InteractionResponse {
            contents: Some(Body {
                content_type: crate::mcp::model::CONTENT_TYPE.to_string(),
                content: Some(configured.body_bytes),
                content_type_hint: body::ContentTypeHint::Text as i32,
            }),
            rules: configured.rules,
            generators: HashMap::new(),
            message_metadata: None,
            plugin_configuration: None,
            interaction_markup: String::new(),
            interaction_markup_type: 0,
            part_name: String::new(),
            metadata_rules: HashMap::new(),
            metadata_generators: HashMap::new(),
        };

        Ok(Response::new(ConfigureInteractionResponse {
            error: String::new(),
            interaction: vec![interaction],
            plugin_configuration: None,
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
        _request: Request<StartMockServerRequest>,
    ) -> Result<Response<StartMockServerResponse>, Status> {
        Err(Status::unimplemented(
            "StartMockServer is not implemented in this Phase 1 build; see plan task 1.8 and the final report",
        ))
    }

    async fn shutdown_mock_server(
        &self,
        _request: Request<ShutdownMockServerRequest>,
    ) -> Result<Response<ShutdownMockServerResponse>, Status> {
        Err(Status::unimplemented("ShutdownMockServer is not implemented in this Phase 1 build"))
    }

    async fn get_mock_server_results(
        &self,
        _request: Request<MockServerRequest>,
    ) -> Result<Response<MockServerResults>, Status> {
        Err(Status::unimplemented("GetMockServerResults is not implemented in this Phase 1 build"))
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

    let mcp = interaction
        .pointer("/contents/mcp")
        .or_else(|| interaction.get("mcp"))
        .ok_or("interaction has no mcp contents")?;

    serde_json::from_value(mcp.clone()).map_err(|e| e.to_string())
}
