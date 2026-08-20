use std::{env, fs, path::Path};

use agentx_domain::WorkflowDefinition;
use agentx_node_protocol::{
    InvocationCancellationStatus, InvocationResourceRequest, InvocationResourceResponse, Item,
    LifecycleRequest, LifecycleResponse, NodeActionRequest, NodeActionResult, NodeManifestVersion,
    NodeProtocolError, ProducedArtifact, ProviderOption, ProviderRequest, ProviderResponse,
    ResumeContract, ResumeKind,
};
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    routing::{get, post},
};
use schemars::JsonSchema;
use serde_json::{Map, Value, json};
use time::{Duration, OffsetDateTime};

#[derive(Clone)]
struct EchoState {
    auth_token: Option<String>,
    http: reqwest::Client,
}

type ProtocolResponse<T> = Result<Json<T>, (StatusCode, Json<NodeProtocolError>)>;

#[tokio::main]
async fn main() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("openapi") => {
            write_json(
                args.get(1).map_or("openapi/node-api.json", String::as_str),
                &openapi(),
            )?;
            return Ok(());
        }
        Some("schemas") => {
            write_schemas(args.get(1).map_or("schemas", String::as_str))?;
            return Ok(());
        }
        _ => {}
    }

    let state = EchoState {
        auth_token: env::var("AGENTX_REMOTE_NODE_AUTH_TOKEN")
            .ok()
            .filter(|value| !value.is_empty()),
        http: reqwest::Client::new(),
    };
    let router = Router::new()
        .route("/agentx/node/v1/actions/execute", post(execute))
        .route(
            "/agentx/node/v1/providers/{provider}/invoke",
            post(provider),
        )
        .route("/agentx/node/v1/lifecycle/{operation}", post(lifecycle))
        .route(
            "/agentx/node/v1/openapi.json",
            get(|| async { Json(openapi()) }),
        )
        .with_state(state);
    agentx_service_kit::serve("echo-node", router, Default::default()).await
}

async fn execute(
    State(state): State<EchoState>,
    headers: HeaderMap,
    Json(request): Json<NodeActionRequest>,
) -> ProtocolResponse<NodeActionResult> {
    authorize(&state, &headers)?;
    validate_protocol(&request.protocol_version)?;
    if request.deadline <= OffsetDateTime::now_utc() {
        return Err(protocol_error(
            StatusCode::REQUEST_TIMEOUT,
            "NODE_DEADLINE_EXCEEDED",
            "The node action deadline has expired",
        ));
    }
    if request.idempotency_key.is_empty() {
        return Err(protocol_error(
            StatusCode::BAD_REQUEST,
            "NODE_IDEMPOTENCY_KEY_REQUIRED",
            "idempotencyKey is required",
        ));
    }

    let mode = request
        .parameters
        .common
        .get("fixtureMode")
        .and_then(Value::as_str)
        .unwrap_or("echo");
    if mode == "delay" {
        let delay_ms = request
            .parameters
            .common
            .get("delayMs")
            .and_then(Value::as_u64)
            .unwrap_or(1_000)
            .min(30_000);
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }
    let result = match mode {
        "fail" => NodeActionResult::Failed {
            error: NodeProtocolError {
                code: request
                    .parameters
                    .common
                    .get("errorCode")
                    .and_then(Value::as_str)
                    .unwrap_or("ECHO_FAILURE")
                    .into(),
                message: "Echo node fixture failure".into(),
                retryable: request
                    .parameters
                    .common
                    .get("retryable")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                details: json!({"idempotencyKey":request.idempotency_key}),
            },
        },
        "suspend" => NodeActionResult::Suspended {
            resume: ResumeContract {
                kind: ResumeKind::Webhook,
                timeout_at: Some(OffsetDateTime::now_utc() + Duration::minutes(10)),
                allowed_output_ports: vec!["main".into(), "timed_out".into()],
                payload_schema: json!({"type":"object"}),
            },
            checkpoint: json!({"echo":true}),
        },
        "broker" => execute_broker_fixture(&state, &request).await?,
        "echo" | "delay" => NodeActionResult::Completed {
            outputs: vec![
                request
                    .inputs
                    .into_iter()
                    .flat_map(|input| input.items)
                    .collect(),
            ],
            artifacts: request
                .parameters
                .common
                .get("produceArtifact")
                .and_then(Value::as_bool)
                .filter(|value| *value)
                .map(|_| ProducedArtifact {
                    name: "echo.txt".into(),
                    artifact_handle: "fixture-artifact".into(),
                    content_type: Some("text/plain".into()),
                    size_bytes: 4,
                })
                .into_iter()
                .collect(),
        },
        _ => NodeActionResult::Failed {
            error: NodeProtocolError {
                code: "ECHO_FIXTURE_MODE_UNKNOWN".into(),
                message: format!("Unknown echo fixture mode {mode}"),
                retryable: false,
                details: json!({}),
            },
        },
    };
    Ok(Json(result))
}

