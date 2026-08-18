use std::collections::BTreeSet;

use agentx_runtime_contracts::{
    ApplyReceiptV1, ContentHash, DELEGATION_TOKEN_TTL_SECONDS, ExecutionCheckpointV1,
    ExecutionCollectionPageV1, ExecutionCommandV1, ExecutionDetailV1, ExecutionNodeV1,
    ExecutionRuntimeDetailsV1, ExecutionSearchPageV1, ExecutionSearchRequestV1, ExecutionWaitV1,
    PartialExecutionModeV1, RuntimeCommandApplyRequestV1, SideEffectResolutionV1, content_hash,
    issue_delegation_token, now_unix,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::Response,
    routing::{get, post},
};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    api_error::{ApiError, ApiResult},
    control_api::{Actor, ControlApiState},
};

pub fn routes() -> Router<ControlApiState> {
    Router::new()
        .route("/api/v1/executions", get(search_executions))
        .route("/api/v1/executions/{id}", get(get_execution))
        .route("/api/v1/executions/{id}/nodes", get(get_nodes))
        .route("/api/v1/executions/{id}/nodes/{node_id}", get(get_node))
        .route("/api/v1/executions/{id}/events", get(get_events))
        .route("/api/v1/executions/{id}/waits", get(get_waits))
        .route("/api/v1/executions/{id}/checkpoints", get(get_checkpoints))
        .route(
            "/api/v1/executions/{id}/runtime-details",
            get(get_runtime_details),
        )
        .route(
            "/api/v1/executions/{id}/artifacts/{artifact_id}",
            get(get_artifact),
        )
        .route("/api/v1/executions/{id}/cancel", post(cancel_execution))
        .route("/api/v1/executions/{id}/fork", post(fork_execution))
        .route(
            "/api/v1/executions/{id}/side-effect-confirmations",
            post(confirm_side_effect),
        )
        .route("/api/v1/executions/{id}/trace", get(get_trace))
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionListQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    application_id: Option<Uuid>,
    workflow_id: Option<Uuid>,
    status: Option<String>,
    search: Option<String>,
    cursor: Option<String>,
}

async fn search_executions(
    State(state): State<ControlApiState>,
    actor: Actor,
    Query(query): Query<ExecutionListQuery>,
) -> ApiResult<Json<Value>> {
    actor.require("execution:view")?;
    let page_size = query.page_size.unwrap_or(50).clamp(1, 100);
    let request = ExecutionSearchRequestV1 {
        api_version: 1,
        tenant_id: actor.tenant_id,
        application_ids: query.application_id.into_iter().collect(),
        workflow_ids: query.workflow_id.into_iter().collect(),
        statuses: query.status.into_iter().collect(),
        created_after: None,
        created_before: None,
        search: query.search,
        cursor: query.cursor,
        limit: page_size,
    };
    let request_hash = content_hash(&json!({"operation":"execution-search","request":request}))
        .map_err(ApiError::internal)?;
    let tenant_wide = request.application_ids.is_empty() && request.workflow_ids.is_empty();
    let token = delegation_token(
        &state,
        &actor,
        "runtime.query.executions",
        request.application_ids.iter().copied().collect(),
        request.workflow_ids.iter().copied().collect(),
        BTreeSet::new(),
        tenant_wide,
        request_hash,
    )?;
    let response = state
        .http
        .post(format!(
            "{}/internal/runtime/v1/query/executions:search",
            state.runtime_query_url
        ))
        .bearer_auth(token)
        .json(&request)
        .send()
        .await
        .map_err(runtime_unavailable)?;
    let page: ExecutionSearchPageV1 = runtime_json(response).await?;
    let mut items = Vec::with_capacity(page.items.len());
    for summary in page.items {
        items.push(summary_json(&state, actor.tenant_id, summary).await?);
    }
    Ok(Json(json!({
        "items": items,
        "page": query.page.unwrap_or(1),
        "pageSize": page_size,
        "total": page.total,
        "nextCursor": page.next,
        "snapshotId": page.snapshot_id,
    })))
}

