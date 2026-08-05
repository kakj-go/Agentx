use agentx_application::{
    ArtifactStore, ExecutionRuntime, ForkExecutionCommand, RequestExecution,
    RuntimeResourceSnapshot, SideEffectConfirmationCommand,
};
use agentx_domain::{
    ArtifactId, CheckpointId, ExecutionId, NodeExecutionId, TenantId, UserId, WorkflowDefinition,
    WorkflowId, WorkflowVersionId, canonical_content_hash,
};
use agentx_infrastructure::artifact::MySqlObjectArtifactStore;
use agentx_runtime::{CompileContext, WorkflowCompiler};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use std::collections::{HashMap, HashSet, VecDeque};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    control_common::require_workflow_access,
    error::{AppError, AppResult},
    security::AuthActor,
    state::AppState,
};

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartExecutionRequest {
    #[serde(default)]
    pub input: Value,
    pub idempotency_key: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DebugExecutionRequest {
    pub expected_revision: u64,
    pub mode: String,
    pub target_node_id: Option<String>,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub input_source: Value,
    #[serde(default)]
    pub overlay_ids: Vec<Uuid>,
    #[serde(default)]
    pub side_effect_decisions: Value,
    pub idempotency_key: Option<String>,
}

#[derive(Deserialize, IntoParams, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEventQuery {
    pub after: Option<u64>,
    pub limit: Option<u32>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEventResponse {
    pub sequence: u64,
    pub event_type: String,
    pub status: String,
    pub summary: Value,
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: OffsetDateTime,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEventListResponse {
    pub items: Vec<ExecutionEventResponse>,
    pub next_cursor: Option<u64>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionCommandResponse {
    pub execution_id: Uuid,
    pub status: String,
    pub replayed: bool,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeAttemptResponse {
    pub id: Uuid,
    pub attempt_number: u32,
    pub status: String,
    pub worker_instance_id: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub deadline_at: Option<OffsetDateTime>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineageResponse {
    pub delivery_id: Uuid,
    pub target_item_index: u32,
    pub source_node_execution_id: Uuid,
    pub source_run_index: u32,
    pub source_output_index: u32,
    pub source_item_index: u32,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeExecutionResponse {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub node_id: String,
    pub node_name: String,
    pub node_type: String,
    pub node_version: u32,
    pub generation: u32,
    pub activation_slot: u32,
    pub run_index: u32,
    pub iteration_index: u32,
    pub status: String,
    pub capability: String,
    pub side_effect_level: String,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub ended_at: Option<OffsetDateTime>,
    pub attempts: Vec<NodeAttemptResponse>,
    pub lineage: Vec<LineageResponse>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeExecutionListResponse {
    pub items: Vec<NodeExecutionResponse>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointResponse {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Option<Uuid>,
    pub sequence_number: u64,
    pub checkpoint_type: String,
    pub state_hash: String,
    pub activation_count: u64,
    pub delivery_count: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointListResponse {
    pub items: Vec<CheckpointResponse>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaitResponse {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub node_execution_id: Uuid,
    pub wait_kind: String,
    pub status: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub wake_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub timeout_at: Option<OffsetDateTime>,
    pub authentication_mode: String,
    pub resume_url: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaitListResponse {
    pub items: Vec<WaitResponse>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForkRequest {
    pub checkpoint_id: Uuid,
    pub mode: String,
    pub node_id: Option<String>,
    #[serde(default)]
    pub input_overrides: Value,
    #[serde(default)]
    pub side_effect_decisions: Value,
    pub idempotency_key: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SideEffectConfirmationRequest {
    pub node_execution_id: Uuid,
    pub checkpoint_id: Option<Uuid>,
    pub decision: String,
    pub idempotency_key: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdempotentCommandResponse {
    pub accepted: bool,
    pub replayed: bool,
}

#[utoipa::path(post,path="/api/v1/workflow-versions/{version_id}/executions",request_body=StartExecutionRequest,responses((status=202,body=ExecutionCommandResponse)))]
pub async fn start_execution(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(version_id): Path<Uuid>,
    Json(input): Json<StartExecutionRequest>,
) -> AppResult<(StatusCode, Json<ExecutionCommandResponse>)> {
    actor.require("execution:run")?;
    let workflow_id: Uuid =
        sqlx::query_scalar("SELECT workflow_id FROM workflow_versions WHERE tenant_id=? AND id=?")
            .bind(actor.tenant_id)
            .bind(version_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::not_found("Workflow Version"))?;
    require_workflow_access(&state.pool, &actor, workflow_id, true).await?;
    let runtime = runtime(&state)?;
    let accepted = runtime
        .request_execution(RequestExecution {
            tenant_id: TenantId::from_uuid(actor.tenant_id),
            invocation_id: None,
            session_id: None,
            source: agentx_domain::ExecutionSource::Version {
                version_id: WorkflowVersionId::from_uuid(version_id),
            },
            requested_by: Some(UserId::from_uuid(actor.user_id)),
            trigger_type: "manual".into(),
            input: input.input,
            debug_plan: serde_json::json!({}),
            debug_overlay: serde_json::json!({}),
            resource_snapshots: Vec::new(),
            idempotency_key: input.idempotency_key,
        })
        .await
        .map_err(runtime_error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ExecutionCommandResponse {
            execution_id: accepted.execution_id.as_uuid(),
            status: status_name(accepted.status).into(),
            replayed: false,
        }),
    ))
}

#[utoipa::path(post,path="/api/v1/workflows/{id}/debug-executions",request_body=DebugExecutionRequest,responses((status=202,body=ExecutionCommandResponse)),params(("id" = Uuid, Path)))]
pub async fn start_debug_execution(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<DebugExecutionRequest>,
) -> AppResult<(StatusCode, Json<ExecutionCommandResponse>)> {
    actor.require("execution:run")?;
    require_workflow_access(&state.pool, &actor, id, true).await?;
    if !matches!(
        input.mode.as_str(),
        "full" | "single_node" | "to_node" | "from_node"
    ) {
        return Err(AppError::bad_request(
            "INVALID_DEBUG_MODE",
            "Unsupported debug execution mode",
        ));
    }
    if input.mode != "full" && input.target_node_id.as_deref().is_none_or(str::is_empty) {
        return Err(AppError::unprocessable(
            "DEBUG_TARGET_REQUIRED",
            "Partial debug execution requires a target node",
        ));
    }
    if matches!(input.mode.as_str(), "single_node" | "from_node")
        && input
            .input_source
            .get("kind")
            .and_then(Value::as_str)
            .is_none()
    {
        return Err(AppError::unprocessable(
            "DEBUG_INPUT_SOURCE_REQUIRED",
            "This debug mode requires an explicit input source",
        ));
    }
    let current: u64 = sqlx::query_scalar(
        "SELECT revision FROM workflow_drafts WHERE tenant_id=? AND workflow_id=?",
    )
    .bind(actor.tenant_id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Workflow draft"))?;
    if current != input.expected_revision {
        return Err(AppError::conflict(
            "DRAFT_REVISION_CONFLICT",
            format!("Draft is now at revision {current}"),
        ));
    }
    let definition_value: Value = sqlx::query_scalar("SELECT definition_json FROM workflow_draft_revisions WHERE tenant_id=? AND workflow_id=? AND revision=?")
        .bind(actor.tenant_id).bind(id).bind(input.expected_revision).fetch_optional(&state.pool).await?.ok_or_else(|| AppError::not_found("Workflow Draft Revision"))?;
    let definition: WorkflowDefinition =
        serde_json::from_value(definition_value).map_err(AppError::internal)?;
    let registry = crate::catalog::registry_for_tenant(&state.pool, actor.tenant_id).await?;
    WorkflowCompiler::new(&registry)
        .compile(&definition, &CompileContext::default())
        .map_err(|error| AppError::unprocessable("WORKFLOW_COMPILE_FAILED", error.to_string()))?;
    let included_node_ids =
        debug_included_node_ids(&definition, &input.mode, input.target_node_id.as_deref())
            .map_err(|message| AppError::unprocessable("DEBUG_PLAN_INVALID", message))?;
    let included = included_node_ids.iter().cloned().collect::<HashSet<_>>();
    validate_side_effect_decisions(
        &definition,
        &included,
        &registry,
        &input.side_effect_decisions,
    )?;
    let mut overlays = Vec::new();
    let mut overlay_node_ids = HashSet::new();
    let mut seen_overlay_ids = HashSet::new();
    for overlay_id in &input.overlay_ids {
        if !seen_overlay_ids.insert(*overlay_id) {
            return Err(AppError::unprocessable(
                "DEBUG_OVERLAY_DUPLICATE",
                format!("Overlay {overlay_id} was selected more than once"),
            ));
        }
        let row = sqlx::query("SELECT node_id,kind,payload_json,artifact_id,schema_hash FROM workflow_debug_overlays WHERE tenant_id=? AND workflow_id=? AND id=? AND stale=FALSE")
            .bind(actor.tenant_id).bind(id).bind(overlay_id).fetch_optional(&state.pool).await?.ok_or_else(|| AppError::unprocessable("DEBUG_OVERLAY_INVALID", format!("Overlay {overlay_id} is missing or stale")))?;
        let node_id: String = row.try_get("node_id")?;
        if !included.contains(&node_id) {
            return Err(AppError::unprocessable(
                "DEBUG_OVERLAY_OUTSIDE_PLAN",
                format!("Overlay {overlay_id} targets a node outside the debug plan"),
            ));
        }
        if !overlay_node_ids.insert(node_id.clone()) {
            return Err(AppError::unprocessable(
                "DEBUG_OVERLAY_DUPLICATE_NODE",
                format!("More than one overlay targets node '{node_id}'"),
            ));
        }
        let expected_schema_hash =
            crate::workflow_studio::node_overlay_schema_hash(&definition, &registry, &node_id)?;
        let stored_schema_hash: Option<String> = row.try_get("schema_hash")?;
        if stored_schema_hash.as_deref() != Some(expected_schema_hash.as_str()) {
            sqlx::query("UPDATE workflow_debug_overlays SET stale=TRUE WHERE tenant_id=? AND workflow_id=? AND id=?")
                .bind(actor.tenant_id).bind(id).bind(overlay_id).execute(&state.pool).await?;
            return Err(AppError::unprocessable(
                "DEBUG_OVERLAY_STALE",
                format!("Overlay {overlay_id} no longer matches node '{node_id}'"),
            ));
        }
        let kind: String = row.try_get("kind")?;
        let stored_payload: Value = row.try_get("payload_json")?;
        let artifact_id: Option<Uuid> = row.try_get("artifact_id")?;
        let payload =
            resolve_overlay_payload(&state, &actor, id, &kind, stored_payload, artifact_id).await?;
        overlays.push(serde_json::json!({
            "id": overlay_id,
            "nodeId": node_id,
            "kind": kind,
            "payload": payload,
            "artifactId": artifact_id,
            "schemaHash": expected_schema_hash,
        }));
    }
    let overlay_snapshot = serde_json::json!({"items":overlays});
    let overlay_hash = canonical_content_hash(&overlay_snapshot).map_err(AppError::internal)?;
    let skipped_node_ids = definition
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .filter(|node_id| !included.contains(node_id))
        .collect::<Vec<_>>();
    let debug_plan = serde_json::json!({"mode":input.mode,"targetNodeId":input.target_node_id,"includedNodeIds":included_node_ids,"skippedNodeIds":skipped_node_ids,"inputSource":input.input_source,"overlayIds":input.overlay_ids,"overlayHash":overlay_hash,"sideEffectDecisions":input.side_effect_decisions});
    let resolved_input =
        resolve_debug_input(&state, &actor, id, &input.input_source, input.input).await?;
    let resource_snapshots = crate::grants::validate_and_snapshot(&state, &actor, id, &definition)
        .await?
        .into_iter()
        .map(|snapshot| RuntimeResourceSnapshot {
            node_id: snapshot.node_id,
            reference: snapshot.reference,
            snapshot_hash: snapshot.snapshot_hash,
            snapshot: snapshot.snapshot,
        })
        .collect();
    let accepted = runtime(&state)?
        .request_execution(RequestExecution {
            tenant_id: TenantId::from_uuid(actor.tenant_id),
            invocation_id: None,
            session_id: None,
            source: agentx_domain::ExecutionSource::DraftRevision {
                workflow_id: WorkflowId::from_uuid(id),
                revision: input.expected_revision,
            },
            requested_by: Some(UserId::from_uuid(actor.user_id)),
            trigger_type: "manual_debug".into(),
            input: resolved_input,
            debug_plan,
            debug_overlay: overlay_snapshot,
            resource_snapshots,
            idempotency_key: input.idempotency_key,
        })
        .await
        .map_err(runtime_error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ExecutionCommandResponse {
            execution_id: accepted.execution_id.as_uuid(),
            status: status_name(accepted.status).into(),
            replayed: false,
        }),
    ))
}

#[utoipa::path(get,path="/api/v1/executions/{id}/events",params(("id" = Uuid, Path),ExecutionEventQuery),responses((status=200,body=ExecutionEventListResponse)))]
pub async fn list_execution_events(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Query(query): Query<ExecutionEventQuery>,
) -> AppResult<Json<ExecutionEventListResponse>> {
    actor.require("execution:view")?;
    let workflow_id = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow_id, false).await?;
    let after = query.after.unwrap_or_default();
    let limit = query.limit.unwrap_or(200).clamp(1, 200);
    let rows = sqlx::query("SELECT sequence_number,event_type,status,summary_json,occurred_at FROM execution_events WHERE tenant_id=? AND execution_id=? AND sequence_number>? ORDER BY sequence_number LIMIT ?")
        .bind(actor.tenant_id).bind(id).bind(after).bind(limit).fetch_all(&state.pool).await?;
    let items = rows
        .into_iter()
        .map(|row| {
            Ok(ExecutionEventResponse {
                sequence: row.try_get("sequence_number")?,
                event_type: row.try_get("event_type")?,
                status: row.try_get("status")?,
                summary: row.try_get("summary_json")?,
                occurred_at: row.try_get("occurred_at")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    let next_cursor = items.last().map(|item| item.sequence);
    Ok(Json(ExecutionEventListResponse { items, next_cursor }))
}

#[utoipa::path(post,path="/api/v1/executions/{id}/cancel",responses((status=204)))]
pub async fn cancel_execution(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    actor.require("execution:cancel")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, false).await?;
    runtime(&state)?
        .cancel_execution(
            TenantId::from_uuid(actor.tenant_id),
            ExecutionId::from_uuid(id),
        )
        .await
        .map_err(runtime_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get,path="/api/v1/executions/{id}/nodes",responses((status=200,body=NodeExecutionListResponse)))]
pub async fn list_nodes(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<NodeExecutionListResponse>> {
    actor.require("execution:view")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, false).await?;
    let rows = sqlx::query(&format!("{NODE_SELECT} ORDER BY n.id"))
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_all(&state.pool)
        .await?;
    let mut items = Vec::new();
    for row in rows {
        items.push(node_from_row(&state, actor.tenant_id, row, false).await?);
    }
    Ok(Json(NodeExecutionListResponse { items }))
}

#[utoipa::path(get,path="/api/v1/executions/{id}/nodes/{node_execution_id}",responses((status=200,body=NodeExecutionResponse)))]
pub async fn get_node(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((id, node_execution_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<NodeExecutionResponse>> {
    actor.require("execution:view")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, false).await?;
    let row = sqlx::query(&format!("{NODE_SELECT} AND n.id=?"))
        .bind(actor.tenant_id)
        .bind(id)
        .bind(node_execution_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Node Execution"))?;
    Ok(Json(
        node_from_row(&state, actor.tenant_id, row, true).await?,
    ))
}

#[utoipa::path(get,path="/api/v1/executions/{id}/checkpoints",responses((status=200,body=CheckpointListResponse)))]
pub async fn list_checkpoints(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<CheckpointListResponse>> {
    actor.require("execution:view")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, false).await?;
    let rows=sqlx::query("SELECT id,execution_id,node_execution_id,sequence_number,checkpoint_type,state_hash,payload_json,created_at FROM checkpoints WHERE tenant_id=? AND execution_id=? ORDER BY sequence_number").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    let items = rows
        .into_iter()
        .map(|row| {
            let payload: Value = row.try_get("payload_json")?;
            Ok(CheckpointResponse {
                id: row.try_get("id")?,
                execution_id: row.try_get("execution_id")?,
                node_execution_id: row.try_get("node_execution_id")?,
                sequence_number: row.try_get("sequence_number")?,
                checkpoint_type: row.try_get("checkpoint_type")?,
                state_hash: row.try_get("state_hash")?,
                activation_count: payload
                    .get("activations")
                    .and_then(Value::as_object)
                    .map_or(0, |v| v.len() as u64),
                delivery_count: payload
                    .get("deliveries")
                    .and_then(Value::as_array)
                    .map_or(0, |v| v.len() as u64),
                created_at: row.try_get("created_at")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(Json(CheckpointListResponse { items }))
}

#[utoipa::path(get,path="/api/v1/executions/{id}/waits",responses((status=200,body=WaitListResponse)))]
pub async fn list_waits(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
) -> AppResult<Json<WaitListResponse>> {
    actor.require("execution:view")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, false).await?;
    let rows=sqlx::query("SELECT id,execution_id,node_execution_id,wait_kind,status,wake_at,timeout_at,authentication_mode FROM wait_subscriptions WHERE tenant_id=? AND execution_id=? ORDER BY created_at").bind(actor.tenant_id).bind(id).fetch_all(&state.pool).await?;
    let items = rows
        .into_iter()
        .map(|row| {
            let wait_id: Uuid = row.try_get("id")?;
            let status: String = row.try_get("status")?;
            Ok(WaitResponse {
                id: wait_id,
                execution_id: row.try_get("execution_id")?,
                node_execution_id: row.try_get("node_execution_id")?,
                wait_kind: row.try_get("wait_kind")?,
                resume_url: (status == "waiting")
                    .then(|| format!("/gateway/v1/waits/{wait_id}/resume")),
                status,
                wake_at: row.try_get("wake_at")?,
                timeout_at: row.try_get("timeout_at")?,
                authentication_mode: row.try_get("authentication_mode")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(Json(WaitListResponse { items }))
}

#[utoipa::path(post,path="/api/v1/executions/{id}/fork",request_body=ForkRequest,responses((status=202,body=ExecutionCommandResponse)))]
pub async fn fork_execution(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<ForkRequest>,
) -> AppResult<(StatusCode, Json<ExecutionCommandResponse>)> {
    actor.require("execution:fork")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, true).await?;
    let accepted = runtime(&state)?
        .fork_execution(ForkExecutionCommand {
            tenant_id: TenantId::from_uuid(actor.tenant_id),
            source_execution_id: ExecutionId::from_uuid(id),
            checkpoint_id: CheckpointId::from_uuid(input.checkpoint_id),
            mode: input.mode,
            node_id: input.node_id,
            input_overrides: input.input_overrides,
            side_effect_decisions: input.side_effect_decisions,
            actor_user_id: UserId::from_uuid(actor.user_id),
            idempotency_key: input.idempotency_key,
        })
        .await
        .map_err(runtime_error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ExecutionCommandResponse {
            execution_id: accepted.execution_id.as_uuid(),
            status: status_name(accepted.status).into(),
            replayed: false,
        }),
    ))
}

#[utoipa::path(post,path="/api/v1/executions/{id}/side-effect-confirmations",request_body=SideEffectConfirmationRequest,responses((status=200,body=IdempotentCommandResponse)))]
pub async fn confirm_side_effect(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(id): Path<Uuid>,
    Json(input): Json<SideEffectConfirmationRequest>,
) -> AppResult<Json<IdempotentCommandResponse>> {
    actor.require("execution:fork")?;
    let workflow = execution_workflow(&state, &actor, id).await?;
    require_workflow_access(&state.pool, &actor, workflow, true).await?;
    let replayed = runtime(&state)?
        .confirm_side_effect(SideEffectConfirmationCommand {
            tenant_id: TenantId::from_uuid(actor.tenant_id),
            execution_id: ExecutionId::from_uuid(id),
            node_execution_id: NodeExecutionId::from_uuid(input.node_execution_id),
            checkpoint_id: input.checkpoint_id.map(CheckpointId::from_uuid),
            decision: input.decision,
            actor_user_id: UserId::from_uuid(actor.user_id),
            idempotency_key: input.idempotency_key,
        })
        .await
        .map_err(runtime_error)?;
    Ok(Json(IdempotentCommandResponse {
        accepted: true,
        replayed,
    }))
}

const NODE_SELECT: &str = "SELECT n.id,n.execution_id,n.node_id,n.node_name,n.node_type,n.node_version,n.generation,n.activation_slot,n.run_index,n.iteration_index,n.status,n.capability,n.side_effect_level,n.input_json,n.output_json,n.error_code,n.error_message,n.started_at,n.ended_at FROM node_executions n WHERE n.tenant_id=? AND n.execution_id=?";
async fn node_from_row(
    state: &AppState,
    tenant: Uuid,
    row: sqlx::mysql::MySqlRow,
    detail: bool,
) -> AppResult<NodeExecutionResponse> {
    let id: Uuid = row.try_get("id")?;
    let attempts=sqlx::query("SELECT id,attempt_number,status,worker_instance_id,deadline_at,error_code,error_message,started_at,ended_at FROM node_attempts WHERE tenant_id=? AND node_execution_id=? ORDER BY attempt_number").bind(tenant).bind(id).fetch_all(&state.pool).await?.into_iter().map(|r|Ok(NodeAttemptResponse{id:r.try_get("id")?,attempt_number:r.try_get("attempt_number")?,status:r.try_get("status")?,worker_instance_id:r.try_get("worker_instance_id")?,deadline_at:r.try_get("deadline_at")?,error_code:r.try_get("error_code")?,error_message:r.try_get("error_message")?,started_at:r.try_get("started_at")?,ended_at:r.try_get("ended_at")?})).collect::<Result<Vec<_>,sqlx::Error>>()?;
    let lineage = if detail {
        sqlx::query("SELECT delivery_id,target_item_index,source_node_execution_id,source_run_index,source_output_index,source_item_index FROM item_lineage WHERE tenant_id=? AND execution_id=? AND delivery_id IN (SELECT id FROM execution_edge_deliveries WHERE source_node_execution_id=? OR target_node_id=?) ORDER BY delivery_id,target_item_index").bind(tenant).bind(row.try_get::<Uuid,_>("execution_id")?).bind(id).bind(row.try_get::<String,_>("node_id")?).fetch_all(&state.pool).await?.into_iter().map(|r|Ok(LineageResponse{delivery_id:r.try_get("delivery_id")?,target_item_index:r.try_get("target_item_index")?,source_node_execution_id:r.try_get("source_node_execution_id")?,source_run_index:r.try_get("source_run_index")?,source_output_index:r.try_get("source_output_index")?,source_item_index:r.try_get("source_item_index")?})).collect::<Result<Vec<_>,sqlx::Error>>()?
    } else {
        Vec::new()
    };
    Ok(NodeExecutionResponse {
        id,
        execution_id: row.try_get("execution_id")?,
        node_id: row.try_get("node_id")?,
        node_name: row.try_get("node_name")?,
        node_type: row.try_get("node_type")?,
        node_version: row.try_get("node_version")?,
        generation: row.try_get("generation")?,
        activation_slot: row.try_get("activation_slot")?,
        run_index: row.try_get("run_index")?,
        iteration_index: row.try_get("iteration_index")?,
        status: row.try_get("status")?,
        capability: row.try_get("capability")?,
        side_effect_level: row.try_get("side_effect_level")?,
        input: row.try_get("input_json")?,
        output: row.try_get("output_json")?,
        error_code: row.try_get("error_code")?,
        error_message: row.try_get("error_message")?,
        started_at: row.try_get("started_at")?,
        ended_at: row.try_get("ended_at")?,
        attempts,
        lineage,
    })
}
async fn execution_workflow(state: &AppState, actor: &AuthActor, id: Uuid) -> AppResult<Uuid> {
    sqlx::query_scalar("SELECT workflow_id FROM workflow_executions WHERE tenant_id=? AND id=?")
        .bind(actor.tenant_id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::not_found("Execution"))
}

async fn resolve_debug_input(
    state: &AppState,
    actor: &AuthActor,
    workflow_id: Uuid,
    source: &Value,
    fallback: Value,
) -> AppResult<Value> {
    match source.get("kind").and_then(Value::as_str) {
        None | Some("manual") => Ok(source.get("value").cloned().unwrap_or(fallback)),
        Some("history_output") => {
            let execution_id = source_uuid(source, "executionId")?;
            let node_execution_id = source_uuid(source, "nodeExecutionId")?;
            let output: Value = sqlx::query_scalar("SELECT n.output_json FROM node_executions n JOIN workflow_executions e ON e.id=n.execution_id AND e.tenant_id=n.tenant_id WHERE n.tenant_id=? AND n.id=? AND n.execution_id=? AND e.workflow_id=? AND n.status='succeeded'")
                .bind(actor.tenant_id).bind(node_execution_id).bind(execution_id).bind(workflow_id)
                .fetch_optional(&state.pool).await?.ok_or_else(|| AppError::unprocessable("DEBUG_INPUT_SOURCE_INVALID", "Historical node output was not found"))?;
            let port = source
                .get("outputPort")
                .and_then(Value::as_str)
                .unwrap_or("main");
            Ok(history_output_items(output.get(port).unwrap_or(&output)))
        }
        Some("artifact") => {
            let execution_id = source_uuid(source, "executionId")?;
            let artifact_id = source_uuid(source, "artifactId")?;
            ensure_execution_artifact(
                &state.pool,
                actor.tenant_id,
                workflow_id,
                execution_id,
                artifact_id,
            )
            .await?;
            load_artifact_json(state, actor.tenant_id, artifact_id).await
        }
        Some("checkpoint") => Err(AppError::unprocessable(
            "DEBUG_CHECKPOINT_REQUIRES_FORK",
            "Use the checkpoint Fork command to resume from checkpoint state",
        )),
        Some(kind) => Err(AppError::unprocessable(
            "DEBUG_INPUT_SOURCE_INVALID",
            format!("Unsupported debug input source '{kind}'"),
        )),
    }
}

async fn resolve_overlay_payload(
    state: &AppState,
    actor: &AuthActor,
    workflow_id: Uuid,
    kind: &str,
    payload: Value,
    artifact_id: Option<Uuid>,
) -> AppResult<Value> {
    match kind {
        "artifact" => {
            let artifact_id = artifact_id.ok_or_else(|| {
                AppError::unprocessable(
                    "DEBUG_OVERLAY_ARTIFACT_REQUIRED",
                    "Artifact overlay is missing artifactId",
                )
            })?;
            let execution_id = source_uuid(&payload, "executionId")?;
            ensure_execution_artifact(
                &state.pool,
                actor.tenant_id,
                workflow_id,
                execution_id,
                artifact_id,
            )
            .await?;
            load_artifact_json(state, actor.tenant_id, artifact_id).await
        }
        "history_output" => {
            let mut source = payload;
            source
                .as_object_mut()
                .ok_or_else(|| {
                    AppError::unprocessable(
                        "DEBUG_OVERLAY_HISTORY_INVALID",
                        "Historical output overlay payload must be an object",
                    )
                })?
                .insert("kind".into(), Value::String("history_output".into()));
            resolve_debug_input(state, actor, workflow_id, &source, Value::Null).await
        }
        "pin_data" | "mock_output" | "temporary_input" => Ok(payload),
        _ => Err(AppError::unprocessable(
            "DEBUG_OVERLAY_INVALID",
            format!("Unsupported debug overlay kind '{kind}'"),
        )),
    }
}

async fn load_artifact_json(
    state: &AppState,
    tenant_id: Uuid,
    artifact_id: Uuid,
) -> AppResult<Value> {
    let objects = state.object_store.clone().ok_or_else(|| {
        AppError::service_unavailable(
            "ARTIFACT_STORE_UNAVAILABLE",
            "Artifact storage is unavailable",
        )
    })?;
    let artifact = MySqlObjectArtifactStore::new(state.pool.clone(), objects)
        .get(
            TenantId::from_uuid(tenant_id),
            ArtifactId::from_uuid(artifact_id),
        )
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::not_found("Debug input Artifact"))?;
    serde_json::from_slice(&artifact.content)
        .map_err(|error| AppError::unprocessable("DEBUG_INPUT_ARTIFACT_INVALID", error.to_string()))
}

pub(crate) async fn ensure_execution_artifact(
    pool: &sqlx::MySqlPool,
    tenant_id: Uuid,
    workflow_id: Uuid,
    execution_id: Uuid,
    artifact_id: Uuid,
) -> AppResult<()> {
    let linked: bool = sqlx::query_scalar(EXECUTION_ARTIFACT_LINK_QUERY)
        .bind(tenant_id)
        .bind(workflow_id)
        .bind(execution_id)
        .bind(artifact_id)
        .bind(artifact_id)
        .bind(artifact_id)
        .bind(artifact_id)
        .fetch_one(pool)
        .await?;
    if linked {
        Ok(())
    } else {
        Err(AppError::not_found("Execution Artifact"))
    }
}

const EXECUTION_ARTIFACT_LINK_QUERY: &str = "SELECT EXISTS(SELECT 1 FROM workflow_executions e WHERE e.tenant_id=? AND e.workflow_id=? AND e.id=? AND (EXISTS(SELECT 1 FROM runtime_calls c WHERE c.tenant_id=e.tenant_id AND c.execution_id=e.id AND c.response_artifact_id=?) OR EXISTS(SELECT 1 FROM agent_runs r WHERE r.tenant_id=e.tenant_id AND r.execution_id=e.id AND r.state_artifact_id=?) OR EXISTS(SELECT 1 FROM agent_iterations i JOIN agent_runs r ON r.id=i.agent_run_id AND r.tenant_id=i.tenant_id WHERE i.tenant_id=e.tenant_id AND r.execution_id=e.id AND i.state_artifact_id=?) OR EXISTS(SELECT 1 FROM checkpoint_artifacts ca JOIN checkpoints c ON c.id=ca.checkpoint_id AND c.tenant_id=ca.tenant_id WHERE ca.tenant_id=e.tenant_id AND c.execution_id=e.id AND ca.artifact_id=?)))";

fn validate_side_effect_decisions(
    definition: &WorkflowDefinition,
    included: &HashSet<String>,
    registry: &agentx_runtime::NodeRegistry,
    decisions: &Value,
) -> AppResult<()> {
    let decisions = decisions.as_object();
    for node in &definition.nodes {
        if !included.contains(&node.id)
            || !registry
                .get(&node.node_type, node.type_version)
                .is_some_and(|manifest| {
                    manifest.side_effect_level
                        == agentx_node_protocol::SideEffectLevel::Irreversible
                })
        {
            continue;
        }
        let decision = decisions
            .and_then(|values| values.get(&node.id))
            .and_then(Value::as_str);
        match decision {
            None => {
                return Err(AppError::unprocessable(
                    "SIDE_EFFECT_DECISION_REQUIRED",
                    format!(
                        "Node '{}' requires an explicit side-effect decision",
                        node.id
                    ),
                ));
            }
            Some("execute") => {}
            Some(_) => {
                return Err(AppError::unprocessable(
                    "SIDE_EFFECT_DECISION_INVALID",
                    format!("Node '{}' requires the 'execute' decision", node.id),
                ));
            }
        }
    }
    Ok(())
}

fn source_uuid(source: &Value, field: &str) -> AppResult<Uuid> {
    source
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::unprocessable("DEBUG_INPUT_SOURCE_INVALID", format!("{field} is required"))
        })?
        .parse()
        .map_err(|_| {
            AppError::unprocessable("DEBUG_INPUT_SOURCE_INVALID", format!("{field} is invalid"))
        })
}

fn history_output_items(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| item.get("json").cloned().unwrap_or_else(|| item.clone()))
                .collect(),
        ),
        item => item.get("json").cloned().unwrap_or_else(|| item.clone()),
    }
}
fn runtime(state: &AppState) -> AppResult<&dyn ExecutionRuntime> {
    state.runtime.as_deref().ok_or_else(|| {
        AppError::service_unavailable("RUNTIME_UNAVAILABLE", "Workflow runtime is unavailable")
    })
}
fn runtime_error(error: anyhow::Error) -> AppError {
    let message = error.to_string();
    if message.contains("not found") {
        AppError::not_found("Runtime resource")
    } else if message.contains("INVALID")
        || message.contains("IDEMPOTENCY")
        || message.contains("SIDE_EFFECT")
    {
        AppError::conflict("RUNTIME_COMMAND_REJECTED", message)
    } else {
        AppError::service_unavailable("RUNTIME_UNAVAILABLE", "Workflow runtime is unavailable")
    }
}
fn status_name(status: agentx_domain::ExecutionStatus) -> &'static str {
    match status {
        agentx_domain::ExecutionStatus::Created => "created",
        agentx_domain::ExecutionStatus::Queued => "queued",
        agentx_domain::ExecutionStatus::Running => "running",
        agentx_domain::ExecutionStatus::Waiting => "waiting",
        agentx_domain::ExecutionStatus::WaitingApproval => "waiting_approval",
        agentx_domain::ExecutionStatus::Suspended => "suspended",
        agentx_domain::ExecutionStatus::Succeeded => "succeeded",
        agentx_domain::ExecutionStatus::Failed => "failed",
        agentx_domain::ExecutionStatus::Cancelled => "cancelled",
        agentx_domain::ExecutionStatus::TimedOut => "timed_out",
    }
}

fn debug_included_node_ids(
    definition: &WorkflowDefinition,
    mode: &str,
    target: Option<&str>,
) -> Result<Vec<String>, String> {
    let enabled = definition
        .nodes
        .iter()
        .filter(|node| !node.disabled)
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    if mode == "full" {
        return Ok(definition
            .nodes
            .iter()
            .filter(|node| enabled.contains(&node.id))
            .map(|node| node.id.clone())
            .collect());
    }
    let target = target
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Partial debug execution requires a target node".to_owned())?;
    if !enabled.contains(target) {
        return Err(format!("Debug target '{target}' is missing or disabled"));
    }
    let mut forward: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut reverse: HashMap<&str, Vec<&str>> = HashMap::new();
    for connection in &definition.connections {
        if enabled.contains(&connection.source_node_id)
            && enabled.contains(&connection.target_node_id)
        {
            forward
                .entry(&connection.source_node_id)
                .or_default()
                .push(&connection.target_node_id);
            reverse
                .entry(&connection.target_node_id)
                .or_default()
                .push(&connection.source_node_id);
        }
    }
    let mut included = HashSet::new();
    let mut queue = VecDeque::from([target]);
    while let Some(node_id) = queue.pop_front() {
        if !included.insert(node_id) {
            continue;
        }
        match mode {
            "single_node" => {}
            "to_node" => queue.extend(reverse.get(node_id).into_iter().flatten().copied()),
            "from_node" => queue.extend(forward.get(node_id).into_iter().flatten().copied()),
            _ => return Err(format!("Unsupported debug mode '{mode}'")),
        }
    }
    let result = definition
        .nodes
        .iter()
        .filter(|node| included.contains(node.id.as_str()))
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    if result.is_empty() {
        return Err(format!("Debug target '{target}' is not reachable"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn public_status_names_match_database() {
        assert_eq!(
            status_name(agentx_domain::ExecutionStatus::WaitingApproval),
            "waiting_approval"
        );
    }

    #[test]
    fn partial_debug_plan_selects_only_the_requested_subgraph() {
        let definition: WorkflowDefinition = serde_json::from_value(json!({
            "schemaVersion": "3.0",
            "nodes": [
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger"},
                {"id":"left","type":"set","typeVersion":1,"name":"Left"},
                {"id":"target","type":"set","typeVersion":1,"name":"Target"},
                {"id":"right","type":"set","typeVersion":1,"name":"Right"}
            ],
            "connections": [
                {"id":"a","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"left","targetHandle":"main","order":0},
                {"id":"b","sourceNodeId":"left","sourceHandle":"main","targetNodeId":"target","targetHandle":"main","order":0},
                {"id":"c","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"right","targetHandle":"main","order":1}
            ],
            "settings": {}
        })).expect("valid definition");

        assert_eq!(
            debug_included_node_ids(&definition, "single_node", Some("left")).unwrap(),
            vec!["left"]
        );
        assert_eq!(
            debug_included_node_ids(&definition, "to_node", Some("target")).unwrap(),
            vec!["trigger", "left", "target"]
        );
        assert_eq!(
            debug_included_node_ids(&definition, "from_node", Some("left")).unwrap(),
            vec!["left", "target"]
        );
    }

    #[test]
    fn historical_output_unwraps_runtime_items_without_losing_plain_json() {
        assert_eq!(
            history_output_items(&json!([{"json":{"value":1}}, {"json":{"value":2}}])),
            json!([{"value":1}, {"value":2}])
        );
        assert_eq!(
            history_output_items(&json!({"json":{"value":3}})),
            json!({"value":3})
        );
        assert_eq!(
            history_output_items(&json!({"value":4})),
            json!({"value":4})
        );
    }

    #[test]
    fn irreversible_debug_nodes_require_the_exact_execute_decision() {
        let definition: WorkflowDefinition = serde_json::from_value(json!({
            "schemaVersion": "3.0",
            "nodes": [
                {"id":"trigger","type":"manual_trigger","typeVersion":1,"name":"Trigger"},
                {"id":"agent","type":"agent","typeVersion":1,"name":"Agent","resourceReferences":[{"bindingId":"model-binding","bindingRole":"ai_model","resourceType":"model","resourceId":"018f47a0-7e9c-7000-8000-000000000001","operation":"use"}]}
            ],
            "connections": [{"id":"a","sourceNodeId":"trigger","sourceHandle":"main","targetNodeId":"agent","targetHandle":"main","order":0}],
            "settings": {}
        })).expect("valid definition");
        let registry = agentx_runtime::NodeRegistry::m5_defaults();
        let included = HashSet::from(["agent".to_owned()]);

        let missing = validate_side_effect_decisions(&definition, &included, &registry, &json!({}))
            .unwrap_err();
        assert_eq!(missing.code, "SIDE_EFFECT_DECISION_REQUIRED");

        let invalid = validate_side_effect_decisions(
            &definition,
            &included,
            &registry,
            &json!({"agent":"allow"}),
        )
        .unwrap_err();
        assert_eq!(invalid.code, "SIDE_EFFECT_DECISION_INVALID");
        validate_side_effect_decisions(
            &definition,
            &included,
            &registry,
            &json!({"agent":"execute"}),
        )
        .unwrap();

        validate_side_effect_decisions(
            &definition,
            &HashSet::from(["trigger".to_owned()]),
            &registry,
            &json!({}),
        )
        .unwrap();
    }

    #[test]
    fn execution_artifact_query_is_scoped_to_tenant_workflow_and_execution() {
        for boundary in [
            "e.tenant_id=?",
            "e.workflow_id=?",
            "e.id=?",
            "c.execution_id=e.id",
            "r.execution_id=e.id",
            "c.execution_id=e.id AND ca.artifact_id=?",
        ] {
            assert!(
                EXECUTION_ARTIFACT_LINK_QUERY.contains(boundary),
                "missing Artifact ownership boundary: {boundary}"
            );
        }
        assert_eq!(EXECUTION_ARTIFACT_LINK_QUERY.matches('?').count(), 7);
    }
}