async fn execute_broker_fixture(
    state: &EchoState,
    request: &NodeActionRequest,
) -> Result<NodeActionResult, (StatusCode, Json<NodeProtocolError>)> {
    let handle = request.credential_handles.first().ok_or_else(|| {
        protocol_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ECHO_CREDENTIAL_HANDLE_REQUIRED",
            "Broker fixture requires a Credential Handle",
        )
    })?;
    let response = state
        .http
        .post(&handle.broker_url)
        .json(&InvocationResourceRequest {
            handle: handle.handle.clone(),
            tenant_id: request.tenant_id,
            node_execution_id: request.node_execution_id,
            attempt_id: request.attempt_id,
        })
        .send()
        .await
        .map_err(|error| {
            protocol_error(
                StatusCode::BAD_GATEWAY,
                "ECHO_BROKER_UNAVAILABLE",
                error.to_string(),
            )
        })?;
    if !response.status().is_success() {
        return Err(protocol_error(
            StatusCode::BAD_GATEWAY,
            "ECHO_BROKER_REJECTED",
            format!("Broker returned {}", response.status()),
        ));
    }
    let credential = response
        .json::<InvocationResourceResponse>()
        .await
        .map_err(|error| {
            protocol_error(
                StatusCode::BAD_GATEWAY,
                "ECHO_BROKER_RESPONSE_INVALID",
                error.to_string(),
            )
        })?;
    let credential_type = match credential {
        InvocationResourceResponse::Credential {
            credential_type, ..
        } => credential_type,
        InvocationResourceResponse::Artifact { .. } => {
            return Err(protocol_error(
                StatusCode::BAD_GATEWAY,
                "ECHO_BROKER_RESOURCE_INVALID",
                "Credential Handle resolved to an Artifact",
            ));
        }
    };
    let cancellation_url = request.cancellation_url.as_ref().ok_or_else(|| {
        protocol_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ECHO_CANCELLATION_URL_REQUIRED",
            "Broker fixture requires a cancellation URL",
        )
    })?;
    let cancellation = state
        .http
        .get(cancellation_url)
        .send()
        .await
        .map_err(|error| {
            protocol_error(
                StatusCode::BAD_GATEWAY,
                "ECHO_CANCELLATION_UNAVAILABLE",
                error.to_string(),
            )
        })?;
    if !cancellation.status().is_success() {
        return Err(protocol_error(
            StatusCode::BAD_GATEWAY,
            "ECHO_CANCELLATION_REJECTED",
            format!("Cancellation endpoint returned {}", cancellation.status()),
        ));
    }
    let cancellation = cancellation
        .json::<InvocationCancellationStatus>()
        .await
        .map_err(|error| {
            protocol_error(
                StatusCode::BAD_GATEWAY,
                "ECHO_CANCELLATION_RESPONSE_INVALID",
                error.to_string(),
            )
        })?;
    Ok(NodeActionResult::Completed {
        outputs: vec![vec![Item {
            json: json!({
                "credentialResolved": true,
                "credentialType": credential_type,
                "leaseValid": cancellation.lease_valid,
                "cancellationRequested": cancellation.cancellation_requested
            }),
            ..Item::default()
        }]],
        artifacts: Vec::new(),
    })
}

async fn provider(
    State(state): State<EchoState>,
    headers: HeaderMap,
    AxumPath(provider): AxumPath<String>,
    Json(request): Json<ProviderRequest>,
) -> ProtocolResponse<ProviderResponse> {
    authorize(&state, &headers)?;
    validate_protocol(&request.protocol_version)?;
    if provider != request.provider {
        return Err(protocol_error(
            StatusCode::BAD_REQUEST,
            "NODE_PROVIDER_MISMATCH",
            "The provider path does not match the request",
        ));
    }
    Ok(Json(ProviderResponse {
        options: vec![ProviderOption {
            label: format!("{provider}:{}", request.operation),
            value: json!("echo"),
            metadata: json!({"protocolVersion":request.protocol_version}),
        }],
        next_cursor: None,
    }))
}