async fn get_execution(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    actor.require("execution:view")?;
    let detail = runtime_execution_detail(&state, &actor, id).await?;
    let mut response = summary_json(&state, actor.tenant_id, detail.summary).await?;
    let object = response
        .as_object_mut()
        .ok_or_else(|| ApiError::internal("Execution response is not an object"))?;
    object.insert(
        "parentExecutionId".into(),
        json!(detail.parent_execution_id),
    );
    object.insert("output".into(), detail.output.unwrap_or(Value::Null));
    object.insert("error".into(), detail.error.unwrap_or(Value::Null));
    object.insert("stateVersion".into(), json!(detail.state_version));
    object.insert("admissionEpoch".into(), json!(detail.admission_epoch));
    object.insert("traceWatermark".into(), json!(detail.trace_watermark));
    Ok(Json(response))
}

async fn get_nodes(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    actor.require("execution:view")?;
    let page: ExecutionCollectionPageV1<ExecutionNodeV1> = runtime_execution_get(
        &state,
        &actor,
        id,
        "execution_nodes",
        &format!("/internal/runtime/v1/query/executions/{id}/nodes"),
    )
    .await?;
    Ok(Json(json!({
        "items": page.items.into_iter().map(|node| node_json(id, node)).collect::<Vec<_>>()
    })))
}

async fn get_node(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path((id, node_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Value>> {
    actor.require("execution:view")?;
    let node: ExecutionNodeV1 = runtime_execution_get(
        &state,
        &actor,
        id,
        "execution_node",
        &format!("/internal/runtime/v1/query/executions/{id}/nodes/{node_id}"),
    )
    .await?;
    Ok(Json(node_json(id, node)))
}

async fn get_events(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    actor.require("execution:view")?;
    let page: ExecutionCollectionPageV1<Value> = runtime_execution_get(
        &state,
        &actor,
        id,
        "execution_events",
        &format!("/internal/runtime/v1/query/executions/{id}/events"),
    )
    .await?;
    Ok(Json(json!({"items":page.items,"nextCursor":null})))
}

async fn get_waits(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    actor.require("execution:view")?;
    let page: ExecutionCollectionPageV1<ExecutionWaitV1> = runtime_execution_get(
        &state,
        &actor,
        id,
        "execution_waits",
        &format!("/internal/runtime/v1/query/executions/{id}/waits"),
    )
    .await?;
    let items = page
        .items
        .into_iter()
        .map(|wait| {
            json!({
                "id":wait.wait_id,"executionId":id,"nodeExecutionId":wait.node_execution_id,
                "waitKind":wait.wait_kind,"status":wait.status,"wakeAt":wait.wake_at,
                "timeoutAt":wait.timeout_at,"authenticationMode":"signed","resumeUrl":null
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({"items":items})))
}

async fn get_checkpoints(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    actor.require("execution:view")?;
    let page: ExecutionCollectionPageV1<ExecutionCheckpointV1> = runtime_execution_get(
        &state,
        &actor,
        id,
        "execution_checkpoints",
        &format!("/internal/runtime/v1/query/executions/{id}/checkpoints"),
    )
    .await?;
    let items = page
        .items
        .into_iter()
        .map(|checkpoint| json!({
            "id":checkpoint.checkpoint_id,"executionId":id,
            "nodeExecutionId":checkpoint.node_execution_id,
            "sequenceNumber":checkpoint.sequence_number,"checkpointType":checkpoint.checkpoint_type,
            "stateHash":checkpoint.state_hash,"activationCount":0,"deliveryCount":0,
            "createdAt":checkpoint.created_at
        }))
        .collect::<Vec<_>>();
    Ok(Json(json!({"items":items})))
}

async fn get_runtime_details(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    actor.require("trace:view")?;
    let details: ExecutionRuntimeDetailsV1 = runtime_execution_get(
        &state,
        &actor,
        id,
        "execution_runtime_details",
        &format!("/internal/runtime/v1/query/executions/{id}/runtime-details"),
    )
    .await?;
    let input_tokens = details
        .calls
        .iter()
        .map(|call| call.input_tokens)
        .sum::<u64>();
    let output_tokens = details
        .calls
        .iter()
        .map(|call| call.output_tokens)
        .sum::<u64>();
    let cost_micros = details
        .calls
        .iter()
        .map(|call| call.cost_micros)
        .sum::<u64>();
    Ok(Json(json!({
        "executionId":id,"inputTokens":input_tokens,"outputTokens":output_tokens,
        "costMicros":cost_micros,"agentRuns":details.agent_runs,"iterations":[],
        "calls":details.calls,"sandboxes":details.sandboxes,"attempts":details.attempts
    })))
}

async fn get_artifact(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path((id, artifact_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Response> {
    actor.require("trace:view")?;
    let path = format!("/internal/runtime/v1/query/executions/{id}/artifacts/{artifact_id}");
    let request_hash = content_hash(&json!({
        "operation":"execution_artifact","executionId":id
    }))
    .map_err(ApiError::internal)?;
    let token = delegation_token(
        &state,
        &actor,
        "runtime.query.execution",
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::from([id]),
        false,
        request_hash,
    )?;
    let upstream = state
        .http
        .get(format!("{}{}", state.runtime_query_url, path))
        .bearer_auth(token)
        .send()
        .await
        .map_err(runtime_unavailable)?;
    if !upstream.status().is_success() {
        return Err(runtime_response_error(upstream).await);
    }
    let status = StatusCode::from_u16(upstream.status().as_u16()).map_err(ApiError::internal)?;
    let mut builder = Response::builder().status(status);
    for name in [
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::CONTENT_DISPOSITION,
        header::ETAG,
    ] {
        if let Some(value) = upstream.headers().get(&name) {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(Body::from_stream(upstream.bytes_stream()))
        .map_err(ApiError::internal)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForkRequest {
    checkpoint_id: Uuid,
    mode: String,
    node_id: Option<String>,
    side_effect_decisions: Value,
    idempotency_key: Option<String>,
}

async fn cancel_execution(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    actor.require("execution:cancel")?;
    let detail = runtime_execution_detail(&state, &actor, id).await?;
    let request = RuntimeCommandApplyRequestV1 {
        api_version: 1,
        tenant_id: actor.tenant_id,
        command_id: Uuid::now_v7(),
        object_version: detail.state_version,
        idempotency_key: format!("bff:cancel:{id}:{}", detail.state_version),
        command: ExecutionCommandV1::Cancel {
            execution_id: id,
            expected_state_version: detail.state_version,
        },
    };
    apply_runtime_command(&state, &actor, id, &request).await?;
    Ok(Json(
        json!({"executionId":id,"status":"accepted","replayed":false}),
    ))
}

async fn fork_execution(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(input): Json<ForkRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    actor.require("execution:fork")?;
    let detail = runtime_execution_detail(&state, &actor, id).await?;
    let mode = match input.mode.as_str() {
        "whole" => PartialExecutionModeV1::Whole,
        "node" => PartialExecutionModeV1::Node,
        "to_node" => PartialExecutionModeV1::ToNode,
        "from_node" => PartialExecutionModeV1::FromNode,
        _ => {
            return Err(ApiError::bad_request(
                "INVALID_FORK_MODE",
                "Fork mode is invalid",
            ));
        }
    };
    let resolution = side_effect_resolution(&input.side_effect_decisions)?;
    let command_id = Uuid::now_v7();
    let request = RuntimeCommandApplyRequestV1 {
        api_version: 1,
        tenant_id: actor.tenant_id,
        command_id,
        object_version: detail.state_version,
        idempotency_key: input
            .idempotency_key
            .unwrap_or_else(|| format!("bff:fork:{id}:{command_id}")),
        command: ExecutionCommandV1::Fork {
            source_execution_id: id,
            checkpoint_id: input.checkpoint_id,
            mode,
            node_id: input.node_id,
            side_effect_resolution: resolution,
        },
    };
    let receipt = apply_runtime_command(&state, &actor, id, &request).await?;
    let fork_execution_id =
        agentx_runtime_contracts::deterministic_uuid(receipt.event_id, b"fork-execution");
    Ok((
        StatusCode::ACCEPTED,
        Json(
            json!({"executionId":fork_execution_id,"status":"accepted","replayed":receipt.replayed}),
        ),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SideEffectRequest {
    node_execution_id: Uuid,
    decision: String,
    idempotency_key: String,
}

async fn confirm_side_effect(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(input): Json<SideEffectRequest>,
) -> ApiResult<Json<Value>> {
    actor.require("execution:fork")?;
    let detail = runtime_execution_detail(&state, &actor, id).await?;
    let resolution = parse_resolution(&input.decision)?;
    let request = RuntimeCommandApplyRequestV1 {
        api_version: 1,
        tenant_id: actor.tenant_id,
        command_id: Uuid::now_v7(),
        object_version: detail.state_version,
        idempotency_key: input.idempotency_key,
        command: ExecutionCommandV1::SideEffectConfirmation {
            execution_id: id,
            node_execution_id: input.node_execution_id,
            resolution,
            expected_state_version: detail.state_version,
        },
    };
    apply_runtime_command(&state, &actor, id, &request).await?;
    Ok(Json(json!({"accepted":true,"replayed":false})))
}

#[derive(Default, Deserialize)]
struct TraceQuery {
    limit: Option<u32>,
}

async fn get_trace(
    State(state): State<ControlApiState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Query(query): Query<TraceQuery>,
) -> ApiResult<Json<Value>> {
    actor.require("trace:view")?;
    let detail = runtime_execution_detail(&state, &actor, id).await?;
    let request_hash = content_hash(&json!({
        "operation":"execution-trace","executionId":id,
        "expectedWatermark":detail.trace_watermark,"limit":query.limit.unwrap_or(200)
    }))
    .map_err(ApiError::internal)?;
    let token = delegation_token_with_audience(
        &state,
        &actor,
        "agentx-observability-query",
        "observability.trace.read",
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::from([id]),
        false,
        request_hash.clone(),
    )?;
    let response = state
        .http
        .get(format!(
            "{}/internal/observability/v1/executions/{id}/trace?expectedWatermark={}&limit={}",
            state.observability_query_url,
            detail.trace_watermark,
            query.limit.unwrap_or(200).clamp(1, 1000)
        ))
        .header("x-agentx-request-hash", request_hash.as_str())
        .bearer_auth(token)
        .send()
        .await
        .map_err(observability_unavailable)?;
    if response.status() == reqwest::StatusCode::ACCEPTED {
        return Err(ApiError::accepted(
            "TRACE_DELAYED",
            "Execution is complete but its trace has not reached the expected watermark",
            2,
        ));
    }
    let trace: agentx_runtime_contracts::ExecutionTraceV1 = observability_json(response).await?;
    Ok(Json(json!({
        "executionId":id,"traceId":detail.summary.trace_id,"events":trace.events,
        "nextCursor":null,"complete":trace.complete,"degraded":trace.degraded
    })))
}

async fn runtime_execution_detail(
    state: &ControlApiState,
    actor: &Actor,
    id: Uuid,
) -> ApiResult<ExecutionDetailV1> {
    let request_hash = content_hash(&json!({"operation":"get_execution","executionId":id}))
        .map_err(ApiError::internal)?;
    let token = delegation_token(
        state,
        actor,
        "runtime.query.execution",
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::from([id]),
        false,
        request_hash,
    )?;
    let response = state
        .http
        .get(format!(
            "{}/internal/runtime/v1/query/executions/{id}",
            state.runtime_query_url
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(runtime_unavailable)?;
    runtime_json(response).await
}

async fn runtime_execution_get<T: serde::de::DeserializeOwned>(
    state: &ControlApiState,
    actor: &Actor,
    id: Uuid,
    operation: &str,
    path: &str,
) -> ApiResult<T> {
    let request_hash = content_hash(&json!({"operation":operation,"executionId":id}))
        .map_err(ApiError::internal)?;
    let token = delegation_token(
        state,
        actor,
        "runtime.query.execution",
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::from([id]),
        false,
        request_hash,
    )?;
    let response = state
        .http
        .get(format!("{}{}", state.runtime_query_url, path))
        .bearer_auth(token)
        .send()
        .await
        .map_err(runtime_unavailable)?;
    runtime_json(response).await
}

async fn apply_runtime_command(
    state: &ControlApiState,
    actor: &Actor,
    execution_id: Uuid,
    request: &RuntimeCommandApplyRequestV1,
) -> ApiResult<ApplyReceiptV1> {
    let request_hash = content_hash(&json!({"operation":"runtime-command","request":request}))
        .map_err(ApiError::internal)?;
    let token = delegation_token(
        state,
        actor,
        "runtime.command.execution",
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::from([execution_id]),
        false,
        request_hash,
    )?;
    let response = state
        .http
        .post(format!(
            "{}/internal/runtime/v1/runtime-commands:apply",
            state.runtime_query_url
        ))
        .bearer_auth(token)
        .json(request)
        .send()
        .await
        .map_err(runtime_unavailable)?;
    runtime_json(response).await
}

#[allow(clippy::too_many_arguments)]
fn delegation_token(
    state: &ControlApiState,
    actor: &Actor,
    scope: &str,
    application_ids: BTreeSet<Uuid>,
    workflow_ids: BTreeSet<Uuid>,
    execution_ids: BTreeSet<Uuid>,
    tenant_wide: bool,
    request_hash: ContentHash,
) -> ApiResult<String> {
    delegation_token_with_audience(
        state,
        actor,
        "agentx-runtime-internal",
        scope,
        application_ids,
        workflow_ids,
        execution_ids,
        tenant_wide,
        request_hash,
    )
}

#[allow(clippy::too_many_arguments)]
fn delegation_token_with_audience(
    state: &ControlApiState,
    actor: &Actor,
    audience: &str,
    scope: &str,
    application_ids: BTreeSet<Uuid>,
    workflow_ids: BTreeSet<Uuid>,
    execution_ids: BTreeSet<Uuid>,
    tenant_wide: bool,
    request_hash: ContentHash,
) -> ApiResult<String> {
    let now = now_unix();
    issue_delegation_token(
        &state.delegation_kid,
        state.delegation_key.expose_secret().as_bytes(),
        &agentx_runtime_contracts::DelegationClaimsV1 {
            iss: "agentx-control".into(),
            aud: audience.into(),
            sub: actor.user_id,
            tenant_id: actor.tenant_id,
            token_version: actor.token_version,
            tenant_wide,
            scope: BTreeSet::from([scope.into()]),
            application_ids,
            workflow_ids,
            execution_ids,
            session_ids: BTreeSet::new(),
            request_hash,
            iat: now,
            exp: now + DELEGATION_TOKEN_TTL_SECONDS,
            jti: Uuid::now_v7(),
        },
    )
    .map_err(ApiError::internal)
}

async fn runtime_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> ApiResult<T> {
    if !response.status().is_success() {
        return Err(runtime_response_error(response).await);
    }
    response.json().await.map_err(ApiError::internal)
}

async fn runtime_response_error(response: reqwest::Response) -> ApiError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    tracing::warn!(%status, response_body=%body, "Runtime Query rejected BFF request");
    if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
        ApiError::unavailable("RUNTIME_QUERY_UNAVAILABLE", "Runtime Query is unavailable")
    } else if status == reqwest::StatusCode::NOT_FOUND {
        ApiError::not_found("Runtime object")
    } else {
        ApiError::forbidden("Runtime rejected the delegated request")
    }
}

async fn observability_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> ApiResult<T> {
    if !response.status().is_success() {
        tracing::warn!(status=%response.status(), "Observability rejected BFF request");
        return Err(ApiError::unavailable(
            "OBSERVABILITY_UNAVAILABLE",
            "Observability query is unavailable",
        ));
    }
    response.json().await.map_err(ApiError::internal)
}

fn runtime_unavailable(error: reqwest::Error) -> ApiError {
    tracing::warn!(%error, "Runtime Query is unavailable");
    ApiError::unavailable("RUNTIME_QUERY_UNAVAILABLE", "Runtime Query is unavailable")
}

fn observability_unavailable(error: reqwest::Error) -> ApiError {
    tracing::warn!(%error, "Observability Query is unavailable");
    ApiError::unavailable(
        "OBSERVABILITY_UNAVAILABLE",
        "Observability query is unavailable",
    )
}

async fn summary_json(
    state: &ControlApiState,
    tenant_id: Uuid,
    summary: agentx_runtime_contracts::ExecutionSummaryV1,
) -> ApiResult<Value> {
    let metadata = sqlx::query(
        "SELECT w.name,wv.version_number FROM workflows w LEFT JOIN workflow_versions wv ON wv.tenant_id=w.tenant_id AND wv.id=? WHERE w.tenant_id=? AND w.id=?",
    )
    .bind(summary.workflow_version_id)
    .bind(tenant_id)
    .bind(summary.workflow_id)
    .fetch_optional(&state.pool)
    .await?;
    let workflow_name = metadata
        .as_ref()
        .and_then(|row| row.try_get::<String, _>("name").ok())
        .unwrap_or_else(|| summary.workflow_id.to_string());
    let version = metadata
        .as_ref()
        .and_then(|row| row.try_get::<Option<u64>, _>("version_number").ok())
        .flatten();
    Ok(json!({
        "id":summary.execution_id,"workflowId":summary.workflow_id,"workflowName":workflow_name,
        "workflowVersionId":summary.workflow_version_id,"workflowVersionNumber":version,
        "invocationId":summary.invocation_id,"sessionId":summary.session_id,"traceId":summary.trace_id,
        "triggerType":summary.trigger_type,"executionType":"production","parentExecutionId":summary.parent_execution_id,
        "callerExecutionId":null,"forkCheckpointId":null,"status":summary.status,
        "startedAt":summary.created_at,"endedAt":summary.completed_at,"durationMs":summary.duration_ms,
        "costMicros":summary.cost_micros,"inputTokens":0,"outputTokens":0,
        "errorCode":summary.error_code,"errorMessage":null,"bundleId":summary.bundle_id
    }))
}

fn node_json(execution_id: Uuid, node: ExecutionNodeV1) -> Value {
    json!({
        "id":node.node_execution_id,"executionId":execution_id,"nodeId":node.node_id,
        "nodeName":node.node_name,"nodeType":node.node_type,"nodeVersion":node.node_version,
        "generation":0,"activationSlot":0,"runIndex":node.run_index,
        "iterationIndex":node.iteration_index,"status":node.status,"capability":node.capability,
        "sideEffectLevel":"none","input":node.input,"output":node.output,
        "errorCode":node.error_code,"errorMessage":node.error_message,"startedAt":node.created_at,
        "endedAt":null,"attempts":[],"lineage":[]
    })
}

fn parse_resolution(value: &str) -> ApiResult<SideEffectResolutionV1> {
    match value {
        "execute" => Ok(SideEffectResolutionV1::Execute),
        "reuse_output" => Ok(SideEffectResolutionV1::ReuseOutput),
        "dry_run" => Ok(SideEffectResolutionV1::DryRun),
        _ => Err(ApiError::bad_request(
            "INVALID_SIDE_EFFECT_RESOLUTION",
            "Side-effect resolution is invalid",
        )),
    }
}

fn side_effect_resolution(value: &Value) -> ApiResult<SideEffectResolutionV1> {
    value
        .as_str()
        .or_else(|| value.get("default").and_then(Value::as_str))
        .map(parse_resolution)
        .transpose()
        .map(|value| value.unwrap_or(SideEffectResolutionV1::Execute))
}