async fn lifecycle(
    State(state): State<EchoState>,
    headers: HeaderMap,
    AxumPath(operation): AxumPath<String>,
    Json(request): Json<LifecycleRequest>,
) -> ProtocolResponse<LifecycleResponse> {
    authorize(&state, &headers)?;
    validate_protocol(&request.protocol_version)?;
    let expected = format!("{:?}", request.operation).to_ascii_lowercase();
    if operation != expected {
        return Err(protocol_error(
            StatusCode::BAD_REQUEST,
            "NODE_LIFECYCLE_MISMATCH",
            "The lifecycle path does not match the request",
        ));
    }
    let state = if request.operation == agentx_node_protocol::LifecycleOperation::Poll {
        json!({
            "operation": operation,
            "nodeType": request.node_type,
            "eventId": request.configuration.get("eventId").cloned().unwrap_or_else(|| json!("echo-poll")),
            "input": request.configuration.get("pollInput").cloned().unwrap_or_else(|| json!({"source":"echo-poll"})),
        })
    } else {
        json!({"operation":operation,"nodeType":request.node_type})
    };
    Ok(Json(LifecycleResponse {
        accepted: true,
        state,
    }))
}

fn authorize(
    state: &EchoState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<NodeProtocolError>)> {
    let Some(expected) = state.auth_token.as_deref() else {
        return Ok(());
    };
    let actual = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    if constant_time_eq(actual.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(protocol_error(
            StatusCode::UNAUTHORIZED,
            "NODE_AUTHENTICATION_FAILED",
            "A valid node service bearer token is required",
        ))
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut different = left.len() ^ right.len();
    let size = left.len().max(right.len());
    for index in 0..size {
        different |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    different == 0
}

fn validate_protocol(version: &str) -> Result<(), (StatusCode, Json<NodeProtocolError>)> {
    if version == agentx_node_protocol::NODE_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(protocol_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_PROTOCOL_VERSION_UNSUPPORTED",
            "The requested node protocol version is not supported",
        ))
    }
}

fn protocol_error(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
) -> (StatusCode, Json<NodeProtocolError>) {
    (
        status,
        Json(NodeProtocolError {
            code: code.into(),
            message: message.into(),
            retryable: false,
            details: json!({}),
        }),
    )
}

fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::SchemaGenerator::default().into_root_schema_for::<T>())
        .expect("schema is JSON serializable")
}

fn component_schema<T: JsonSchema>(name: &str) -> Value {
    let mut value = schema::<T>();
    rewrite_schema_refs(&mut value, name);
    value
}

fn rewrite_schema_refs(value: &mut Value, component: &str) {
    match value {
        Value::Object(object) => {
            let rewritten = object
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|reference| reference.strip_prefix("#/$defs/"))
                .map(|name| format!("#/components/schemas/{component}/$defs/{name}"));
            if let Some(reference) = rewritten {
                object.insert("$ref".into(), json!(reference));
            }
            for value in object.values_mut() {
                rewrite_schema_refs(value, component);
            }
        }
        Value::Array(values) => {
            for value in values {
                rewrite_schema_refs(value, component);
            }
        }
        _ => {}
    }
}

fn openapi() -> Value {
    let schemas = Map::from_iter([
        (
            "NodeActionRequest".into(),
            component_schema::<NodeActionRequest>("NodeActionRequest"),
        ),
        (
            "NodeActionResult".into(),
            component_schema::<NodeActionResult>("NodeActionResult"),
        ),
        (
            "ProviderRequest".into(),
            component_schema::<ProviderRequest>("ProviderRequest"),
        ),
        (
            "ProviderResponse".into(),
            component_schema::<ProviderResponse>("ProviderResponse"),
        ),
        (
            "LifecycleRequest".into(),
            component_schema::<LifecycleRequest>("LifecycleRequest"),
        ),
        (
            "LifecycleResponse".into(),
            component_schema::<LifecycleResponse>("LifecycleResponse"),
        ),
        (
            "NodeProtocolError".into(),
            component_schema::<NodeProtocolError>("NodeProtocolError"),
        ),
    ]);
    json!({
        "openapi":"3.1.0",
        "info":{"title":"Agentx Node Protocol","version":"1.0.0"},
        "security":[{"bearerAuth":[]}],
        "paths":{
            "/agentx/node/v1/actions/execute":{"post":{"operationId":"executeNodeAction","requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/NodeActionRequest"}}}},"responses":{"200":{"description":"Node result","content":{"application/json":{"schema":{"$ref":"#/components/schemas/NodeActionResult"}}}},"400":{"description":"Invalid protocol request","content":{"application/json":{"schema":{"$ref":"#/components/schemas/NodeProtocolError"}}}},"401":{"description":"Authentication failed","content":{"application/json":{"schema":{"$ref":"#/components/schemas/NodeProtocolError"}}}}}}},
            "/agentx/node/v1/providers/{provider}/invoke":{"post":{"operationId":"invokeNodeProvider","parameters":[{"name":"provider","in":"path","required":true,"schema":{"type":"string"}}],"requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/ProviderRequest"}}}},"responses":{"200":{"description":"Provider options","content":{"application/json":{"schema":{"$ref":"#/components/schemas/ProviderResponse"}}}},"400":{"description":"Invalid provider request","content":{"application/json":{"schema":{"$ref":"#/components/schemas/NodeProtocolError"}}}}}}},
            "/agentx/node/v1/lifecycle/{operation}":{"post":{"operationId":"invokeNodeLifecycle","parameters":[{"name":"operation","in":"path","required":true,"schema":{"type":"string","enum":["activate","deactivate","poll","webhook","suspend","resume"]}}],"requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/LifecycleRequest"}}}},"responses":{"200":{"description":"Lifecycle result","content":{"application/json":{"schema":{"$ref":"#/components/schemas/LifecycleResponse"}}}},"400":{"description":"Invalid lifecycle request","content":{"application/json":{"schema":{"$ref":"#/components/schemas/NodeProtocolError"}}}}}}}
        },
        "components":{"securitySchemes":{"bearerAuth":{"type":"http","scheme":"bearer"}},"schemas":schemas}
    })
}

fn write_schemas(directory: &str) -> Result<()> {
    fs::create_dir_all(directory)
        .with_context(|| format!("failed to create schema directory {directory}"))?;
    for (name, value) in [
        (
            "workflow-definition.schema.json",
            schema::<WorkflowDefinition>(),
        ),
        ("node-manifest.schema.json", schema::<NodeManifestVersion>()),
        (
            "node-action-request.schema.json",
            schema::<NodeActionRequest>(),
        ),
        (
            "node-action-result.schema.json",
            schema::<NodeActionResult>(),
        ),
    ] {
        write_json(Path::new(directory).join(name), &value)?;
    }
    Ok(())
}

fn write_json(path: impl AsRef<Path>, value: &Value) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))
        .with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use agentx_domain::{ExecutionId, NodeExecutionId, TenantId, WorkflowVersionId};
    use agentx_node_protocol::{
        ExecutionMode, GroupedInput, Item, ItemSource, NODE_PROTOCOL_VERSION, ResolvedParameters,
        TraceContext,
    };
    use uuid::Uuid;

    use super::*;

    fn action_request() -> NodeActionRequest {
        NodeActionRequest {
            protocol_version: NODE_PROTOCOL_VERSION.into(),
            node_type: "echo".into(),
            node_version: 1,
            tenant_id: TenantId::new(),
            workflow_version_id: Some(WorkflowVersionId::new()),
            execution_id: ExecutionId::new(),
            node_execution_id: NodeExecutionId::new(),
            attempt_id: Uuid::now_v7(),
            run_index: 2,
            iteration_index: 1,
            mode: ExecutionMode::Manual,
            inputs: vec![GroupedInput {
                port: "main:0".into(),
                branch_index: 0,
                items: vec![Item {
                    json: json!({"value":1}),
                    lineage: vec![ItemSource {
                        node_execution_id: NodeExecutionId::new(),
                        node_id: "source".into(),
                        run_index: 1,
                        output_index: 0,
                        item_index: 3,
                    }],
                    ..Item::default()
                }],
            }],
            parameters: ResolvedParameters {
                common: json!({"fixtureMode":"echo"}),
                per_item: vec![json!({"resolved":true})],
            },
            artifact_handles: vec![],
            credential_handles: vec![],
            idempotency_key: "fixture-key".into(),
            deadline: OffsetDateTime::now_utc() + Duration::minutes(1),
            cancellation_url: Some("http://coordinator/cancel".into()),
            trace_context: TraceContext {
                trace_id: Uuid::now_v7().to_string(),
                span_id: Uuid::now_v7().to_string(),
                trace_flags: Some("01".into()),
            },
        }
    }

    #[test]
    fn openapi_covers_all_typed_protocol_families() {
        let document = openapi();
        let paths = document["paths"].as_object().unwrap();
        assert_eq!(paths.len(), 3);
        for schema in [
            "NodeActionRequest",
            "NodeActionResult",
            "ProviderRequest",
            "ProviderResponse",
            "LifecycleRequest",
            "LifecycleResponse",
        ] {
            assert!(document["components"]["schemas"].get(schema).is_some());
        }
        let serialized = serde_json::to_string(&document).unwrap();
        assert!(!serialized.contains("\"$ref\":\"#/$defs/"));
    }

    #[tokio::test]
    async fn echo_action_is_idempotent_and_preserves_grouping_and_lineage() {
        let request = serde_json::to_value(action_request()).unwrap();
        let first = execute(
            State(EchoState {
                auth_token: None,
                http: reqwest::Client::new(),
            }),
            HeaderMap::new(),
            Json(serde_json::from_value(request.clone()).unwrap()),
        )
        .await
        .unwrap()
        .0;
        let serialized = serde_json::to_value(&first).unwrap();
        assert_eq!(serialized["status"], "completed");
        assert_eq!(serialized["outputs"][0][0]["json"]["value"], 1);
        assert_eq!(
            serialized["outputs"][0][0]["lineage"][0]["nodeId"],
            "source"
        );

        let replay = execute(
            State(EchoState {
                auth_token: None,
                http: reqwest::Client::new(),
            }),
            HeaderMap::new(),
            Json(serde_json::from_value(request).unwrap()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(
            serialized["outputs"],
            serde_json::to_value(replay).unwrap()["outputs"]
        );
    }

    #[tokio::test]
    async fn authentication_protocol_and_deadline_are_enforced() {
        let state = EchoState {
            auth_token: Some("secret".into()),
            http: reqwest::Client::new(),
        };
        let unauthorized = execute(
            State(state.clone()),
            HeaderMap::new(),
            Json(action_request()),
        )
        .await
        .unwrap_err();
        assert_eq!(unauthorized.0, StatusCode::UNAUTHORIZED);

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer secret".parse().unwrap());
        let mut expired = action_request();
        expired.deadline = OffsetDateTime::now_utc() - Duration::seconds(1);
        let expired = execute(State(state), headers, Json(expired))
            .await
            .unwrap_err();
        assert_eq!(expired.0, StatusCode::REQUEST_TIMEOUT);
    }

    #[tokio::test]
    async fn public_action_fixtures_are_protocol_conformant() {
        let request: NodeActionRequest =
            serde_json::from_str(include_str!("../fixtures/action-completed.request.json"))
                .unwrap();
        let result = execute(
            State(EchoState {
                auth_token: None,
                http: reqwest::Client::new(),
            }),
            HeaderMap::new(),
            Json(request),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(serde_json::to_value(result).unwrap()["status"], "completed");

        let invalid: NodeActionRequest = serde_json::from_str(include_str!(
            "../fixtures/action-invalid-protocol.request.json"
        ))
        .unwrap();
        let error = execute(
            State(EchoState {
                auth_token: None,
                http: reqwest::Client::new(),
            }),
            HeaderMap::new(),
            Json(invalid),
        )
        .await
        .unwrap_err();
        assert_eq!(error.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error.1.0.code, "NODE_PROTOCOL_VERSION_UNSUPPORTED");
    }

    #[test]
    fn generated_schemas_cover_definition_manifest_and_action_contracts() {
        let definition = schema::<WorkflowDefinition>();
        let manifest = schema::<NodeManifestVersion>();
        let request = schema::<NodeActionRequest>();
        let result = schema::<NodeActionResult>();
        assert_eq!(
            definition["$defs"]["WorkflowSchemaVersion"]["enum"][0],
            "5.0"
        );
        assert!(manifest["properties"].get("readiness").is_some());
        assert!(request["properties"].get("credentialHandles").is_some());
        assert!(result.get("oneOf").is_some());
    }

    #[test]
    fn constant_time_comparison_covers_different_lengths() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secrex"));
        assert!(!constant_time_eq(b"secret", b"secret-long"));
    }
}
